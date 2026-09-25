//! Handle-relative Windows fallback with local-only directory enumeration.
use super::windows_local as local;
use super::{
    EntryKind, FileIdentity, InternalNode, MeasuredMetadata, PartialRanking, ScanCounters,
    ScanOutput, ScanPhase, ScanProgress, ScanSemantics, TraversalProgressClock, finish_scan,
    observe_partial_file,
};
use crate::file_revision::ScannedFileRevision;
use rayon::prelude::*;
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
use windows_sys::Win32::Storage::FileSystem::*;

#[derive(Debug)]
struct Entry {
    name: OsString,
    attributes: u32,
}
struct InspectedEntry {
    name: OsString,
    result: io::Result<LocalEntry>,
}
struct LocalEntry {
    kind: EntryKind,
    measured: MeasuredMetadata,
    directory: Option<File>,
}
struct Frame {
    file: File,
    path: PathBuf,
    node: usize,
    pending: std::vec::IntoIter<Entry>,
    entries: std::vec::IntoIter<InspectedEntry>,
}

fn parse_entries(bytes: &[u8]) -> io::Result<Vec<Entry>> {
    let invalid = || io::Error::other("Invalid Windows directory record");
    let mut offset = 0;
    let mut entries = Vec::new();
    loop {
        let record = bytes.get(offset..).ok_or_else(invalid)?;
        if record.len() < 64 {
            return Err(invalid());
        }
        let word = |start: usize| u32::from_le_bytes(record[start..start + 4].try_into().unwrap());
        let next = word(0) as usize;
        let length = word(60) as usize;
        let end = 64_usize.checked_add(length).ok_or_else(invalid)?;
        if length == 0
            || !length.is_multiple_of(2)
            || end > record.len()
            || (next != 0 && (next < end || !next.is_multiple_of(8) || next >= record.len()))
        {
            return Err(invalid());
        }
        let wide = record[64..end]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>();
        if wide.iter().any(|c| matches!(*c, 0 | 47 | 58 | 92)) {
            return Err(invalid());
        }
        let name = OsString::from_wide(&wide);
        if name != "." && name != ".." {
            entries.push(Entry {
                name,
                attributes: word(56),
            });
        }
        if next == 0 {
            break;
        }
        offset += next;
    }
    Ok(entries)
}

fn directory_metadata(file: &File) -> io::Result<MeasuredMetadata> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if local::needs_recall(information.dwFileAttributes) {
        return Err(local::cloud_error());
    }
    // Directory sizes/revisions are not retained; avoid the file-only standard
    // and basic information queries while still enforcing the volume boundary.
    Ok(MeasuredMetadata {
        filesystem_id: Some(u64::from(information.dwVolumeSerialNumber)),
        ..MeasuredMetadata::default()
    })
}

pub(super) fn measure(file: &File) -> io::Result<MeasuredMetadata> {
    let mut identity = BY_HANDLE_FILE_INFORMATION::default();
    let mut standard = FILE_STANDARD_INFO::default();
    let mut basic = FILE_BASIC_INFO::default();
    // All queries use the same metadata-only handle; no file data is read.
    let ok = unsafe {
        GetFileInformationByHandle(file.as_raw_handle(), &mut identity) != 0
            && GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            ) != 0
            && GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileBasicInfo,
                (&mut basic as *mut FILE_BASIC_INFO).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            ) != 0
    };
    if !ok {
        return Err(io::Error::last_os_error());
    }
    if local::needs_recall(basic.FileAttributes) {
        return Err(local::cloud_error());
    }
    let serial = u64::from(identity.dwVolumeSerialNumber);
    let id = u64::from(identity.nFileIndexHigh) << 32 | u64::from(identity.nFileIndexLow);
    Ok(MeasuredMetadata {
        logical_bytes: u64::try_from(standard.EndOfFile).unwrap_or(0),
        allocated_bytes: u64::try_from(standard.AllocationSize).unwrap_or(0),
        filesystem_id: Some(serial),
        file_identity: (standard.NumberOfLinks > 1).then_some(FileIdentity(serial, id)),
        scan_revision: ScannedFileRevision::from_raw_parts(
            serial,
            id,
            basic.LastWriteTime as u64,
            basic.ChangeTime as u64,
        ),
        metadata_error: false,
    })
}

