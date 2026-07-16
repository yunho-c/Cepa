use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::file_revision::{self, ScannedFileRevision};
use crate::scanner::{CompressionTarget, EntryKind};

use super::{CompressionCapabilityStatus, CompressionStateKind, inspect_open_file, probe};

const NTFS_MAX_UNCOMPRESSED_BYTES: u64 = 30 * 1024 * 1024 * 1024;
const CONTENT_HASH_CHUNK_BYTES: usize = 1024 * 1024;

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
    content_digest: ContentDigest,
    identity_anchor: Arc<File>,
}

impl PreparedCompressionPlan {
    pub(crate) fn preview(&self) -> &CompressionPlanPreview {
        &self.preview
    }

    #[cfg(test)]
    pub(crate) fn identity_anchor_weak(&self) -> std::sync::Weak<File> {
        Arc::downgrade(&self.identity_anchor)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileRevision {
    scanned: ScannedFileRevision,
    logical_bytes: u64,
    allocated_bytes: Option<u64>,
    link_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContentDigest([u8; blake3::OUT_LEN]);

#[derive(Debug)]
enum IntegrityError {
    Cancelled,
    Changed,
    Io(io::Error),
}

pub(super) fn prepare(
    plan_id: u64,
    scan_id: u64,
    node_id: u64,
    target: CompressionTarget,
    operation: CompressionOperation,
    cancel: &AtomicBool,
) -> Result<PreparedCompressionPlan, String> {
    if !matches!(target.kind, EntryKind::File) {
        return Err("Only regular files can be included in a compression plan.".into());
    }

    let opened = file_revision::open_content_snapshot_no_follow(&target.path)
        .map_err(|error| format!("The file could not be opened safely for planning: {error}."))?;
    let revision = FileRevision::from(opened.snapshot);
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
    let state = inspect_open_file(&opened.file, target.kind);
    let content_digest =
        capture_content_digest(&opened.file, &revision, cancel).map_err(prepare_integrity_error)?;
    validate_anchor_binding(&target.path, &opened.file, &revision)?;
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
        content_digest,
        identity_anchor: Arc::new(opened.file),
    })
}

pub(super) fn revalidate(plan: &PreparedCompressionPlan, cancel: &AtomicBool) -> PlanValidation {
    let rebound = match snapshot_revision(&plan.target.path) {
        Ok(revision) => revision,
        Err(error) => {
            return PlanValidation {
                plan_id: plan.preview.plan_id,
                status: PlanValidationStatus::Unavailable,
                detail: format!("The planned file could not be reopened safely: {error}."),
            };
        }
    };
    let anchored = match snapshot_anchor_revision(&plan.identity_anchor) {
        Ok(revision) => revision,
        Err(error) => {
            return PlanValidation {
                plan_id: plan.preview.plan_id,
                status: PlanValidationStatus::Unavailable,
                detail: format!("The plan's retained identity anchor could not be read: {error}."),
            };
        }
    };
    if rebound != anchored {
        return PlanValidation {
            plan_id: plan.preview.plan_id,
            status: PlanValidationStatus::Changed,
            detail: "The path now refers to a different file; discard this plan and rescan.".into(),
        };
    }
    if anchored != plan.revision {
        return PlanValidation {
            plan_id: plan.preview.plan_id,
            status: PlanValidationStatus::Changed,
            detail: "The retained file changed; discard this plan and rescan.".into(),
        };
    }
    let content_digest = match capture_content_digest(&plan.identity_anchor, &anchored, cancel) {
        Ok(digest) => digest,
        Err(IntegrityError::Cancelled) => {
            return PlanValidation {
                plan_id: plan.preview.plan_id,
                status: PlanValidationStatus::Unavailable,
                detail: "Plan validation was superseded by a newer request.".into(),
            };
        }
        Err(IntegrityError::Changed) => {
            return PlanValidation {
                plan_id: plan.preview.plan_id,
                status: PlanValidationStatus::Changed,
                detail: "The retained file changed while its content was being validated; discard this plan and rescan."
                    .into(),
            };
        }
        Err(IntegrityError::Io(error)) => {
            return PlanValidation {
                plan_id: plan.preview.plan_id,
                status: PlanValidationStatus::Unavailable,
                detail: format!("The retained file content could not be read: {error}."),
            };
        }
    };
    if content_digest != plan.content_digest {
        return PlanValidation {
            plan_id: plan.preview.plan_id,
            status: PlanValidationStatus::Changed,
            detail: "The retained file content changed; discard this plan and rescan.".into(),
        };
    }
    let rebound_after_hash = match snapshot_revision(&plan.target.path) {
        Ok(revision) => revision,
        Err(error) => {
            return PlanValidation {
                plan_id: plan.preview.plan_id,
                status: PlanValidationStatus::Unavailable,
                detail: format!("The planned file could not be rebound after validation: {error}."),
            };
        }
    };
    if rebound_after_hash != anchored {
        return PlanValidation {
            plan_id: plan.preview.plan_id,
            status: PlanValidationStatus::Changed,
            detail: "The path changed while its content was being validated; discard this plan and rescan."
                .into(),
        };
    }
    PlanValidation {
        plan_id: plan.preview.plan_id,
        status: PlanValidationStatus::Valid,
        detail:
            "The retained file content, metadata, and current path binding still match this plan."
                .into(),
    }
}

fn prepare_integrity_error(error: IntegrityError) -> String {
    match error {
        IntegrityError::Cancelled => {
            "Compression planning was superseded by a newer request.".into()
        }
        IntegrityError::Changed => {
            "The file changed while its content was being anchored; rescan before planning.".into()
        }
        IntegrityError::Io(error) => {
            format!("The file content could not be read safely for planning: {error}.")
        }
    }
}

fn capture_content_digest(
    file: &File,
    expected: &FileRevision,
    cancel: &AtomicBool,
) -> Result<ContentDigest, IntegrityError> {
    if cancel.load(Ordering::Acquire) {
        return Err(IntegrityError::Cancelled);
    }
    let before = snapshot_anchor_revision(file).map_err(IntegrityError::Io)?;
    if &before != expected {
        return Err(IntegrityError::Changed);
    }
    let digest = hash_open_file(file, expected.logical_bytes, cancel)?;
    let after = snapshot_anchor_revision(file).map_err(IntegrityError::Io)?;
    if after != before {
        return Err(IntegrityError::Changed);
    }
    if cancel.load(Ordering::Acquire) {
        return Err(IntegrityError::Cancelled);
    }
    Ok(digest)
}

fn hash_open_file(
    file: &File,
    logical_bytes: u64,
    cancel: &AtomicBool,
) -> Result<ContentDigest, IntegrityError> {
    hash_open_file_observed(file, logical_bytes, cancel, |_| {})
}

fn hash_open_file_observed(
    file: &File,
    logical_bytes: u64,
    cancel: &AtomicBool,
    mut on_chunk: impl FnMut(u64),
) -> Result<ContentDigest, IntegrityError> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; CONTENT_HASH_CHUNK_BYTES];
    let mut offset = 0_u64;
    while offset < logical_bytes {
        if cancel.load(Ordering::Acquire) {
            return Err(IntegrityError::Cancelled);
        }
        let remaining = logical_bytes - offset;
        let wanted = usize::try_from(remaining.min(CONTENT_HASH_CHUNK_BYTES as u64))
            .unwrap_or(CONTENT_HASH_CHUNK_BYTES);
        let read = loop {
            match read_at(file, &mut buffer[..wanted], offset) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result.map_err(IntegrityError::Io)?,
            }
        };
        if read == 0 {
            return Err(IntegrityError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the file ended while its content was being read",
            )));
        }
        hasher.update(&buffer[..read]);
        offset = offset.saturating_add(read as u64);
        on_chunk(offset);
    }
    Ok(ContentDigest(*hasher.finalize().as_bytes()))
}

