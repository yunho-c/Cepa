use super::{
    EntryKind, FileIdentity, InternalNode, MeasuredMetadata, PROGRESS_ENTRY_INTERVAL,
    PROGRESS_INTERVAL, PartialRanking, ScanCounters, ScanOutput, ScanProgress, ScanSemantics,
    finish_scan, observe_partial_file,
};
use crossbeam_channel::{self as channel, RecvTimeoutError};
use rustix::fd::OwnedFd;
use rustix::fs::{AtFlags, FileType, Mode, OFlags, RawDir, Statx, StatxFlags, open, openat, statx};
use rustix::io::Errno;
use std::ffi::{CStr, OsString};
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DIRECTORY_BUFFER_SIZE: usize = 64 * 1024;
const RESULT_BATCH_SIZE: usize = 512;
const MAX_WORKERS: usize = 8;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(25);
const STATX_REQUEST: StatxFlags = StatxFlags::BASIC_STATS.union(StatxFlags::MNT_ID);
const STATX_AT_FLAGS: AtFlags = AtFlags::NO_AUTOMOUNT.union(AtFlags::SYMLINK_NOFOLLOW);

pub(super) enum NativeScanError {
    Unavailable,
    Fatal(String),
}

#[derive(Debug)]
struct DirectoryTask {
    source: DirectorySource,
    path: Arc<Path>,
    parent_id: usize,
    root_mount_id: u64,
}

#[derive(Debug)]
enum DirectorySource {
    Root(OwnedFd),
    Child {
        parent: Arc<OwnedFd>,
        name: OsString,
        expected: DirectoryIdentity,
    },
}

#[derive(Clone, Copy, Debug)]
struct DirectoryIdentity {
    device_major: u32,
    device_minor: u32,
    inode: u64,
    mount_id: u64,
}

#[derive(Debug)]
struct NativeEntry {
    name: OsString,
    kind: EntryKind,
    measured: MeasuredMetadata,
    mount_boundary: bool,
    directory_identity: Option<DirectoryIdentity>,
}

#[derive(Debug)]
enum WorkerMessage {
    Batch {
        directory_fd: Arc<OwnedFd>,
        path: Arc<Path>,
        parent_id: usize,
        root_mount_id: u64,
        entries: Vec<NativeEntry>,
    },
    Complete {
        parent_id: usize,
    },
    Failed {
        parent_id: usize,
        path: Arc<Path>,
        error: DirectoryReadError,
    },
}

#[derive(Debug)]
enum DirectoryReadError {
    Io(io::Error),
    Cancelled,
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
    if cancel.load(Ordering::Relaxed) {
        return Err(fatal("Scan cancelled."));
    }

    let root_fd = match open_directory(&root) {
        Ok(fd) => fd,
        Err(Errno::NOTDIR) => return Err(fatal("Choose a directory to scan.")),
        Err(error) => {
            return Err(fatal(format!("Could not open {}: {error}", root.display())));
        }
    };
    let root_stat = match statx_fd(&root_fd) {
        Ok(stat) => stat,
        Err(Errno::NOSYS) => return Err(NativeScanError::Unavailable),
        Err(error) => {
            return Err(fatal(format!(
                "Could not read {} with statx: {error}",
                root.display()
            )));
        }
    };
    if !has_required_statx_fields(&root_stat) {
        return Err(NativeScanError::Unavailable);
    }
    if !FileType::from_raw_mode(u32::from(root_stat.stx_mode)).is_dir() {
        return Err(fatal("Choose a directory to scan."));
    }