fn inspect(parent: &File, entry: &Entry) -> io::Result<LocalEntry> {
    if local::enumeration_needs_recall(entry.attributes) {
        return Err(local::cloud_error());
    }
    let file = local::open_child(parent, &entry.name, false)?;
    let attributes = local::attributes(&file)?;
    if local::needs_recall(attributes.FileAttributes) {
        return Err(local::cloud_error());
    }
    let kind = local::classify(attributes.FileAttributes, attributes.ReparseTag);
    let mut measured = match kind {
        EntryKind::File => measure(&file)?,
        EntryKind::Directory => directory_metadata(&file)?,
        _ => MeasuredMetadata::default(),
    };
    let directory = if kind == EntryKind::Directory {
        match local::directory_handle(&file) {
            Ok(directory) => Some(directory),
            Err(error) if local::is_cloud_error(&error) => return Err(error),
            // Keep the inaccessible directory itself, without descending.
            Err(_) => {
                measured.metadata_error = true;
                None
            }
        }
    } else {
        None
    };
    Ok(LocalEntry {
        kind,
        measured,
        directory,
    })
}

pub(super) fn scan_path<F>(
    root: &Path,
    cancel: Arc<AtomicBool>,
    mut on_progress: F,
) -> Result<ScanOutput, String>
where
    F: FnMut(ScanProgress),
{
    let _mode = local::PlaceholderMode::new().map_err(|e| e.to_string())?;
    let (root, file) = local::open_root(root).map_err(|e| e.to_string())?;
    let file = local::directory_handle(&file).map_err(|e| e.to_string())?;
    let hard_link_deduplication_supported = local::has_ntfs_file_ids(&file);
    let root_filesystem = directory_metadata(&file)
        .map_err(|e| e.to_string())?
        .filesystem_id;
    let started_at = Instant::now();
    let root_node_path: Arc<Path> = Arc::from(root.clone());
    let mut nodes = vec![InternalNode::root(&root)];
    let mut counters = ScanCounters::default();
    let mut ranking = PartialRanking::default();
    let mut clock = TraversalProgressClock::new(started_at);
    let threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, 8);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| e.to_string())?;
    let mut stack = vec![Frame {
        file,
        path: root.clone(),
        node: 0,
        pending: Vec::new().into_iter(),
        entries: Vec::new().into_iter(),
    }];
    // One reusable aligned syscall buffer; frames retain at most one parsed
    // batch per ancestor, never the complete contents of a wide directory.
    let mut buffer = vec![0_u64; 16 * 1024 / 8];
    while let Some(frame) = stack.last_mut() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Scan cancelled.".into());
        }
        let entry = match frame.entries.next() {
            Some(entry) => entry,
            None => {
                if !frame.pending.as_slice().is_empty() {
                    // Bound each parallel handoff to 32 entries. Cancellation is
                    // checked before every worker's IO as well as every ingested
                    // result; no full wide-directory measurement queue is retained.
                    let batch: Vec<_> = frame.pending.by_ref().take(32).collect();
                    let inspected: Result<Vec<_>, String> = pool.install(|| {
                        batch
                            .into_par_iter()
                            .map(|entry| {
                                if cancel.load(Ordering::Relaxed) {
                                    return Err("Scan cancelled.".into());
                                }
                                // Thread-local exposure must be installed on every worker,
                                // and restored before the Rayon thread is reused.
                                let _mode =
                                    local::PlaceholderMode::new().map_err(|e| e.to_string())?;
                                let result = inspect(&frame.file, &entry);
                                Ok(InspectedEntry {
                                    name: entry.name,
                                    result,
                                })
                            })
                            .collect()
                    });
                    frame.entries = inspected?.into_iter();
                    continue;
                }
                match local::read_directory(&frame.file, &mut buffer) {
                    Ok(0) => {
                        stack.pop();
                    }
                    Ok(length) => {
                        // SAFETY: the initialized u64 buffer is readable as bytes.
                        let bytes = unsafe {
                            std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length)
                        };
                        frame.pending =
                            parse_entries(bytes).map_err(|e| e.to_string())?.into_iter();
                    }
                    Err(error) => {
                        if frame.node == 0 {
                            return Err(format!(
                                "Could not enumerate {} locally: {error}",
                                root.display()
                            ));
                        }
                        counters.skipped_entries += 1;
                        stack.pop();
                    }
                }
                continue;
            }
        };
        let LocalEntry {
            kind,
            mut measured,
            directory,
        } = match entry.result {
            Ok(local) => local,
            Err(error) if local::is_cloud_error(&error) => {
                counters.skipped_cloud_entries += 1;
                continue;
            }
            Err(_) => {
                counters.skipped_entries += 1;
                continue;
            }
        };
        if measured.metadata_error {
            counters.skipped_entries += 1;
        }
        if matches!((root_filesystem, measured.filesystem_id), (Some(root), Some(child)) if root != child)
        {
            counters.skipped_filesystems += 1;
            continue;
        }
        if !hard_link_deduplication_supported {
            measured.file_identity = None;
        }
        if kind == EntryKind::Directory {
            measured.file_identity = None;
            measured.scan_revision = None;
        }
        let path = frame.path.join(&entry.name);
        let (id, replaced) = counters.push_node(&mut nodes, frame.node, entry.name, kind, measured);
        observe_partial_file(&mut ranking, &nodes, id, replaced);
        if clock.should_check_clock() {
            let now = Instant::now();
            if clock.update_due_at(now) {
                on_progress(ScanProgress {
                    phase: ScanPhase::Scanning,
                    entries_scanned: counters.files_scanned + counters.directories_scanned,
                    files_scanned: counters.files_scanned,
                    directories_scanned: counters.directories_scanned,
                    logical_bytes: counters.observed_logical_bytes,
                    allocated_bytes: counters.observed_allocated_bytes,
                    skipped_entries: counters.skipped_entries,
                    current_path: path.to_string_lossy().into_owned(),
                    elapsed_ms: super::elapsed_ms(started_at),
                    largest_items: ranking.items(&nodes),
                });
                clock.mark_updated(now);
            }
        }
        if let Some(file) = directory {
            stack.push(Frame {
                file,
                path,
                node: id,
                pending: Vec::new().into_iter(),
                entries: Vec::new().into_iter(),
            });
        }
    }
    finish_scan(
        root,
        root_node_path,
        nodes,
        counters,
        ranking,
        "win32",
        ScanSemantics {
            allocated_size_is_estimate: false,
            hard_link_deduplication_supported,
            same_filesystem_enforced: true,
        },
        started_at,
        Instant::now(),
        &cancel,
        &mut on_progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(name: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0; 64];
        bytes[60..64].copy_from_slice(&((name.len() * 2) as u32).to_le_bytes());
        for word in name {
            bytes.extend(word.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn parses_counted_utf16_and_rejects_malformed_directory_batches() {
        let bytes = record(&[0xD800, b'x' as u16]);
        assert_eq!(
            parse_entries(&bytes).unwrap()[0].name,
            OsString::from_wide(&[0xD800, b'x' as u16])
        );
        assert!(parse_entries(&bytes[..bytes.len() - 1]).is_err());
        let mut invalid = bytes.clone();
        invalid[0..4].copy_from_slice(&8_u32.to_le_bytes());
        assert!(parse_entries(&invalid).is_err());
        let mut invalid = bytes;
        invalid[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_entries(&invalid).is_err());
        assert!(parse_entries(&record(&[b'.' as u16])).unwrap().is_empty());
        assert!(parse_entries(&record(&[b'a' as u16, b'\\' as u16, b'b' as u16])).is_err());
    }
}
