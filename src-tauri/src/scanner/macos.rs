use super::{
    EntryKind, FileIdentity, InternalNode, MeasuredMetadata, PROGRESS_ENTRY_INTERVAL,
    PROGRESS_INTERVAL, ScanCounters, ScanOutput, ScanProgress, ScanSemantics, finish_scan,
};
use libc::{self, attribute_set_t, attrlist, attrreference_t};
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

const BUFFER_SIZE: usize = 256 * 1024;
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
    let mut pending_directories = vec![(root.clone(), 0_usize)];
    let mut entries_since_progress = 0_u64;
    let mut last_progress_at = Instant::now();
    let mut buffer = vec![0_u64; BUFFER_SIZE / size_of::<u64>()];
    let mut attribute_list = requested_attributes();
    let mut first_root_call = true;

    while let Some((directory_path, parent_id)) = pending_directories.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(fatal("Scan cancelled."));
        }

        let directory = match File::open(&directory_path) {
            Ok(directory) => directory,
            Err(error) if parent_id != 0 => {
                counters.skipped_entries += 1;
                let _ = error;
                continue;
            }
            Err(error) => {
                return Err(fatal(format!(
                    "Could not open {}: {error}",
                    directory_path.display()
                )));
            }
        };

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(fatal("Scan cancelled."));
            }

            let entry_count = unsafe {
                // SAFETY: `directory` owns a readable directory descriptor;
                // both pointers reference writable, correctly sized values for
                // the duration of the system call.
                libc::getattrlistbulk(
                    directory.as_raw_fd(),
                    (&mut attribute_list as *mut attrlist).cast(),
                    buffer.as_mut_ptr().cast(),
                    BUFFER_SIZE,
                    u64::from(libc::FSOPT_PACK_INVAL_ATTRS),
                )
            };

            if entry_count < 0 {
                let error = io::Error::last_os_error();
                if parent_id == 0 && first_root_call && is_unavailable(&error) {
                    return Err(NativeScanError::Unavailable);
                }
                counters.skipped_entries += 1;
                break;
            }
            first_root_call = false;
            if entry_count == 0 {
                break;
            }

            let bytes = unsafe {
                // SAFETY: the `u64` buffer is contiguous and initialized by
                // the kernel for the returned record count. Parsing validates
                // every record length before reading fields.
                std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), BUFFER_SIZE)
            };
            let entries = parse_entries(bytes, entry_count as usize).map_err(fatal)?;

            for entry in entries {
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

                let child_path = matches!(entry.kind, EntryKind::Directory)
                    .then(|| directory_path.join(&entry.name));
                let node_id = counters.push_node(
                    &mut nodes,
                    parent_id,
                    entry.name,
                    entry.kind,
                    entry.measured,
                );

                if matches!(entry.kind, EntryKind::Directory) {
                    if entry.mount_point {
                        counters.skipped_filesystems += 1;
                    } else if entry.firmlink || !entry.can_enforce_mount_boundary {
                        counters.skipped_entries += 1;
                    } else if let Some(child_path) = child_path.clone() {
                        pending_directories.push((child_path, node_id));
                    }
                }

                entries_since_progress += 1;
                if entries_since_progress >= PROGRESS_ENTRY_INTERVAL
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
                        elapsed_ms: started_at.elapsed().as_millis().min(u128::from(u64::MAX))
                            as u64,
                    });
                    entries_since_progress = 0;
                    last_progress_at = Instant::now();
                }
            }
        }
    }

    let traversal_completed_at = Instant::now();
    finish_scan(
        root,
        root_node_path,
        nodes,
        counters,
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
