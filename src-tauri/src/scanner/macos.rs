use super::{
    EntryKind, FileIdentity, InternalNode, MeasuredMetadata, PROGRESS_ENTRY_INTERVAL,
    PROGRESS_INTERVAL, PartialRanking, ScanCounters, ScanOutput, ScanProgress, ScanSemantics,
    finish_scan, observe_partial_file,
};
use crossbeam_channel::{self as channel, RecvTimeoutError};
use libc::{self, attribute_set_t, attrlist, attrreference_t};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const BUFFER_SIZE: usize = 256 * 1024;
const MAX_WORKERS: usize = 8;
const DEFAULT_PARALLELISM: usize = 4;
const LARGE_DIRECTORY_FILE_COUNT: usize = 256;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(25);
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const SF_FIRMLINK: u32 = 0x0080_0000;
const VREG: u32 = 1;
const VDIR: u32 = 2;
const VLNK: u32 = 5;

pub(super) enum NativeScanError {
    Unavailable,
    Fatal(String),
}

#[derive(Debug)]
struct NativeEntry {
    name: OsString,
    kind: EntryKind,
    measured: MeasuredMetadata,
    entry_error: u32,
    mount_point: bool,
    firmlink: bool,
    can_enforce_mount_boundary: bool,
}

#[derive(Debug)]
struct DirectoryTask {
    path: Arc<Path>,
    parent_id: usize,
}

#[derive(Debug)]
enum DirectoryReadError {
    Io(io::Error),
    Parse(String),
    Cancelled,
}

#[derive(Debug)]
enum WorkerMessage {
    Batch {
        path: Arc<Path>,
        parent_id: usize,
        entries: Vec<NativeEntry>,
    },
    Complete,
    Failed(DirectoryReadError),
}

pub(super) fn scan_path<F>(
    root: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: &mut F,
) -> Result<ScanOutput, NativeScanError>
where
    F: FnMut(ScanProgress),
{
    let root = root
        .canonicalize()
        .map_err(|error| fatal(format!("Could not open {}: {error}", root.display())))?;
    let root_metadata = root
        .metadata()
        .map_err(|error| fatal(format!("Could not read {}: {error}", root.display())))?;
    if !root_metadata.is_dir() {
        return Err(fatal("Choose a directory to scan."));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(fatal("Scan cancelled."));
    }

    let started_at = Instant::now();
    let root_node_path: Arc<Path> = Arc::from(root.clone());
    let mut nodes = vec![InternalNode::root(&root)];
    let mut counters = ScanCounters::default();
    let mut partial_ranking = PartialRanking::default();
    let mut pending_directories = Vec::new();
    let mut entries_since_progress = 0_u64;
    let mut last_progress_at = Instant::now();
    let mut buffer = vec![0_u64; BUFFER_SIZE / size_of::<u64>()];
    let mut attribute_list = requested_attributes();
    let root_directory = open_directory(&root)
        .map_err(|error| fatal(format!("Could not open {}: {error}", root.display())))?;
    let mut root_returned_entries = false;
    loop {
        match read_directory_batch(&root_directory, &cancel, &mut attribute_list, &mut buffer) {
            Ok(Some(entries)) => {
                root_returned_entries = true;
                ingest_entries(
                    entries,
                    &root,
                    0,
                    &mut nodes,
                    &mut counters,
                    &mut partial_ranking,
                    &mut pending_directories,
                    &mut entries_since_progress,
                    &mut last_progress_at,
                    started_at,
                    &cancel,
                    on_progress,
                )?;
            }
            Ok(None) => break,
            Err(DirectoryReadError::Io(error))
                if !root_returned_entries && is_unavailable(&error) =>
            {
                return Err(NativeScanError::Unavailable);
            }
            Err(DirectoryReadError::Io(error)) => {
                return Err(fatal(format!("Could not scan {}: {error}", root.display())));
            }
            Err(DirectoryReadError::Parse(error)) => return Err(fatal(error)),
            Err(DirectoryReadError::Cancelled) => return Err(fatal("Scan cancelled.")),
        }
    }

    traverse_directories(
        &mut pending_directories,
        &mut nodes,
        &mut counters,
        &mut partial_ranking,
        &mut entries_since_progress,
        &mut last_progress_at,
        started_at,
        &cancel,
        on_progress,
    )?;

    let traversal_completed_at = Instant::now();
    finish_scan(
        root,
        root_node_path,
        nodes,
        counters,
        partial_ranking,
        "getattrlistbulk",
        ScanSemantics {
            allocated_size_is_estimate: false,
            hard_link_deduplication_supported: true,
            same_filesystem_enforced: true,
        },
        started_at,
        traversal_completed_at,
        &cancel,
        on_progress,
    )
    .map_err(fatal)
}

