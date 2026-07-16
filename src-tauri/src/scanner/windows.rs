use super::mft::{self, Record};
use super::{
    CANCELLATION_CHECK_INTERVAL_ENTRIES, EntryKind, FileIdentity, HardLinkOwner, InternalNode,
    MeasuredMetadata, PROGRESS_INTERVAL, PartialRanking, ScanCounters, ScanOutput, ScanPhase,
    ScanProgress, ScanSemantics, finish_scan, observe_partial_file,
};
use crate::file_revision::ScannedFileRevision;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_HANDLE_EOF, ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER,
    ERROR_JOURNAL_NOT_ACTIVE, ERROR_MORE_DATA, ERROR_NOT_SUPPORTED, GENERIC_READ,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileBasicInfo,
    FileIdType, FileStandardInfo, FindClose, FindFirstFileNameW, FindNextFileNameW,
    GetFileInformationByHandle, GetFileInformationByHandleEx, GetVolumeInformationW,
    GetVolumeNameForVolumeMountPointW, GetVolumePathNameW, OPEN_EXISTING, OpenFileById,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{FSCTL_ENUM_USN_DATA, MFT_ENUM_DATA_V0};

const ENUM_BUFFER_SIZE: usize = 1024 * 1024;
const MAX_WORKERS: usize = 8;
const MEASUREMENT_BATCH_SIZE: usize = 256;
const RESULT_BATCHES_PER_WORKER: usize = 2;
const WINDOWS_PATH_BUFFER: usize = 32_768;

pub(super) enum NativeScanError {
    Unavailable,
    Fatal(String),
}

#[derive(Clone, Copy, Debug, Default)]
struct WindowsMeasurement {
    measured: MeasuredMetadata,
    link_count: u32,
}

#[derive(Clone, Copy, Debug)]
struct HardLinkCandidate {
    node_id: usize,
    measured: MeasuredMetadata,
    link_count: u32,
}

struct FindHandle(*mut c_void);

impl Drop for FindHandle {
    fn drop(&mut self) {
        unsafe {
            FindClose(self.0);
        }
    }
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

