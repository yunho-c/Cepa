use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

use crate::file_revision::{self, ScannedFileRevision};
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
    scanned: ScannedFileRevision,
    logical_bytes: u64,
    allocated_bytes: Option<u64>,
    link_count: u64,
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
    let scanned_revision = target.scan_revision.ok_or_else(|| {
        "The scan could not retain a stable file identity and revision; rescan with a supported backend before planning."
            .to_string()
    })?;
    if revision.scanned != scanned_revision {
        return Err(
            "The file identity or revision changed after the scan; rescan before planning.".into(),
        );
    }
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

fn snapshot_revision(path: &Path) -> io::Result<FileRevision> {
    let snapshot = file_revision::snapshot_no_follow(path)?;
    Ok(FileRevision {
        scanned: snapshot.scanned,
        logical_bytes: snapshot.logical_bytes,
        allocated_bytes: snapshot.allocated_bytes,
        link_count: snapshot.link_count,
    })
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
            scan_revision: file_revision::snapshot_no_follow(path)
                .ok()
                .map(|snapshot| snapshot.scanned),
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

    #[test]
    fn preparation_rejects_same_length_replacement_after_scan() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        let original = temp.path().join("original.bin");
        fs::write(&path, b"before").expect("write fixture");
        let stale = target(&path);
        fs::rename(&path, &original).expect("move scanned file aside");
        fs::write(&path, b"after!").expect("replace fixture at the same length");

        let error = prepare(1, 1, 1, stale, CompressionOperation::Compress)
            .expect_err("replacement after scan must fail");

        assert!(error.contains("identity or revision changed"));
    }

    #[test]
    fn preparation_rejects_same_length_rewrite_after_scan() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, b"before").expect("write fixture");
        let stale = target(&path);
        std::thread::sleep(std::time::Duration::from_millis(25));
        fs::write(&path, b"after!").expect("rewrite fixture at the same length");

        let error = prepare(1, 1, 1, stale, CompressionOperation::Compress)
            .expect_err("rewrite after scan must fail");

        assert!(error.contains("identity or revision changed"));
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

    #[test]
    fn preparation_rejects_an_unavailable_scan_revision() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, b"unchanged").expect("write fixture");
        let mut target = target(&path);
        target.scan_revision = None;

        let error = prepare(1, 1, 1, target, CompressionOperation::Compress)
            .expect_err("missing scan identity must fail");

        assert!(error.contains("could not retain a stable file identity"));
    }
}