#[allow(clippy::too_many_arguments)]
fn traverse_directories<F>(
    pending_directories: &mut Vec<DirectoryTask>,
    nodes: &mut Vec<InternalNode>,
    counters: &mut ScanCounters,
    partial_ranking: &mut PartialRanking,
    entries_since_progress: &mut u64,
    last_progress_at: &mut Instant,
    started_at: Instant,
    cancel: &AtomicBool,
    on_progress: &mut F,
) -> Result<(), NativeScanError>
where
    F: FnMut(ScanProgress),
{
    if pending_directories.is_empty() {
        return Ok(());
    }

    let max_workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(MAX_WORKERS);
    let initial_workers = max_workers.min(DEFAULT_PARALLELISM);
    let queue_capacity = max_workers.saturating_mul(2).max(1);
    let (task_sender, task_receiver) = channel::bounded::<DirectoryTask>(queue_capacity);
    let (result_sender, result_receiver) = channel::bounded::<WorkerMessage>(queue_capacity);
    let abort = AtomicBool::new(false);

    std::thread::scope(|scope| {
        for _ in 0..initial_workers {
            spawn_worker(
                scope,
                task_receiver.clone(),
                result_sender.clone(),
                &abort,
                cancel,
            );
        }

        let mut outstanding = 0_usize;
        // Four concurrent small-directory calls avoided APFS contention in
        // metadata-heavy trees. Large file batches amortize the syscall and
        // channel costs, so they can profitably use the full worker pool.
        let mut parallelism = initial_workers;
        let mut spawned_workers = initial_workers;
        let mut result = Ok(());
        loop {
            if cancel.load(Ordering::Relaxed) {
                result = Err(fatal("Scan cancelled."));
                break;
            }

            while outstanding < parallelism {
                let Some(task) = pending_directories.pop() else {
                    break;
                };
                if task_sender.send(task).is_err() {
                    result = Err(fatal(
                        "The native scanner worker pool stopped unexpectedly.",
                    ));
                    break;
                }
                outstanding += 1;
            }
            if result.is_err() || outstanding == 0 {
                break;
            }

            match result_receiver.recv_timeout(WORKER_POLL_INTERVAL) {
                Ok(WorkerMessage::Batch {
                    path,
                    parent_id,
                    entries,
                }) => {
                    if contains_large_file_batch(&entries) {
                        for _ in spawned_workers..max_workers {
                            spawn_worker(
                                scope,
                                task_receiver.clone(),
                                result_sender.clone(),
                                &abort,
                                cancel,
                            );
                        }
                        spawned_workers = max_workers;
                        parallelism = max_workers;
                    }
                    if let Err(error) = ingest_entries(
                        entries,
                        &path,
                        parent_id,
                        nodes,
                        counters,
                        partial_ranking,
                        pending_directories,
                        entries_since_progress,
                        last_progress_at,
                        started_at,
                        cancel,
                        on_progress,
                    ) {
                        result = Err(error);
                        break;
                    }
                }
                Ok(WorkerMessage::Complete) => outstanding -= 1,
                Ok(WorkerMessage::Failed(read_error)) => {
                    outstanding -= 1;
                    match read_error {
                        DirectoryReadError::Io(_) => counters.skipped_entries += 1,
                        DirectoryReadError::Parse(error) => {
                            result = Err(fatal(error));
                            break;
                        }
                        DirectoryReadError::Cancelled => {
                            result = Err(fatal("Scan cancelled."));
                            break;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    result = Err(fatal(
                        "The native scanner worker pool stopped unexpectedly.",
                    ));
                    break;
                }
            }
        }

        abort.store(true, Ordering::Relaxed);
        drop(task_sender);
        drop(result_receiver);
        result
    })
}

fn spawn_worker<'scope, 'env: 'scope>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    task_receiver: channel::Receiver<DirectoryTask>,
    result_sender: channel::Sender<WorkerMessage>,
    abort: &'scope AtomicBool,
    cancel: &'scope AtomicBool,
) {
    scope.spawn(move || {
        let mut buffer = vec![0_u64; BUFFER_SIZE / size_of::<u64>()];
        let mut attribute_list = requested_attributes();
        loop {
            if abort.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed) {
                break;
            }
            let task = match task_receiver.recv_timeout(WORKER_POLL_INTERVAL) {
                Ok(task) => task,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            process_directory(
                task,
                cancel,
                &mut attribute_list,
                &mut buffer,
                &result_sender,
            );
        }
    });
}

fn process_directory(
    task: DirectoryTask,
    cancel: &AtomicBool,
    attribute_list: &mut attrlist,
    buffer: &mut [u64],
    result_sender: &channel::Sender<WorkerMessage>,
) {
    let directory = match open_directory(&task.path) {
        Ok(directory) => directory,
        Err(error) => {
            let _ = result_sender.send(WorkerMessage::Failed(DirectoryReadError::Io(error)));
            return;
        }
    };
    loop {
        match read_directory_batch(&directory, cancel, attribute_list, buffer) {
            Ok(Some(entries)) => {
                if result_sender
                    .send(WorkerMessage::Batch {
                        path: task.path.clone(),
                        parent_id: task.parent_id,
                        entries,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Ok(None) => {
                let _ = result_sender.send(WorkerMessage::Complete);
                return;
            }
            Err(error) => {
                let _ = result_sender.send(WorkerMessage::Failed(error));
                return;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn ingest_entries<F>(
    entries: Vec<NativeEntry>,
    directory_path: &Path,
    parent_id: usize,
    nodes: &mut Vec<InternalNode>,
    counters: &mut ScanCounters,
    partial_ranking: &mut PartialRanking,
    pending_directories: &mut Vec<DirectoryTask>,
    entries_since_progress: &mut u64,
    last_progress_at: &mut Instant,
    started_at: Instant,
    cancel: &AtomicBool,
    on_progress: &mut F,
) -> Result<(), NativeScanError>
where
    F: FnMut(ScanProgress),
{
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err(fatal("Scan cancelled."));
        }
        if entry.entry_error != 0 {
            counters.skipped_entries += 1;
            continue;
        }
        if entry.measured.metadata_error {
            counters.skipped_entries += 1;
        }
        if entry.name.as_bytes() == b"." || entry.name.as_bytes() == b".." {
            continue;
        }

        let child_path =
            matches!(entry.kind, EntryKind::Directory).then(|| directory_path.join(&entry.name));
        let (node_id, replaced_owner) =
            counters.push_node(nodes, parent_id, entry.name, entry.kind, entry.measured);
        observe_partial_file(partial_ranking, nodes, node_id, replaced_owner);

        if matches!(entry.kind, EntryKind::Directory) {
            if entry.mount_point {
                counters.skipped_filesystems += 1;
            } else if entry.firmlink || !entry.can_enforce_mount_boundary {
                counters.skipped_entries += 1;
            } else if let Some(child_path) = child_path.clone() {
                pending_directories.push(DirectoryTask {
                    path: Arc::from(child_path),
                    parent_id: node_id,
                });
            }
        }

        *entries_since_progress += 1;
        if *entries_since_progress >= PROGRESS_ENTRY_INTERVAL
            || last_progress_at.elapsed() >= PROGRESS_INTERVAL
        {
            let current_path =
                child_path.unwrap_or_else(|| directory_path.join(&nodes[node_id].name));
            on_progress(ScanProgress {
                entries_scanned: counters.files_scanned + counters.directories_scanned,
                files_scanned: counters.files_scanned,
                directories_scanned: counters.directories_scanned,
                logical_bytes: counters.observed_logical_bytes,
                allocated_bytes: counters.observed_allocated_bytes,
                skipped_entries: counters.skipped_entries,
                current_path: current_path.to_string_lossy().into_owned(),
                elapsed_ms: started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                largest_items: partial_ranking.items(nodes),
            });
            *entries_since_progress = 0;
            *last_progress_at = Instant::now();
        }
    }
    Ok(())
}

fn contains_large_file_batch(entries: &[NativeEntry]) -> bool {
    entries
        .iter()
        .filter(|entry| matches!(entry.kind, EntryKind::File))
        .nth(LARGE_DIRECTORY_FILE_COUNT - 1)
        .is_some()
}

fn open_directory(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

fn read_directory_batch(
    directory: &File,
    cancel: &AtomicBool,
    attribute_list: &mut attrlist,
    buffer: &mut [u64],
) -> Result<Option<Vec<NativeEntry>>, DirectoryReadError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(DirectoryReadError::Cancelled);
    }

    let entry_count = unsafe {
        // SAFETY: `directory` owns a readable directory descriptor; both
        // pointers reference writable, correctly sized values for the
        // duration of the system call.
        libc::getattrlistbulk(
            directory.as_raw_fd(),
            (attribute_list as *mut attrlist).cast(),
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(buffer),
            u64::from(libc::FSOPT_PACK_INVAL_ATTRS),
        )
    };
    if entry_count < 0 {
        return Err(DirectoryReadError::Io(io::Error::last_os_error()));
    }
    if entry_count == 0 {
        return Ok(None);
    }

    let bytes = unsafe {
        // SAFETY: the `u64` buffer is contiguous and initialized by the kernel
        // for the returned record count. Parsing validates every record length
        // before reading fields.
        std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), std::mem::size_of_val(buffer))
    };
    parse_entries(bytes, entry_count as usize)
        .map(Some)
        .map_err(DirectoryReadError::Parse)
}

fn requested_attributes() -> attrlist {
    attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_RETURNED_ATTRS
            | libc::ATTR_CMN_NAME
            | libc::ATTR_CMN_DEVID
            | libc::ATTR_CMN_OBJTYPE
            | libc::ATTR_CMN_FLAGS
            | libc::ATTR_CMN_FILEID
            | ATTR_CMN_ERROR,
        volattr: 0,
        dirattr: libc::ATTR_DIR_MOUNTSTATUS,
        fileattr: libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_TOTALSIZE | libc::ATTR_FILE_ALLOCSIZE,
        forkattr: 0,
    }
}

fn parse_entries(buffer: &[u8], entry_count: usize) -> Result<Vec<NativeEntry>, String> {
    let mut entries = Vec::with_capacity(entry_count);
    let mut record_start = 0_usize;

    for _ in 0..entry_count {
        let record_length = read_at::<u32>(buffer, record_start)? as usize;
        if record_length < size_of::<u32>() || record_start + record_length > buffer.len() {
            return Err("getattrlistbulk returned an invalid record length".to_string());
        }
        let record = &buffer[record_start..record_start + record_length];
        entries.push(parse_entry(record)?);
        record_start += record_length;
    }
    Ok(entries)
}

fn parse_entry(record: &[u8]) -> Result<NativeEntry, String> {
    let mut cursor = Cursor::new(record, size_of::<u32>());
    let returned = cursor.read::<attribute_set_t>()?;
    let entry_error = cursor.read::<u32>()?;

    let name_reference_offset = cursor.offset;
    let name_reference = cursor.read::<attrreference_t>()?;
    let name = referenced_name(record, name_reference_offset, name_reference)?;
    let device = cursor.read::<libc::dev_t>()?;
    let object_type = cursor.read::<u32>()?;
    let flags = cursor.read::<u32>()?;
    let file_id = cursor.read::<u64>()?;
    let kind = match object_type {
        VREG => EntryKind::File,
        VDIR => EntryKind::Directory,
        VLNK => EntryKind::Symlink,
        _ => EntryKind::Other,
    };
    // Darwin omits directory attributes for non-directories and file
    // attributes for non-files, even with FSOPT_PACK_INVAL_ATTRS. The common
    // object type therefore determines which requested attribute group is
    // physically present in this record.
    let mount_status = if matches!(kind, EntryKind::Directory) {
        cursor.read::<u32>()?
    } else {
        0
    };
    let (link_count, logical_bytes, allocated_bytes) = if matches!(kind, EntryKind::File) {
        (
            cursor.read::<u32>()?,
            cursor.read::<i64>()?,
            cursor.read::<i64>()?,
        )
    } else {
        (0, 0, 0)
    };
    let file_attributes_valid = returned.fileattr
        & (libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_TOTALSIZE | libc::ATTR_FILE_ALLOCSIZE)
        == (libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_TOTALSIZE | libc::ATTR_FILE_ALLOCSIZE);
    let identity_attributes_valid = returned.commonattr
        & (libc::ATTR_CMN_DEVID | libc::ATTR_CMN_FILEID)
        == (libc::ATTR_CMN_DEVID | libc::ATTR_CMN_FILEID);

    Ok(NativeEntry {
        name,
        kind,
        measured: MeasuredMetadata {
            logical_bytes: if matches!(kind, EntryKind::File) && file_attributes_valid {
                logical_bytes.max(0) as u64
            } else {
                0
            },
            allocated_bytes: if matches!(kind, EntryKind::File) && file_attributes_valid {
                allocated_bytes.max(0) as u64
            } else {
                0
            },
            filesystem_id: identity_attributes_valid.then_some(device as u64),
            file_identity: (matches!(kind, EntryKind::File)
                && file_attributes_valid
                && identity_attributes_valid
                && link_count > 1)
                .then_some(FileIdentity(device as u64, file_id)),
            metadata_error: matches!(kind, EntryKind::File) && !file_attributes_valid,
        },
        entry_error,
        mount_point: matches!(kind, EntryKind::Directory)
            && returned.dirattr & libc::ATTR_DIR_MOUNTSTATUS != 0
            && mount_status & libc::DIR_MNTSTATUS_MNTPOINT != 0,
        firmlink: returned.commonattr & libc::ATTR_CMN_FLAGS != 0 && flags & SF_FIRMLINK != 0,
        can_enforce_mount_boundary: !matches!(kind, EntryKind::Directory)
            || returned.dirattr & libc::ATTR_DIR_MOUNTSTATUS != 0,
    })
}

fn referenced_name(
    record: &[u8],
    reference_offset: usize,
    reference: attrreference_t,
) -> Result<OsString, String> {
    let start = reference_offset as isize + reference.attr_dataoffset as isize;
    if start < 0 {
        return Err("getattrlistbulk returned an invalid name offset".to_string());
    }
    let start = start as usize;
    let end = start
        .checked_add(reference.attr_length as usize)
        .filter(|end| *end <= record.len())
        .ok_or_else(|| "getattrlistbulk returned an invalid name length".to_string())?;
    let bytes = &record[start..end];
    let bytes = bytes.split(|byte| *byte == 0).next().unwrap_or_default();
    if bytes.is_empty() {
        return Err("getattrlistbulk returned an empty entry name".to_string());
    }
    Ok(OsString::from_vec(bytes.to_vec()))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], offset: usize) -> Self {
        Self { bytes, offset }
    }

    fn read<T: Copy>(&mut self) -> Result<T, String> {
        let value = read_at::<T>(self.bytes, self.offset)?;
        self.offset += size_of::<T>();
        Ok(value)
    }
}

fn read_at<T: Copy>(bytes: &[u8], offset: usize) -> Result<T, String> {
    let end = offset
        .checked_add(size_of::<T>())
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| "getattrlistbulk returned a truncated attribute record".to_string())?;
    let _ = end;
    let value = unsafe {
        // SAFETY: bounds are checked above and Darwin attribute fields are
        // packed to 4-byte alignment, so unaligned reads are required.
        std::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<T>())
    };
    Ok(value)
}

fn is_unavailable(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::EINVAL) | Some(libc::ENOTSUP) | Some(libc::ENOTTY)
    )
}

fn fatal(error: impl Into<String>) -> NativeScanError {
    NativeScanError::Fatal(error.into())
}

#[cfg(test)]
mod tests {
    use super::parse_entries;

    #[test]
    fn rejects_truncated_attribute_records() {
        let error = parse_entries(&4_u32.to_ne_bytes(), 1)
            .expect_err("a length-only record must be rejected");

        assert_eq!(
            error,
            "getattrlistbulk returned a truncated attribute record"
        );
    }
}