    let root_handle = open_path(&root, FILE_READ_ATTRIBUTES)?;
    let root_information = file_information(&root_handle)
        .map_err(|error| fatal(format!("Could not read {}: {error}", root.display())))?;
    if root_information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(fatal("Choose a directory to scan."));
    }
    let root_reference = u64::from(root_information.nFileIndexHigh) << 32
        | u64::from(root_information.nFileIndexLow);
    let volume_serial = u64::from(root_information.dwVolumeSerialNumber);

    let volume_root = volume_root(&root)?;
    if !is_ntfs(&volume_root)? {
        return Err(NativeScanError::Unavailable);
    }
    if volume_root_reference(&volume_root)? != root_reference {
        return Err(NativeScanError::Unavailable);
    }
    let volume_handle = open_volume(&volume_root)?;

    let started_at = Instant::now();
    // MFT enumeration describes the whole volume and has a fixed cost
    // independent of the selected subtree. Auto selection therefore uses it
    // only for a volume root; subfolders fall back before any progress is sent.
    let records = enumerate_mft(
        &volume_handle,
        &cancel,
        started_at,
        &root,
        on_progress,
        |reference, _, _| reference != root_reference,
    )?;
    let ordered =
        mft::subtree_order(&records, root_reference).map_err(|_| NativeScanError::Unavailable)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(fatal("Scan cancelled."));
    }

    let root_node_path: Arc<Path> = Arc::from(root.clone());
    let mut nodes = preallocate_node_arena(&root, ordered.len())?;
    let mut counters = ScanCounters::default();
    let mut partial_ranking = PartialRanking::default();
    let mut node_by_reference = std::collections::HashMap::with_capacity(ordered.len() + 1);
    let mut file_nodes = Vec::new();
    let mut hard_links = Vec::new();
    node_by_reference.insert(root_reference, 0_usize);

    for &record_index in &ordered {
        if nodes
            .len()
            .is_multiple_of(CANCELLATION_CHECK_INTERVAL_ENTRIES as usize)
            && cancel.load(Ordering::Relaxed)
        {
            return Err(fatal("Scan cancelled."));
        }
        let record = &records[record_index];
        let parent = node_by_reference
            .get(&record.parent_reference)
            .copied()
            .ok_or(NativeScanError::Unavailable)?;
        let kind = entry_kind(record.attributes);
        let name = OsString::from_wide(&record.name);
        let (node_id, _) =
            counters.push_node(&mut nodes, parent, name, kind, MeasuredMetadata::default());
        node_by_reference.insert(record.reference, node_id);
        if matches!(kind, EntryKind::File) {
            file_nodes.push((record_index, node_id));
        }
    }
    drop(node_by_reference);

    measure_files(
        &volume_handle,
        &records,
        &file_nodes,
        volume_serial,
        &mut nodes,
        &mut counters,
        &mut partial_ranking,
        &mut hard_links,
        &cancel,
        started_at,
        &root,
        on_progress,
    )?;
    hard_links.sort_unstable_by_key(|candidate| candidate.node_id);

    ingest_hard_links(
        &root,
        &mut nodes,
        &mut counters,
        &mut partial_ranking,
        &hard_links,
        &cancel,
    )?;

    let traversal_completed_at = Instant::now();
    finish_scan(
        root,
        root_node_path,
        nodes,
        counters,
        partial_ranking,
        "mft",
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

fn enumerate_mft<F, P>(
    volume: &File,
    cancel: &AtomicBool,
    started_at: Instant,
    root: &Path,
    on_progress: &mut F,
    mut include: P,
) -> Result<Vec<Record>, NativeScanError>
where
    F: FnMut(ScanProgress),
    P: FnMut(u64, u64, u32) -> bool,
{
    let mut request = MFT_ENUM_DATA_V0 {
        StartFileReferenceNumber: 0,
        LowUsn: 0,
        HighUsn: i64::MAX,
    };
    let mut buffer = vec![0_u8; ENUM_BUFFER_SIZE];
    let mut records = Vec::new();
    let mut last_progress_at = Instant::now();

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(fatal("Scan cancelled."));
        }
        let mut returned = 0_u32;
        let success = unsafe {
            DeviceIoControl(
                volume.as_raw_handle(),
                FSCTL_ENUM_USN_DATA,
                &request as *const MFT_ENUM_DATA_V0 as *const c_void,
                size_of::<MFT_ENUM_DATA_V0>() as u32,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut returned,
                null_mut(),
            )
        };
        if success == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_HANDLE_EOF as i32) {
                break;
            }
            return if unavailable_error(&error) {
                Err(NativeScanError::Unavailable)
            } else {
                Err(fatal(format!(
                    "Could not enumerate the NTFS index: {error}"
                )))
            };
        }

        let returned = usize::try_from(returned).map_err(|_| NativeScanError::Unavailable)?;
        let (continuation, mut batch) =
            mft::parse_batch_filtered(&buffer[..returned], &mut include)
                .map_err(|_| NativeScanError::Unavailable)?;
        if continuation <= request.StartFileReferenceNumber {
            return Err(NativeScanError::Unavailable);
        }
        request.StartFileReferenceNumber = continuation;
        records.append(&mut batch);

        if last_progress_at.elapsed() >= PROGRESS_INTERVAL {
            on_progress(empty_progress(root, started_at));
            last_progress_at = Instant::now();
        }
    }
    Ok(records)
}