#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.read_at(buffer, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;

    file.seek_read(buffer, offset)
}

#[cfg(not(any(unix, windows)))]
fn read_at(_file: &File, _buffer: &mut [u8], _offset: u64) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "positioned file reads are unavailable on this platform",
    ))
}

impl From<file_revision::FileSnapshot> for FileRevision {
    fn from(snapshot: file_revision::FileSnapshot) -> Self {
        Self {
            scanned: snapshot.scanned,
            logical_bytes: snapshot.logical_bytes,
            allocated_bytes: snapshot.allocated_bytes,
            link_count: snapshot.link_count,
        }
    }
}

fn snapshot_revision(path: &Path) -> io::Result<FileRevision> {
    file_revision::snapshot_no_follow(path).map(FileRevision::from)
}

fn snapshot_anchor_revision(file: &File) -> io::Result<FileRevision> {
    file_revision::snapshot_open_file(file).map(FileRevision::from)
}

fn validate_anchor_binding(
    path: &Path,
    identity_anchor: &File,
    expected: &FileRevision,
) -> Result<(), String> {
    let anchored = snapshot_anchor_revision(identity_anchor).map_err(|error| {
        format!("The retained file could not be revalidated during planning: {error}.")
    })?;
    if &anchored != expected {
        return Err("The file changed while its compression plan was being prepared; rescan before planning.".into());
    }
    let rebound = snapshot_revision(path).map_err(|error| {
        format!("The file path could not be rebound safely during planning: {error}.")
    })?;
    if rebound != anchored {
        return Err("The file path changed while its compression plan was being prepared; rescan before planning.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn prepare_test(
        plan_id: u64,
        scan_id: u64,
        node_id: u64,
        target: CompressionTarget,
        operation: CompressionOperation,
    ) -> Result<PreparedCompressionPlan, String> {
        prepare(
            plan_id,
            scan_id,
            node_id,
            target,
            operation,
            &AtomicBool::new(false),
        )
    }

    fn revalidate_test(plan: &PreparedCompressionPlan) -> PlanValidation {
        revalidate(plan, &AtomicBool::new(false))
    }

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

        let plan = prepare_test(3, 5, 7, target(&path), CompressionOperation::Compress)
            .expect("prepare plan");
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
        let plan = prepare_test(1, 1, 1, target(&path), CompressionOperation::Compress)
            .expect("prepare plan");

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
        let validation = revalidate_test(&plan);

        assert_eq!(validation.status, PlanValidationStatus::Changed);
    }

    #[test]
    fn revalidation_detects_content_changes_when_metadata_revisions_collide() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, b"before").expect("write fixture");
        let mut plan = prepare_test(1, 1, 1, target(&path), CompressionOperation::Compress)
            .expect("prepare plan");

        fs::write(&path, b"after!").expect("rewrite fixture at the same length");
        plan.revision = snapshot_revision(&path).expect("snapshot rewritten fixture");
        let validation = revalidate_test(&plan);

        assert_eq!(validation.status, PlanValidationStatus::Changed);
        assert!(validation.detail.contains("content changed"));
    }

    #[test]
    fn preparation_honors_cancellation_before_reading_content() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, vec![7_u8; CONTENT_HASH_CHUNK_BYTES * 2]).expect("write fixture");
        let cancel = AtomicBool::new(true);

        let error = prepare(
            1,
            1,
            1,
            target(&path),
            CompressionOperation::Compress,
            &cancel,
        )
        .expect_err("cancelled preparation must stop before hashing");

        assert!(error.contains("superseded"));
    }

    #[test]
    fn content_hashing_honors_cancellation_between_bounded_chunks() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        let logical_bytes = CONTENT_HASH_CHUNK_BYTES * 2;
        fs::write(&path, vec![7_u8; logical_bytes]).expect("write fixture");
        let opened =
            file_revision::open_content_snapshot_no_follow(&path).expect("open fixture safely");
        let cancel = AtomicBool::new(false);
        let result = hash_open_file_observed(&opened.file, logical_bytes as u64, &cancel, |_| {
            cancel.store(true, Ordering::Release)
        });

        assert!(matches!(result, Err(IntegrityError::Cancelled)));
    }

    #[test]
    fn positioned_content_hashing_preserves_the_anchor_cursor() {
        use std::io::Seek;

        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        let content = vec![9_u8; CONTENT_HASH_CHUNK_BYTES + 7];
        fs::write(&path, &content).expect("write fixture");
        let mut opened =
            file_revision::open_content_snapshot_no_follow(&path).expect("open fixture safely");
        let before = opened
            .file
            .stream_position()
            .expect("read initial anchor cursor");

        let digest = capture_content_digest(
            &opened.file,
            &FileRevision::from(opened.snapshot),
            &AtomicBool::new(false),
        )
        .expect("hash fixture through positioned reads");

        assert_eq!(digest, ContentDigest(*blake3::hash(&content).as_bytes()));
        assert_eq!(
            opened
                .file
                .stream_position()
                .expect("read final anchor cursor"),
            before
        );
    }

    #[test]
    fn revalidation_rejects_a_regular_file_replacement_after_planning() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        let original = temp.path().join("original.bin");
        fs::write(&path, b"before").expect("write fixture");
        let plan = prepare_test(1, 1, 1, target(&path), CompressionOperation::Compress)
            .expect("prepare plan");

        fs::rename(&path, &original).expect("move planned file aside");
        fs::write(&path, b"before").expect("replace path with same-length content");

        snapshot_anchor_revision(&plan.identity_anchor)
            .expect("path replacement must not close the retained identity anchor");
        let validation = revalidate_test(&plan);
        assert_eq!(validation.status, PlanValidationStatus::Changed);
        assert!(validation.detail.contains("different file"));
    }

    #[test]
    fn revalidation_keeps_the_anchor_but_rejects_an_unlinked_path() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, b"retained").expect("write fixture");
        let plan = prepare_test(1, 1, 1, target(&path), CompressionOperation::Compress)
            .expect("prepare plan");

        fs::remove_file(&path).expect("unlink planned file");

        snapshot_anchor_revision(&plan.identity_anchor)
            .expect("the active plan must keep its unlinked identity anchor alive");
        assert_eq!(
            revalidate_test(&plan).status,
            PlanValidationStatus::Unavailable
        );
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

        let error = prepare_test(1, 1, 1, stale, CompressionOperation::Compress)
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

        let error = prepare_test(1, 1, 1, stale, CompressionOperation::Compress)
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
        let plan = prepare_test(1, 1, 1, target(&path), CompressionOperation::Compress)
            .expect("prepare plan");
        fs::remove_file(&path).expect("remove planned file");
        symlink(&replacement, &path).expect("replace with symlink");

        let validation = revalidate_test(&plan);

        assert_eq!(validation.status, PlanValidationStatus::Unavailable);
    }

    #[test]
    fn preparation_rejects_scan_size_drift() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, b"changed").expect("write fixture");
        let mut stale = target(&path);
        stale.logical_bytes = stale.logical_bytes.saturating_add(1);

        let error = prepare_test(1, 1, 1, stale, CompressionOperation::Compress)
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

        let error = prepare_test(1, 1, 1, target, CompressionOperation::Compress)
            .expect_err("missing scan identity must fail");

        assert!(error.contains("could not retain a stable file identity"));
    }
}