    let started_at = Instant::now();
    let root_node_path: Arc<Path> = Arc::from(root.clone());
    let mut nodes = vec![InternalNode::root(&root)];
    let mut counters = ScanCounters::default();
    let mut partial_ranking = PartialRanking::default();
    let mut pending_directories = vec![DirectoryTask {
        source: DirectorySource::Root(root_fd),
        path: root_node_path.clone(),
        parent_id: 0,
        root_mount_id: root_stat.stx_mnt_id,
    }];
    let mut entries_since_progress = 0_u64;
    let mut last_progress_at = Instant::now();

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
        "statx",
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
    let worker_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, MAX_WORKERS);
    let queue_capacity = worker_count.saturating_mul(2).max(1);
    let (task_sender, task_receiver) = channel::bounded::<DirectoryTask>(queue_capacity);
    let (result_sender, result_receiver) = channel::bounded::<WorkerMessage>(queue_capacity);
    let abort = AtomicBool::new(false);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            spawn_worker(
                scope,
                task_receiver.clone(),
                result_sender.clone(),
                &abort,
                cancel,
            );
        }
        drop(result_sender);
        drop(task_receiver);

        let mut outstanding = 0_usize;
        let mut result = Ok(());
        loop {
            if cancel.load(Ordering::Relaxed) {
                result = Err(fatal("Scan cancelled."));
                break;
            }

            while outstanding < worker_count {
                let Some(task) = pending_directories.pop() else {
                    break;
                };
                if task_sender.send(task).is_err() {
                    result = Err(fatal("The Linux scanner worker pool stopped unexpectedly."));
                    break;
                }
                outstanding += 1;
            }
            if result.is_err() || outstanding == 0 {
                break;
            }

            match result_receiver.recv_timeout(WORKER_POLL_INTERVAL) {
                Ok(WorkerMessage::Batch {
                    directory_fd,
                    path,
                    parent_id,
                    root_mount_id,
                    entries,
                }) => {
                    if let Err(error) = ingest_entries(
                        entries,
                        directory_fd,
                        &path,
                        parent_id,
                        root_mount_id,
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
                Ok(WorkerMessage::Complete { parent_id }) => {
                    debug_assert!(parent_id < nodes.len());
                    outstanding -= 1;
                }
                Ok(WorkerMessage::Failed {
                    parent_id,
                    path,
                    error,
                }) => {
                    outstanding -= 1;
                    match error {
                        DirectoryReadError::Cancelled => {
                            result = Err(fatal("Scan cancelled."));
                            break;
                        }
                        DirectoryReadError::Io(error) if parent_id == 0 => {
                            result =
                                Err(fatal(format!("Could not scan {}: {error}", path.display())));
                            break;
                        }
                        DirectoryReadError::Io(_) => counters.skipped_entries += 1,
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    result = Err(fatal("The Linux scanner worker pool stopped unexpectedly."));
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
        loop {
            if abort.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed) {
                break;
            }
            let task = match task_receiver.recv_timeout(WORKER_POLL_INTERVAL) {
                Ok(task) => task,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            process_directory(task, cancel, &result_sender);
        }
    });
}

fn process_directory(
    task: DirectoryTask,
    cancel: &AtomicBool,
    result_sender: &channel::Sender<WorkerMessage>,
) {
    let DirectoryTask {
        source,
        path,
        parent_id,
        root_mount_id,
    } = task;
    let directory_fd = match open_task_directory(source) {
        Ok(fd) => Arc::new(fd),
        Err(error) => {
            send_failure(
                parent_id,
                path,
                DirectoryReadError::Io(error),
                result_sender,
            );
            return;
        }
    };
    let mut buffer = vec![MaybeUninit::<u8>::uninit(); DIRECTORY_BUFFER_SIZE];
    let mut directory = RawDir::new(directory_fd.as_ref(), &mut buffer);
    let mut entries = Vec::with_capacity(RESULT_BATCH_SIZE);

    while let Some(entry) = directory.next() {
        if cancel.load(Ordering::Relaxed) {
            send_failure(
                parent_id,
                path.clone(),
                DirectoryReadError::Cancelled,
                result_sender,
            );
            return;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                send_failure(
                    parent_id,
                    path.clone(),
                    DirectoryReadError::Io(error.into()),
                    result_sender,
                );
                return;
            }
        };
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }

        entries.push(read_entry(
            directory_fd.as_ref(),
            root_mount_id,
            name,
            entry.file_type(),
        ));
        if entries.len() == RESULT_BATCH_SIZE
            && !send_batch(
                directory_fd.clone(),
                &path,
                parent_id,
                root_mount_id,
                std::mem::take(&mut entries),
                result_sender,
            )
        {
            return;
        }
    }

    if !entries.is_empty()
        && !send_batch(
            directory_fd.clone(),
            &path,
            parent_id,
            root_mount_id,
            entries,
            result_sender,
        )
    {
        return;
    }
    let _ = result_sender.send(WorkerMessage::Complete { parent_id });
}

fn read_entry(
    directory_fd: &OwnedFd,
    root_mount_id: u64,
    name: &CStr,
    directory_type: FileType,
) -> NativeEntry {
    let name_owned = OsString::from_vec(name.to_bytes().to_vec());
    let stat = match statx(directory_fd, name, STATX_AT_FLAGS, STATX_REQUEST) {
        Ok(stat) if has_required_statx_fields(&stat) => stat,
        Ok(_) | Err(_) => {
            return NativeEntry {
                name: name_owned,
                kind: if directory_type.is_dir() {
                    EntryKind::Directory
                } else {
                    EntryKind::Other
                },
                measured: MeasuredMetadata {
                    metadata_error: true,
                    ..MeasuredMetadata::default()
                },
                mount_boundary: false,
                directory_identity: None,
            };
        }
    };

    let file_type = FileType::from_raw_mode(u32::from(stat.stx_mode));
    let kind = entry_kind(file_type);
    let device = device_id(&stat);
    let mount_boundary = matches!(kind, EntryKind::Directory) && stat.stx_mnt_id != root_mount_id;

    NativeEntry {
        name: name_owned,
        kind,
        measured: MeasuredMetadata {
            logical_bytes: matches!(kind, EntryKind::File)
                .then_some(stat.stx_size)
                .unwrap_or_default(),
            allocated_bytes: matches!(kind, EntryKind::File)
                .then_some(stat.stx_blocks.saturating_mul(512))
                .unwrap_or_default(),
            filesystem_id: Some(device),
            file_identity: (matches!(kind, EntryKind::File) && stat.stx_nlink > 1)
                .then_some(FileIdentity(device, stat.stx_ino)),
            metadata_error: false,
        },
        mount_boundary,
        directory_identity: (matches!(kind, EntryKind::Directory) && !mount_boundary)
            .then(|| DirectoryIdentity::from_stat(&stat)),
    }
}

#[allow(clippy::too_many_arguments)]
fn ingest_entries<F>(
    entries: Vec<NativeEntry>,
    directory_fd: Arc<OwnedFd>,
    directory_path: &Path,
    parent_id: usize,
    root_mount_id: u64,
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
        if entry.measured.metadata_error && !matches!(entry.kind, EntryKind::Directory) {
            counters.skipped_entries += 1;
            continue;
        }
        if entry.measured.metadata_error {
            counters.skipped_entries += 1;
        }

        let child_path = directory_path.join(&entry.name);
        let task_name = entry.name.clone();
        let (node_id, replaced_owner) =
            counters.push_node(nodes, parent_id, entry.name, entry.kind, entry.measured);
        observe_partial_file(partial_ranking, nodes, node_id, replaced_owner);

        if matches!(entry.kind, EntryKind::Directory) {
            if entry.mount_boundary {
                counters.skipped_filesystems += 1;
            } else if let Some(expected) = entry.directory_identity {
                pending_directories.push(DirectoryTask {
                    source: DirectorySource::Child {
                        parent: directory_fd.clone(),
                        name: task_name,
                        expected,
                    },
                    path: Arc::from(child_path.clone()),
                    parent_id: node_id,
                    root_mount_id,
                });
            }
        }

        *entries_since_progress += 1;
        if *entries_since_progress >= PROGRESS_ENTRY_INTERVAL
            || last_progress_at.elapsed() >= PROGRESS_INTERVAL
        {
            on_progress(ScanProgress {
                entries_scanned: counters.files_scanned + counters.directories_scanned,
                files_scanned: counters.files_scanned,
                directories_scanned: counters.directories_scanned,
                logical_bytes: counters.observed_logical_bytes,
                allocated_bytes: counters.observed_allocated_bytes,
                skipped_entries: counters.skipped_entries,
                current_path: child_path.to_string_lossy().into_owned(),
                elapsed_ms: started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                largest_items: partial_ranking.items(nodes),
            });
            *entries_since_progress = 0;
            *last_progress_at = Instant::now();
        }
    }
    Ok(())
}