#[allow(clippy::too_many_arguments)]
fn measure_files<F>(
    volume: &File,
    records: &[Record],
    file_nodes: &[(usize, usize)],
    volume_serial: u64,
    nodes: &mut [InternalNode],
    counters: &mut ScanCounters,
    partial_ranking: &mut PartialRanking,
    hard_links: &mut Vec<HardLinkCandidate>,
    cancel: &AtomicBool,
    started_at: Instant,
    root: &Path,
    on_progress: &mut F,
) -> Result<(), NativeScanError>
where
    F: FnMut(ScanProgress),
{
    if file_nodes.is_empty() {
        return Ok(());
    }

    let worker_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, MAX_WORKERS)
        .min(file_nodes.len());
    let chunk_size = file_nodes.len().div_ceil(worker_count);
    let volume_handle = volume.as_raw_handle() as usize;
    let result_queue_capacity = worker_count.saturating_mul(RESULT_BATCHES_PER_WORKER);
    let (sender, receiver) =
        mpsc::sync_channel::<Vec<(usize, WindowsMeasurement)>>(result_queue_capacity.max(1));
    let mut completed_files = 0_u64;
    let mut last_progress_at = Instant::now();
    let directory_count = counters.directories_scanned;

    std::thread::scope(|scope| -> Result<(), NativeScanError> {
        for chunk in file_nodes.chunks(chunk_size) {
            let sender = sender.clone();
            scope.spawn(move || {
                let mut batch = Vec::with_capacity(MEASUREMENT_BATCH_SIZE);
                for &(record_index, node_id) in chunk {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let record = &records[record_index];
                    let measurement =
                        measure_file_by_id(volume_handle, record.reference, volume_serial)
                            .unwrap_or(WindowsMeasurement {
                                measured: MeasuredMetadata {
                                    metadata_error: true,
                                    ..MeasuredMetadata::default()
                                },
                                link_count: 0,
                            });
                    batch.push((node_id, measurement));
                    if batch.len() == MEASUREMENT_BATCH_SIZE {
                        if sender.send(batch).is_err() {
                            return;
                        }
                        batch = Vec::with_capacity(MEASUREMENT_BATCH_SIZE);
                    }
                }
                if !batch.is_empty() {
                    let _ = sender.send(batch);
                }
            });
        }
        drop(sender);

        let mut cancelled = false;
        while completed_files < file_nodes.len() as u64 {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            match receiver.recv_timeout(PROGRESS_INTERVAL) {
                Ok(batch) => {
                    completed_files = completed_files.saturating_add(batch.len() as u64);
                    for (node_id, measurement) in batch {
                        apply_measurement(
                            node_id,
                            measurement,
                            nodes,
                            counters,
                            partial_ranking,
                            hard_links,
                        );
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(NativeScanError::Unavailable);
                }
            }

            if last_progress_at.elapsed() >= PROGRESS_INTERVAL {
                on_progress(ScanProgress {
                    phase: ScanPhase::Scanning,
                    entries_scanned: completed_files.saturating_add(directory_count),
                    files_scanned: completed_files,
                    directories_scanned: directory_count,
                    logical_bytes: counters.observed_logical_bytes,
                    allocated_bytes: counters.observed_allocated_bytes,
                    skipped_entries: counters.skipped_entries,
                    current_path: root.to_string_lossy().into_owned(),
                    elapsed_ms: super::elapsed_ms(started_at),
                    largest_items: Vec::new(),
                });
                last_progress_at = Instant::now();
            }
        }
        if cancelled {
            drop(receiver);
            return Err(fatal("Scan cancelled."));
        }
        Ok(())
    })?;

    Ok(())
}

fn apply_measurement(
    node_id: usize,
    measurement: WindowsMeasurement,
    nodes: &mut [InternalNode],
    counters: &mut ScanCounters,
    partial_ranking: &mut PartialRanking,
    hard_links: &mut Vec<HardLinkCandidate>,
) {
    if measurement.measured.metadata_error {
        counters.skipped_entries = counters.skipped_entries.saturating_add(1);
        return;
    }

    let measured = measurement.measured;
    let node = &mut nodes[node_id];
    node.logical_bytes = measured.logical_bytes;
    node.allocated_bytes = measured.allocated_bytes;
    node.scan_revision = measured.scan_revision;
    counters.observed_logical_bytes = counters
        .observed_logical_bytes
        .saturating_add(measured.logical_bytes);
    counters.observed_allocated_bytes = counters
        .observed_allocated_bytes
        .saturating_add(measured.allocated_bytes);

    if let Some(identity) = measured.file_identity {
        let previous = counters.hard_link_owners.insert(
            identity,
            HardLinkOwner {
                node_id,
                logical_bytes: measured.logical_bytes,
                allocated_bytes: measured.allocated_bytes,
            },
        );
        debug_assert!(previous.is_none(), "MFT references must be unique");
    }
    observe_partial_file(partial_ranking, nodes, node_id, None);
    if measurement.link_count > 1 {
        hard_links.push(HardLinkCandidate {
            node_id,
            measured,
            link_count: measurement.link_count,
        });
    }
}

