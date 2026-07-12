use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::fs::File;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

use crate::scanner::{CompressionTarget, EntryKind};

use super::{CompressionCapabilityStatus, CompressionStateKind, inspect, probe};

const NTFS_MAX_UNCOMPRESSED_BYTES: u64 = 30 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CompressionOperation {
    Compress,
    Decompress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CompressionPlanStatus {
    Prepared,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PlanBlockerCode {
    WriterUnavailable,
    CapabilityUnavailable,
    StateUnavailable,
    AlreadyInRequestedState,
    FileTooLarge,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanBlocker {
    pub code: PlanBlockerCode,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompressionPlanPreview {
    pub plan_id: u64,
    pub scan_id: u64,
    pub node_id: u64,
    pub operation: CompressionOperation,
    pub status: CompressionPlanStatus,
    pub filesystem: String,
    pub algorithm: Option<String>,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub allocated_size_is_estimate: bool,
    pub estimated_read_bytes: u64,
    pub estimated_write_bytes: u64,
    pub required_free_space_bytes: Option<u64>,
    pub link_count: u64,
    pub warnings: Vec<String>,
    pub blockers: Vec<PlanBlocker>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PlanValidationStatus {
    Valid,
    Changed,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanValidation {
    pub plan_id: u64,
    pub status: PlanValidationStatus,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCompressionPlan {
    preview: CompressionPlanPreview,
    target: CompressionTarget,
    revision: FileRevision,
}

impl PreparedCompressionPlan {
    pub(crate) fn preview(&self) -> &CompressionPlanPreview {
        &self.preview
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileRevision {
    identity: StableIdentity,
    logical_bytes: u64,
    allocated_bytes: Option<u64>,
    modified_seconds: i64,
    modified_subseconds: i64,
    changed_seconds: i64,
    changed_subseconds: i64,
    link_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StableIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows { volume_serial: u64, file_id: u64 },
}

pub(super) fn prepare(
    plan_id: u64,
    scan_id: u64,
    node_id: u64,
    target: CompressionTarget,
    operation: CompressionOperation,
) -> Result<PreparedCompressionPlan, String> {
    if !matches!(target.kind, EntryKind::File) {
        return Err("Only regular files can be included in a compression plan.".into());
    }

    let revision = snapshot_revision(&target.path)
        .map_err(|error| format!("The file could not be opened safely for planning: {error}."))?;
    if revision.logical_bytes != target.logical_bytes {
        return Err("The file size changed after the scan; rescan before planning.".into());
    }
    if !target.allocated_size_is_estimate
        && revision.allocated_bytes != Some(target.allocated_bytes)
    {
        return Err("The file allocation changed after the scan; rescan before planning.".into());
    }

    let capability = probe(&target.path);
    let state = inspect(&target.path, target.kind);
    let mut blockers = Vec::new();
    if !capability.writer_available {
        blockers.push(PlanBlocker {
            code: PlanBlockerCode::WriterUnavailable,
            detail: "Cepa does not have a writable compression backend for this volume.".into(),
        });
    }
    if matches!(
        capability.status,
        CompressionCapabilityStatus::Unsupported | CompressionCapabilityStatus::Unavailable
    ) {
        blockers.push(PlanBlocker {
            code: PlanBlockerCode::CapabilityUnavailable,
            detail: capability.detail.clone(),
        });
    }
    if matches!(
        state.state,
        CompressionStateKind::Unavailable
            | CompressionStateKind::Unsupported
            | CompressionStateKind::NotApplicable
    ) {
        blockers.push(PlanBlocker {
            code: PlanBlockerCode::StateUnavailable,
            detail: state.detail.clone(),
        });
    }
    let already_requested = matches!(
        (operation, state.state),
        (
            CompressionOperation::Compress,
            CompressionStateKind::Compressed
        ) | (
            CompressionOperation::Decompress,
            CompressionStateKind::NotCompressed
        )
    );
    if already_requested {
        blockers.push(PlanBlocker {
            code: PlanBlockerCode::AlreadyInRequestedState,
            detail: "The filesystem already reports the requested compression state.".into(),
        });
    }
    if operation == CompressionOperation::Compress
        && capability.filesystem.eq_ignore_ascii_case("ntfs")
        && target.logical_bytes > NTFS_MAX_UNCOMPRESSED_BYTES
    {
        blockers.push(PlanBlocker {
            code: PlanBlockerCode::FileTooLarge,
            detail: "NTFS compression does not support files larger than 30 GiB.".into(),
        });
    }

    let mut warnings = Vec::new();
    if revision.link_count > 1 {
        warnings.push(format!(
            "This file has {} hard links; changing one path changes their shared data.",
            revision.link_count
        ));
    }
    if target.allocated_size_is_estimate {
        warnings.push(
            "The scan backend estimated allocated size; apply must measure it again natively."
                .into(),
        );
    }

    let preview = CompressionPlanPreview {
        plan_id,
        scan_id,
        node_id,
        operation,
        status: if blockers.is_empty() {
            CompressionPlanStatus::Prepared
        } else {
            CompressionPlanStatus::Blocked
        },
        filesystem: capability.filesystem,
        algorithm: (operation == CompressionOperation::Compress)
            .then(|| capability.algorithms.first().cloned())
            .flatten(),
        logical_bytes: target.logical_bytes,
        allocated_bytes: target.allocated_bytes,
        allocated_size_is_estimate: target.allocated_size_is_estimate,
        estimated_read_bytes: target.logical_bytes,
        estimated_write_bytes: target.logical_bytes,
        // No writer exists yet, so no backend has established a safe margin.
        required_free_space_bytes: None,
        link_count: revision.link_count,
        warnings,
        blockers,
    };
    Ok(PreparedCompressionPlan {
        preview,
        target,
        revision,
    })
}

pub(super) fn revalidate(plan: &PreparedCompressionPlan) -> PlanValidation {
    match snapshot_revision(&plan.target.path) {
        Ok(revision) if revision == plan.revision => PlanValidation {
            plan_id: plan.preview.plan_id,
            status: PlanValidationStatus::Valid,
            detail: "The file identity and revision still match this plan.".into(),
        },
        Ok(_) => PlanValidation {
            plan_id: plan.preview.plan_id,
            status: PlanValidationStatus::Changed,
            detail: "The file identity or revision changed; discard this plan and rescan.".into(),
        },
        Err(error) => PlanValidation {
            plan_id: plan.preview.plan_id,
            status: PlanValidationStatus::Unavailable,
            detail: format!("The planned file could not be reopened safely: {error}."),
        },
    }
}

#[cfg(unix)]
fn snapshot_revision(path: &Path) -> io::Result<FileRevision> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is no longer a regular file",
        ));
    }
    Ok(FileRevision {
        identity: StableIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        logical_bytes: metadata.len(),
        allocated_bytes: Some(metadata.blocks().saturating_mul(512)),
        modified_seconds: metadata.mtime(),
        modified_subseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_subseconds: metadata.ctime_nsec(),
        link_count: metadata.nlink(),
    })
}

#[cfg(windows)]
fn snapshot_revision(path: &Path) -> io::Result<FileRevision> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileBasicInfo, FileStandardInfo,
        GetFileInformationByHandle, GetFileInformationByHandleEx, OPEN_EXISTING,
    };

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide.push(0);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is a reparse point",
        ));
    }
    let mut standard = FILE_STANDARD_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if standard.Directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is no longer a regular file",
        ));
    }
    let mut basic = FILE_BASIC_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    Ok(FileRevision {
        identity: StableIdentity::Windows {
            volume_serial: u64::from(information.dwVolumeSerialNumber),
            file_id: u64::from(information.nFileIndexHigh) << 32
                | u64::from(information.nFileIndexLow),
        },
        logical_bytes: u64::try_from(standard.EndOfFile).unwrap_or(0),
        allocated_bytes: Some(u64::try_from(standard.AllocationSize).unwrap_or(0)),
        modified_seconds: basic.LastWriteTime,
        modified_subseconds: 0,
        changed_seconds: basic.ChangeTime,
        changed_subseconds: 0,
        link_count: u64::from(standard.NumberOfLinks),
    })
}