fn send_batch(
    directory_fd: Arc<OwnedFd>,
    path: &Arc<Path>,
    parent_id: usize,
    root_mount_id: u64,
    entries: Vec<NativeEntry>,
    result_sender: &channel::Sender<WorkerMessage>,
) -> bool {
    result_sender
        .send(WorkerMessage::Batch {
            directory_fd,
            path: path.clone(),
            parent_id,
            root_mount_id,
            entries,
        })
        .is_ok()
}

fn send_failure(
    parent_id: usize,
    path: Arc<Path>,
    error: DirectoryReadError,
    result_sender: &channel::Sender<WorkerMessage>,
) {
    let _ = result_sender.send(WorkerMessage::Failed {
        parent_id,
        path,
        error,
    });
}

fn open_task_directory(source: DirectorySource) -> io::Result<OwnedFd> {
    match source {
        DirectorySource::Root(fd) => Ok(fd),
        DirectorySource::Child {
            parent,
            name,
            expected,
        } => {
            let fd = openat(
                parent.as_ref(),
                &name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(io::Error::from)?;
            let opened = statx_fd(&fd).map_err(io::Error::from)?;
            if expected.matches(&opened) {
                Ok(fd)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "the directory identity changed before traversal",
                ))
            }
        }
    }
}

fn open_directory(path: &Path) -> Result<OwnedFd, Errno> {
    open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
}