fn measure_file_by_id(
    volume_handle: usize,
    reference: u64,
    volume_serial: u64,
) -> io::Result<WindowsMeasurement> {
    let descriptor = FILE_ID_DESCRIPTOR {
        dwSize: size_of::<FILE_ID_DESCRIPTOR>() as u32,
        Type: FileIdType,
        Anonymous: FILE_ID_DESCRIPTOR_0 {
            FileId: reference as i64,
        },
    };
    let handle = unsafe {
        OpenFileById(
            volume_handle as _,
            &descriptor,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    };
    let file = owned_file(handle)?;
    let mut standard = FILE_STANDARD_INFO::default();
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut basic = FILE_BASIC_INFO::default();
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(WindowsMeasurement {
        measured: MeasuredMetadata {
            logical_bytes: u64::try_from(standard.EndOfFile).unwrap_or(0),
            allocated_bytes: u64::try_from(standard.AllocationSize).unwrap_or(0),
            filesystem_id: Some(volume_serial),
            file_identity: (standard.NumberOfLinks > 1)
                .then_some(FileIdentity(volume_serial, reference)),
            scan_revision: ScannedFileRevision::from_raw_parts(
                volume_serial,
                reference,
                basic.LastWriteTime as u64,
                basic.ChangeTime as u64,
            ),
            metadata_error: false,
        },
        link_count: standard.NumberOfLinks,
    })
}

fn ingest_hard_links(
    root: &Path,
    nodes: &mut Vec<InternalNode>,
    counters: &mut ScanCounters,
    partial_ranking: &mut PartialRanking,
    candidates: &[HardLinkCandidate],
    cancel: &AtomicBool,
) -> Result<(), NativeScanError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(fatal("Scan cancelled."));
    }
    reserve_hard_link_nodes(nodes, candidates)?;

    for candidate in candidates {
        if cancel.load(Ordering::Relaxed) {
            return Err(fatal("Scan cancelled."));
        }
        let primary_path = node_path(root, nodes, candidate.node_id);
        let link_paths = match enumerate_hard_links(&primary_path) {
            Ok(paths) => paths,
            Err(_) => {
                counters.skipped_entries = counters.skipped_entries.saturating_add(1);
                continue;
            }
        };

        let mut unique_paths = HashSet::with_capacity(link_paths.len());
        let mut resolved_links = 0_u32;
        for relative_path in link_paths {
            if !unique_paths.insert(relative_path.clone()) {
                continue;
            }
            let Some((parent, name)) = resolve_link_parent(nodes, &relative_path) else {
                continue;
            };
            if nodes[parent]
                .children
                .iter()
                .any(|child| nodes[*child].name() == name)
            {
                resolved_links = resolved_links.saturating_add(1);
                continue;
            }

            let (node_id, replaced_owner) = counters.push_node(
                nodes,
                parent,
                name.to_owned(),
                EntryKind::File,
                candidate.measured,
            );
            observe_partial_file(partial_ranking, nodes, node_id, replaced_owner);
            resolved_links = resolved_links.saturating_add(1);
        }
        if resolved_links != candidate.link_count {
            counters.skipped_entries = counters.skipped_entries.saturating_add(1);
        }
    }
    Ok(())
}

fn reserve_hard_link_nodes(
    nodes: &mut Vec<InternalNode>,
    candidates: &[HardLinkCandidate],
) -> Result<(), NativeScanError> {
    let additional_nodes = candidates.iter().try_fold(0_usize, |total, candidate| {
        total.checked_add(candidate.link_count.saturating_sub(1) as usize)
    });
    let additional_nodes = additional_nodes
        .ok_or_else(|| fatal("The NTFS hard-link index contains too many names."))?;
    nodes.try_reserve_exact(additional_nodes).map_err(|error| {
        fatal(format!(
            "Could not reserve memory for {additional_nodes} NTFS hard-link names: {error}"
        ))
    })
}

fn enumerate_hard_links(path: &Path) -> io::Result<Vec<PathBuf>> {
    let path = wide_null(path.as_os_str());
    let mut buffer = vec![0_u16; 512];
    let handle = loop {
        let mut length = buffer.len() as u32;
        let handle =
            unsafe { FindFirstFileNameW(path.as_ptr(), 0, &mut length, buffer.as_mut_ptr()) };
        if handle != INVALID_HANDLE_VALUE {
            break FindHandle(handle);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) {
            buffer.resize(length as usize, 0);
        } else {
            return Err(error);
        }
    };

    let mut paths = vec![link_path_from_buffer(&buffer)];
    loop {
        let mut length = buffer.len() as u32;
        let success = unsafe { FindNextFileNameW(handle.0, &mut length, buffer.as_mut_ptr()) };
        if success != 0 {
            paths.push(link_path_from_buffer(&buffer));
            continue;
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_HANDLE_EOF) => break,
            Some(ERROR_MORE_DATA) => buffer.resize(length as usize, 0),
            _ => return Err(error),
        }
    }
    Ok(paths)
}