#[cfg(not(any(unix, windows)))]
fn snapshot_revision(_path: &Path) -> io::Result<FileRevision> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "stable file identity is unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn target(path: &Path) -> CompressionTarget {
        let metadata = fs::metadata(path).expect("read fixture metadata");
        #[cfg(unix)]
        let allocated_bytes = {
            use std::os::unix::fs::MetadataExt;
            metadata.blocks() * 512
        };
        #[cfg(not(unix))]
        let allocated_bytes = metadata.len();
        CompressionTarget {
            path: path.to_path_buf(),
            kind: EntryKind::File,
            logical_bytes: metadata.len(),
            allocated_bytes,
            allocated_size_is_estimate: !cfg!(unix),
        }
    }

    #[test]
    fn plan_wire_contract_is_blocked_without_a_writer() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, vec![7_u8; 4096]).expect("write fixture");

        let plan =
            prepare(3, 5, 7, target(&path), CompressionOperation::Compress).expect("prepare plan");
        let wire = serde_json::to_value(plan.preview()).expect("serialize preview");

        assert_eq!(wire["planId"], 3);
        assert_eq!(wire["scanId"], 5);
        assert_eq!(wire["nodeId"], 7);
        assert_eq!(wire["status"], "blocked");
        assert!(
            wire["blockers"].as_array().is_some_and(|items| {
                items.iter().any(|item| item["code"] == "writerUnavailable")
            })
        );
    }

    #[test]
    fn revalidation_detects_same_length_revision_changes() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, b"before").expect("write fixture");
        let plan =
            prepare(1, 1, 1, target(&path), CompressionOperation::Compress).expect("prepare plan");

        // Some Linux filesystems expose nanoseconds but source timestamps from a
        // coarser kernel clock tick. Cross that tick so this remains a metadata
        // revision test rather than implying that timestamps fingerprint data.
        std::thread::sleep(std::time::Duration::from_millis(25));
        fs::write(&path, b"after!").expect("change fixture without changing length");
        let current = snapshot_revision(&path).expect("snapshot changed fixture");
        assert_ne!(
            plan.revision, current,
            "rewrite must change metadata revision"
        );
        let validation = revalidate(&plan);

        assert_eq!(validation.status, PlanValidationStatus::Changed);
    }

    #[cfg(unix)]
    #[test]
    fn revalidation_refuses_a_symlink_replacement() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        let replacement = temp.path().join("replacement.bin");
        fs::write(&path, b"original").expect("write fixture");
        fs::write(&replacement, b"replacement").expect("write replacement");
        let plan =
            prepare(1, 1, 1, target(&path), CompressionOperation::Compress).expect("prepare plan");
        fs::remove_file(&path).expect("remove planned file");
        symlink(&replacement, &path).expect("replace with symlink");

        let validation = revalidate(&plan);

        assert_eq!(validation.status, PlanValidationStatus::Unavailable);
    }

    #[test]
    fn preparation_rejects_scan_size_drift() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, b"changed").expect("write fixture");
        let mut stale = target(&path);
        stale.logical_bytes = stale.logical_bytes.saturating_add(1);

        let error = prepare(1, 1, 1, stale, CompressionOperation::Compress)
            .expect_err("stale scan size must fail");

        assert!(error.contains("size changed"));
    }
}