fn statx_fd(fd: &OwnedFd) -> Result<Statx, Errno> {
    statx(fd, c".", STATX_AT_FLAGS, STATX_REQUEST)
}

fn has_required_statx_fields(stat: &Statx) -> bool {
    StatxFlags::from_bits_retain(stat.stx_mask).contains(STATX_REQUEST)
}

impl DirectoryIdentity {
    fn from_stat(stat: &Statx) -> Self {
        Self {
            device_major: stat.stx_dev_major,
            device_minor: stat.stx_dev_minor,
            inode: stat.stx_ino,
            mount_id: stat.stx_mnt_id,
        }
    }

    fn matches(self, opened: &Statx) -> bool {
        has_required_statx_fields(opened)
            && FileType::from_raw_mode(u32::from(opened.stx_mode)).is_dir()
            && self.device_major == opened.stx_dev_major
            && self.device_minor == opened.stx_dev_minor
            && self.inode == opened.stx_ino
            && self.mount_id == opened.stx_mnt_id
    }
}

fn device_id(stat: &Statx) -> u64 {
    (u64::from(stat.stx_dev_major) << 32) | u64::from(stat.stx_dev_minor)
}

fn entry_kind(file_type: FileType) -> EntryKind {
    if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn fatal(error: impl Into<String>) -> NativeScanError {
    NativeScanError::Fatal(error.into())
}

#[cfg(test)]
mod tests {
    use super::{device_id, entry_kind};
    use crate::scanner::EntryKind;
    use rustix::fs::FileType;

    #[test]
    fn maps_linux_file_types_without_following_links() {
        assert!(matches!(entry_kind(FileType::RegularFile), EntryKind::File));
        assert!(matches!(
            entry_kind(FileType::Directory),
            EntryKind::Directory
        ));
        assert!(matches!(entry_kind(FileType::Symlink), EntryKind::Symlink));
        assert!(matches!(entry_kind(FileType::Socket), EntryKind::Other));
    }

    #[test]
    fn device_identity_keeps_major_and_minor_components() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let fd = super::open_directory(temp.path()).expect("open fixture directory");
        let stat = super::statx_fd(&fd).expect("stat fixture directory");
        assert_eq!(
            device_id(&stat),
            (u64::from(stat.stx_dev_major) << 32) | u64::from(stat.stx_dev_minor)
        );
    }
}