fn link_path_from_buffer(buffer: &[u16]) -> PathBuf {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    Path::new(&OsString::from_wide(&buffer[..end]))
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect()
}

fn resolve_link_parent<'a>(
    nodes: &'a [InternalNode],
    relative_path: &'a Path,
) -> Option<(usize, &'a OsStr)> {
    let mut components = relative_path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => Some(name),
            _ => None,
        });
    let name = components.next_back()?;
    let mut parent = 0_usize;
    for component in components {
        parent = nodes[parent].children.iter().copied().find(|child| {
            matches!(nodes[*child].kind, EntryKind::Directory) && nodes[*child].name() == component
        })?;
    }
    Some((parent, name))
}

fn node_path(root: &Path, nodes: &[InternalNode], mut node_id: usize) -> PathBuf {
    let mut ancestors = Vec::new();
    while let Some(parent) = nodes[node_id].parent_id() {
        ancestors.push(node_id);
        node_id = parent;
    }
    let mut path = root.to_path_buf();
    for ancestor in ancestors.into_iter().rev() {
        path.push(nodes[ancestor].name());
    }
    path
}

fn open_path(path: &Path, access: u32) -> Result<File, NativeScanError> {
    let wide_path = wide_null(path.as_os_str());
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    owned_file(handle).map_err(|error| {
        if unavailable_error(&error) {
            NativeScanError::Unavailable
        } else {
            fatal(format!("Could not open {}: {error}", path.display()))
        }
    })
}

fn open_volume(volume_root: &[u16]) -> Result<File, NativeScanError> {
    let mut volume_name = vec![0_u16; WINDOWS_PATH_BUFFER];
    let success = unsafe {
        GetVolumeNameForVolumeMountPointW(
            volume_root.as_ptr(),
            volume_name.as_mut_ptr(),
            volume_name.len() as u32,
        )
    };
    if success == 0 {
        return Err(NativeScanError::Unavailable);
    }
    truncate_at_nul(&mut volume_name);
    if volume_name.last() == Some(&('\\' as u16)) {
        volume_name.pop();
    }
    volume_name.push(0);

    let handle = unsafe {
        CreateFileW(
            volume_name.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    owned_file(handle).map_err(|error| {
        if unavailable_error(&error) {
            NativeScanError::Unavailable
        } else {
            fatal(format!("Could not open the selected NTFS volume: {error}"))
        }
    })
}

fn volume_root(path: &Path) -> Result<Vec<u16>, NativeScanError> {
    let path = wide_null(path.as_os_str());
    let mut root = vec![0_u16; WINDOWS_PATH_BUFFER];
    let success =
        unsafe { GetVolumePathNameW(path.as_ptr(), root.as_mut_ptr(), root.len() as u32) };
    if success == 0 {
        return Err(NativeScanError::Unavailable);
    }
    truncate_at_nul(&mut root);
    root.push(0);
    Ok(root)
}

fn volume_root_reference(volume_root: &[u16]) -> Result<u64, NativeScanError> {
    let end = volume_root
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(volume_root.len());
    let path = PathBuf::from(OsString::from_wide(&volume_root[..end]));
    let handle = open_path(&path, FILE_READ_ATTRIBUTES)?;
    let information = file_information(&handle).map_err(|_| NativeScanError::Unavailable)?;
    Ok(u64::from(information.nFileIndexHigh) << 32 | u64::from(information.nFileIndexLow))
}

fn is_ntfs(volume_root: &[u16]) -> Result<bool, NativeScanError> {
    let mut filesystem = [0_u16; 32];
    let success = unsafe {
        GetVolumeInformationW(
            volume_root.as_ptr(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            filesystem.as_mut_ptr(),
            filesystem.len() as u32,
        )
    };
    if success == 0 {
        return Err(NativeScanError::Unavailable);
    }
    let end = filesystem
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(filesystem.len());
    Ok(String::from_utf16_lossy(&filesystem[..end]).eq_ignore_ascii_case("NTFS"))
}

fn file_information(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let success = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(information)
    }
}

fn owned_file(handle: *mut c_void) -> io::Result<File> {
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_handle(handle) })
    }
}

fn entry_kind(attributes: u32) -> EntryKind {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        EntryKind::Symlink
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

fn preallocate_node_arena(
    root: &Path,
    additional_nodes: usize,
) -> Result<Vec<InternalNode>, NativeScanError> {
    let capacity = additional_nodes
        .checked_add(1)
        .ok_or_else(|| fatal("The NTFS index contains too many entries."))?;
    let mut nodes = Vec::new();
    nodes.try_reserve_exact(capacity).map_err(|error| {
        fatal(format!(
            "Could not reserve memory for {capacity} NTFS entries: {error}"
        ))
    })?;
    nodes.push(InternalNode::root(root));
    Ok(nodes)
}

fn unavailable_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error().map(|code| code as u32),
        Some(
            ERROR_ACCESS_DENIED
                | ERROR_INVALID_FUNCTION
                | ERROR_INVALID_PARAMETER
                | ERROR_JOURNAL_NOT_ACTIVE
                | ERROR_NOT_SUPPORTED
        )
    )
}

fn empty_progress(root: &Path, started_at: Instant) -> ScanProgress {
    ScanProgress {
        phase: ScanPhase::Scanning,
        entries_scanned: 0,
        files_scanned: 0,
        directories_scanned: 0,
        logical_bytes: 0,
        allocated_bytes: 0,
        skipped_entries: 0,
        current_path: root.to_string_lossy().into_owned(),
        elapsed_ms: super::elapsed_ms(started_at),
        largest_items: Vec::new(),
    }
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn truncate_at_nul(value: &mut Vec<u16>) {
    if let Some(end) = value.iter().position(|unit| *unit == 0) {
        value.truncate(end);
    }
}

fn fatal(error: impl Into<String>) -> NativeScanError {
    NativeScanError::Fatal(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_volume_relative_hard_link_names() {
        let mut buffer = r"\cepa-validation\beta\hardlink.bin"
            .encode_utf16()
            .collect::<Vec<_>>();
        buffer.push(0);

        assert_eq!(
            link_path_from_buffer(&buffer),
            PathBuf::from(r"cepa-validation\beta\hardlink.bin")
        );
    }

    #[test]
    fn treats_directory_reparse_points_as_links() {
        assert!(matches!(
            entry_kind(FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT),
            EntryKind::Symlink
        ));
        assert!(matches!(
            entry_kind(FILE_ATTRIBUTE_DIRECTORY),
            EntryKind::Directory
        ));
    }

    #[test]
    fn preallocated_node_arena_holds_the_known_subtree_without_growth() {
        let Ok(mut nodes) = preallocate_node_arena(Path::new(r"C:\"), 17) else {
            panic!("the test node arena should fit in memory");
        };
        let reserved_capacity = nodes.capacity();
        let mut counters = ScanCounters::default();

        for index in 0..17 {
            counters.push_node(
                &mut nodes,
                0,
                OsString::from(format!("file-{index}")),
                EntryKind::File,
                MeasuredMetadata::default(),
            );
        }

        assert_eq!(nodes.len(), 18);
        assert_eq!(nodes.capacity(), reserved_capacity);
    }

    #[test]
    fn rejects_an_unrepresentable_ntfs_node_count() {
        let Err(NativeScanError::Fatal(error)) =
            preallocate_node_arena(Path::new(r"C:\"), usize::MAX)
        else {
            panic!("an overflowing node count must be rejected");
        };

        assert_eq!(error, "The NTFS index contains too many entries.");
    }

    #[test]
    fn hard_link_alias_reservation_prevents_geometric_arena_growth() {
        let Ok(mut nodes) = preallocate_node_arena(Path::new(r"C:\"), 2) else {
            panic!("the test node arena should fit in memory");
        };
        let mut counters = ScanCounters::default();
        for index in 0..2 {
            counters.push_node(
                &mut nodes,
                0,
                OsString::from(format!("primary-{index}")),
                EntryKind::File,
                MeasuredMetadata::default(),
            );
        }
        let candidates = [
            HardLinkCandidate {
                node_id: 1,
                measured: MeasuredMetadata::default(),
                link_count: 3,
            },
            HardLinkCandidate {
                node_id: 2,
                measured: MeasuredMetadata::default(),
                link_count: 2,
            },
        ];
        let Ok(()) = reserve_hard_link_nodes(&mut nodes, &candidates) else {
            panic!("the hard-link alias reservation should fit in memory");
        };
        let reserved_capacity = nodes.capacity();

        for index in 0..3 {
            counters.push_node(
                &mut nodes,
                0,
                OsString::from(format!("link-{index}")),
                EntryKind::File,
                MeasuredMetadata::default(),
            );
        }

        assert_eq!(nodes.len(), 6);
        assert_eq!(nodes.capacity(), reserved_capacity);
    }

    #[test]
    fn hard_link_cancellation_precedes_alias_reservation() {
        let Ok(mut nodes) = preallocate_node_arena(Path::new(r"C:\"), 0) else {
            panic!("the test node arena should fit in memory");
        };
        let original_capacity = nodes.capacity();
        let candidates = [HardLinkCandidate {
            node_id: 0,
            measured: MeasuredMetadata::default(),
            link_count: 100,
        }];
        let mut counters = ScanCounters::default();
        let mut ranking = PartialRanking::default();
        let error = ingest_hard_links(
            Path::new(r"C:\"),
            &mut nodes,
            &mut counters,
            &mut ranking,
            &candidates,
            &AtomicBool::new(true),
        );
        let Err(NativeScanError::Fatal(error)) = error else {
            panic!("hard-link ingestion must observe cancellation");
        };

        assert_eq!(error, "Scan cancelled.");
        assert_eq!(nodes.capacity(), original_capacity);
    }

    #[test]
    fn streams_file_measurements_into_prebuilt_nodes() {
        let mut nodes = vec![InternalNode::root(Path::new(r"C:\"))];
        let mut counters = ScanCounters::default();
        let mut ranking = PartialRanking::default();
        let mut hard_links = Vec::new();
        let (node_id, _) = counters.push_node(
            &mut nodes,
            0,
            OsString::from("shared.bin"),
            EntryKind::File,
            MeasuredMetadata::default(),
        );
        let identity = FileIdentity(7, 11);

        apply_measurement(
            node_id,
            WindowsMeasurement {
                measured: MeasuredMetadata {
                    logical_bytes: 4_096,
                    allocated_bytes: 8_192,
                    filesystem_id: Some(7),
                    file_identity: Some(identity),
                    scan_revision: None,
                    metadata_error: false,
                },
                link_count: 2,
            },
            &mut nodes,
            &mut counters,
            &mut ranking,
            &mut hard_links,
        );

        assert_eq!(nodes[node_id].logical_bytes, 4_096);
        assert_eq!(nodes[node_id].allocated_bytes, 8_192);
        assert_eq!(counters.observed_logical_bytes, 4_096);
        assert_eq!(counters.observed_allocated_bytes, 8_192);
        assert_eq!(counters.hard_link_owners.len(), 1);
        assert_eq!(ranking.candidates.len(), 1);
        assert_eq!(hard_links.len(), 1);
        assert_eq!(hard_links[0].node_id, node_id);
    }

    #[test]
    fn records_failed_streamed_measurements_without_ranking_them() {
        let mut nodes = vec![InternalNode::root(Path::new(r"C:\"))];
        let mut counters = ScanCounters::default();
        let mut ranking = PartialRanking::default();
        let mut hard_links = Vec::new();
        let (node_id, _) = counters.push_node(
            &mut nodes,
            0,
            OsString::from("unavailable.bin"),
            EntryKind::File,
            MeasuredMetadata::default(),
        );

        apply_measurement(
            node_id,
            WindowsMeasurement {
                measured: MeasuredMetadata {
                    metadata_error: true,
                    ..MeasuredMetadata::default()
                },
                link_count: 0,
            },
            &mut nodes,
            &mut counters,
            &mut ranking,
            &mut hard_links,
        );

        assert_eq!(counters.skipped_entries, 1);
        assert_eq!(nodes[node_id].logical_bytes, 0);
        assert!(ranking.candidates.is_empty());
        assert!(hard_links.is_empty());
    }
}
