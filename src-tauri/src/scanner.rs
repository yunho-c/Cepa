use jwalk::{Parallelism, WalkDirGeneric};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::Metadata;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use crate::file_revision::CompactScannedFileRevision;
use crate::file_revision::ScannedFileRevision;

#[cfg(unix)]
type RetainedScanRevision = CompactScannedFileRevision;
#[cfg(not(unix))]
type RetainedScanRevision = ScannedFileRevision;

#[cfg(target_os = "linux")]
#[path = "scanner/linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[path = "scanner/local_only.rs"]
mod local_only;
#[cfg(target_os = "macos")]
#[path = "scanner/macos.rs"]
mod macos;
#[cfg(any(target_os = "windows", test))]
#[path = "scanner/mft.rs"]
mod mft;
#[cfg(target_os = "windows")]
#[path = "scanner/windows.rs"]
mod windows;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const PROGRESS_CLOCK_CHECK_INTERVAL_ENTRIES: u8 = 32;
const CANCELLATION_CHECK_INTERVAL_ENTRIES: u64 = 2_048;
pub(crate) const AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES: usize =
    CANCELLATION_CHECK_INTERVAL_ENTRIES as usize;
const MAX_PARTIAL_ITEMS: usize = 8;
const MAX_LIST_ITEMS: usize = 500;
const MAX_CHART_ITEMS_PER_DIRECTORY: usize = 16;
const MAX_CHART_ITEMS_TOTAL: usize = 512;
const MAX_CHART_DEPTH: usize = 3;
const MAX_SEARCH_QUERY_CHARS: usize = 128;
const RANKING_BUFFER_MULTIPLIER: usize = 16;
const MAX_RANKING_CLONE_BYTES: usize = 2 * 1024 * 1024;
const MAX_RANKING_CLONE_IDS: usize = MAX_RANKING_CLONE_BYTES / std::mem::size_of::<NodeId>();

struct TraversalProgressClock {
    last_update_at: Instant,
    last_check_at: Instant,
    entries_in_check_window: u8,
    entries_until_check: u8,
}

impl TraversalProgressClock {
    fn new(now: Instant) -> Self {
        Self {
            last_update_at: now,
            last_check_at: now,
            // Check after the first entry so a single slow metadata operation
            // can still publish promptly, then amortize clock reads in steady state.
            entries_in_check_window: 1,
            entries_until_check: 1,
        }
    }

    fn should_check_clock(&mut self) -> bool {
        debug_assert_ne!(self.entries_until_check, 0);
        self.entries_until_check -= 1;
        self.entries_until_check == 0
    }

    fn update_due_at(&mut self, now: Instant) -> bool {
        let elapsed_since_update = now.saturating_duration_since(self.last_update_at);
        let elapsed_since_check = now.saturating_duration_since(self.last_check_at);
        let update_due = elapsed_since_update >= PROGRESS_INTERVAL;
        let remaining = if update_due {
            PROGRESS_INTERVAL
        } else {
            PROGRESS_INTERVAL.saturating_sub(elapsed_since_update)
        };
        let next_window = progress_clock_check_window(
            remaining,
            elapsed_since_check,
            self.entries_in_check_window,
        );
        self.last_check_at = now;
        self.entries_in_check_window = next_window;
        self.entries_until_check = next_window;
        update_due
    }

    fn mark_updated(&mut self, now: Instant) {
        self.last_update_at = now;
    }
}

fn progress_clock_check_window(
    remaining: Duration,
    sampled_duration: Duration,
    sampled_entries: u8,
) -> u8 {
    let sampled_nanos = sampled_duration.as_nanos();
    if sampled_nanos == 0 {
        return PROGRESS_CLOCK_CHECK_INTERVAL_ENTRIES;
    }
    let numerator = remaining
        .as_nanos()
        .saturating_mul(u128::from(sampled_entries));
    let estimated_entries = numerator.saturating_add(sampled_nanos - 1) / sampled_nanos;
    estimated_entries.clamp(1, u128::from(PROGRESS_CLOCK_CHECK_INTERVAL_ENTRIES)) as u8
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: ScanPhase,
    pub entries_scanned: u64,
    pub files_scanned: u64,
    pub directories_scanned: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub skipped_entries: u64,
    pub current_path: String,
    pub elapsed_ms: u64,
    pub largest_items: Vec<ScanItem>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Scanning,
    Finishing,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanItem {
    pub id: u64,
    pub name: String,
    pub kind: EntryKind,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub file_count: u64,
    pub directory_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breadcrumb {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartItem {
    pub id: Option<u64>,
    pub name: String,
    pub kind: EntryKind,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub children: Vec<ChartItem>,
}

pub(crate) fn chart_item_count(item: &ChartItem) -> usize {
    1 + item.children.iter().map(chart_item_count).sum::<usize>()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryView {
    pub scan_id: u64,
    pub node_id: u64,
    pub root: String,
    pub path: String,
    pub display_name: String,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub total_items: usize,
    pub suppressed_items: usize,
    pub items_truncated: bool,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub items: Vec<ScanItem>,
    pub chart_items: Vec<ChartItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectorySearchResult {
    pub scan_id: u64,
    pub node_id: u64,
    pub query: String,
    pub metric: SizeMetric,
    pub total_matches: usize,
    pub items_truncated: bool,
    pub items: Vec<ScanItem>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SizeMetric {
    Allocated,
    Logical,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub root: String,
    pub display_name: String,
    pub backend: &'static str,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub file_count: u64,
    pub directory_count: u64,
    pub skipped_entries: u64,
    pub skipped_filesystems: u64,
    pub skipped_cloud_entries: u64,
    pub duplicate_hard_links: u64,
    pub traversal_us: u64,
    pub aggregation_us: u64,
    pub indexing_us: u64,
    pub elapsed_ms: u64,
    pub allocated_size_is_estimate: bool,
    pub hard_link_deduplication_supported: bool,
    pub same_filesystem_enforced: bool,
}

impl ScanResult {
    /// Returns the correctness-relevant fields that differ, excluding backend
    /// identity and timing measurements.
    pub fn accounting_mismatches(&self, other: &Self) -> Vec<&'static str> {
        let mut mismatches = Vec::new();
        macro_rules! compare {
            ($field:ident) => {
                if self.$field != other.$field {
                    mismatches.push(stringify!($field));
                }
            };
        }

        compare!(root);
        compare!(display_name);
        compare!(logical_bytes);
        compare!(allocated_bytes);
        compare!(file_count);
        compare!(directory_count);
        compare!(skipped_entries);
        compare!(skipped_filesystems);
        compare!(skipped_cloud_entries);
        compare!(duplicate_hard_links);
        compare!(allocated_size_is_estimate);
        compare!(hard_link_deduplication_supported);
        compare!(same_filesystem_enforced);
        mismatches
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanBackend {
    Auto,
    Jwalk,
    Getattrlistbulk,
    Mft,
    Statx,
}

impl ScanBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Jwalk => "jwalk",
            Self::Getattrlistbulk => "getattrlistbulk",
            Self::Mft => "mft",
            Self::Statx => "statx",
        }
    }
}

impl fmt::Display for ScanBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ScanBackend {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "jwalk" => Ok(Self::Jwalk),
            "getattrlistbulk" => Ok(Self::Getattrlistbulk),
            "mft" => Ok(Self::Mft),
            "statx" => Ok(Self::Statx),
            value => Err(format!(
                "unknown backend {value:?}; expected auto, jwalk, getattrlistbulk, mft, or statx"
            )),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ScanOutput {
    pub result: ScanResult,
    pub snapshot: ScanSnapshot,
}

impl ScanOutput {
    pub(crate) fn set_root_display_name(&mut self, name: String) {
        self.result.display_name = name.clone();
        self.snapshot.nodes[self.snapshot.root].name = name.into();
    }
}

#[derive(Debug)]
pub(crate) struct ScanSnapshot {
    root: NodeId,
    root_path: Arc<Path>,
    root_is_macos_volume: bool,
    nodes: Vec<InternalNode>,
    allocated_size_is_estimate: bool,
    #[cfg(unix)]
    revision_filesystem_id: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct CompressionTarget {
    pub path: PathBuf,
    pub kind: EntryKind,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub allocated_size_is_estimate: bool,
    pub scan_revision: Option<ScannedFileRevision>,
}

type NodeId = usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
struct ParentId(NonZeroUsize);

impl ParentId {
    fn new(node_id: NodeId) -> Self {
        let encoded = node_id
            .checked_add(1)
            .and_then(NonZeroUsize::new)
            .expect("scan node IDs fit in the compact parent representation");
        Self(encoded)
    }

    fn node_id(self) -> NodeId {
        self.0.get() - 1
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MeasuredMetadata {
    logical_bytes: u64,
    allocated_bytes: u64,
    filesystem_id: Option<u64>,
    file_identity: Option<FileIdentity>,
    scan_revision: Option<ScannedFileRevision>,
    metadata_error: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct FileIdentity(u64, u64);

#[derive(Clone, Copy, Debug)]
struct HardLinkOwner {
    node_id: NodeId,
    logical_bytes: u64,
    allocated_bytes: u64,
}

#[derive(Debug, Default)]
struct ScanCounters {
    files_scanned: u64,
    directories_scanned: u64,
    skipped_entries: u64,
    skipped_filesystems: u64,
    skipped_cloud_entries: u64,
    duplicate_hard_links: u64,
    observed_logical_bytes: u64,
    observed_allocated_bytes: u64,
    hard_link_owners: HashMap<FileIdentity, HardLinkOwner>,
    hard_link_path_left: Vec<NodeId>,
    hard_link_path_right: Vec<NodeId>,
    #[cfg(unix)]
    revision_filesystem_id: Option<u64>,
}

#[derive(Debug, Default)]
struct PartialRanking {
    candidates: Vec<PartialCandidate>,
    worst_index: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct PartialCandidate {
    node_id: NodeId,
    logical_bytes: u64,
    allocated_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
struct ScanSemantics {
    allocated_size_is_estimate: bool,
    hard_link_deduplication_supported: bool,
    same_filesystem_enforced: bool,
}

#[derive(Debug)]
struct InternalNode {
    name: OsString,
    parent: Option<ParentId>,
    children: Vec<NodeId>,
    kind: EntryKind,
    logical_bytes: u64,
    allocated_bytes: u64,
    file_count: u64,
    directory_count: u64,
    scan_revision: Option<RetainedScanRevision>,
}

impl InternalNode {
    fn root(path: &Path) -> Self {
        Self {
            name: path.file_name().unwrap_or(path.as_os_str()).to_owned(),
            parent: None,
            children: Vec::new(),
            kind: EntryKind::Directory,
            logical_bytes: 0,
            allocated_bytes: 0,
            file_count: 0,
            directory_count: 1,
            scan_revision: None,
        }
    }

    fn name(&self) -> &OsStr {
        &self.name
    }

    fn parent_id(&self) -> Option<NodeId> {
        self.parent.map(ParentId::node_id)
    }
}

impl ScanCounters {
    fn retain_scan_revision(
        &mut self,
        revision: Option<ScannedFileRevision>,
    ) -> Option<RetainedScanRevision> {
        #[cfg(unix)]
        {
            let (filesystem_id, compact) = revision?.compact();
            match self.revision_filesystem_id {
                Some(retained_id) if retained_id != filesystem_id => None,
                Some(_) => Some(compact),
                None => {
                    self.revision_filesystem_id = Some(filesystem_id);
                    Some(compact)
                }
            }
        }
        #[cfg(not(unix))]
        {
            revision
        }
    }

    fn push_node(
        &mut self,
        nodes: &mut Vec<InternalNode>,
        parent: NodeId,
        name: OsString,
        kind: EntryKind,
        measured: MeasuredMetadata,
    ) -> (NodeId, Option<NodeId>) {
        let (mut logical_bytes, mut allocated_bytes, file_count, directory_count) = match kind {
            EntryKind::Directory => {
                self.directories_scanned += 1;
                (0, 0, 0, 1)
            }
            EntryKind::File => {
                self.files_scanned += 1;
                (measured.logical_bytes, measured.allocated_bytes, 1, 0)
            }
            EntryKind::Symlink | EntryKind::Other => (0, 0, 0, 0),
        };

        let previous_owner = measured
            .file_identity
            .and_then(|identity| self.hard_link_owners.get(&identity).copied());
        if previous_owner.is_some() {
            self.duplicate_hard_links += 1;
            logical_bytes = 0;
            allocated_bytes = 0;
        }

        self.observed_logical_bytes = self.observed_logical_bytes.saturating_add(logical_bytes);
        self.observed_allocated_bytes = self
            .observed_allocated_bytes
            .saturating_add(allocated_bytes);

        let scan_revision = self.retain_scan_revision(measured.scan_revision);
        let node_id = nodes.len();
        nodes[parent].children.push(node_id);
        nodes.push(InternalNode {
            name,
            parent: Some(ParentId::new(parent)),
            children: Vec::new(),
            kind,
            logical_bytes,
            allocated_bytes,
            file_count,
            directory_count,
            scan_revision,
        });

        let mut replaced_owner = None;
        if let Some(identity) = measured.file_identity {
            if let Some(owner) = previous_owner {
                if compare_relative_node_paths(
                    nodes,
                    node_id,
                    owner.node_id,
                    &mut self.hard_link_path_left,
                    &mut self.hard_link_path_right,
                )
                .is_lt()
                {
                    nodes[owner.node_id].logical_bytes = 0;
                    nodes[owner.node_id].allocated_bytes = 0;
                    nodes[node_id].logical_bytes = measured.logical_bytes;
                    nodes[node_id].allocated_bytes = measured.allocated_bytes;
                    self.observed_logical_bytes = self
                        .observed_logical_bytes
                        .saturating_sub(owner.logical_bytes)
                        .saturating_add(measured.logical_bytes);
                    self.observed_allocated_bytes = self
                        .observed_allocated_bytes
                        .saturating_sub(owner.allocated_bytes)
                        .saturating_add(measured.allocated_bytes);
                    self.hard_link_owners.insert(
                        identity,
                        HardLinkOwner {
                            node_id,
                            logical_bytes: measured.logical_bytes,
                            allocated_bytes: measured.allocated_bytes,
                        },
                    );
                    replaced_owner = Some(owner.node_id);
                }
            } else {
                self.hard_link_owners.insert(
                    identity,
                    HardLinkOwner {
                        node_id,
                        logical_bytes: measured.logical_bytes,
                        allocated_bytes: measured.allocated_bytes,
                    },
                );
            }
        }
        (node_id, replaced_owner)
    }
}

impl PartialRanking {
    #[inline]
    fn observe(&mut self, candidate: PartialCandidate) {
        if self.candidates.len() < MAX_PARTIAL_ITEMS {
            self.candidates.push(candidate);
            if self.candidates.len() == MAX_PARTIAL_ITEMS {
                self.refresh_worst();
            }
            return;
        }

        let worst_index = self
            .worst_index
            .expect("a full partial ranking has a worst item");
        if compare_partial_candidates(&candidate, &self.candidates[worst_index]).is_lt() {
            self.candidates[worst_index] = candidate;
            self.refresh_worst();
        }
    }

    fn refresh_worst(&mut self) {
        self.worst_index = self
            .candidates
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| compare_partial_candidates(left, right))
            .map(|(index, _)| index);
    }

    fn replace(&mut self, previous_node_id: NodeId, candidate: PartialCandidate) -> bool {
        let Some(index) = self
            .candidates
            .iter()
            .position(|current| current.node_id == previous_node_id)
        else {
            return false;
        };
        self.candidates[index] = candidate;
        if self.candidates.len() == MAX_PARTIAL_ITEMS {
            self.refresh_worst();
        }
        true
    }

    fn items(&self, nodes: &[InternalNode]) -> Vec<ScanItem> {
        let mut ranked = self.candidates.clone();
        ranked.sort_unstable_by(compare_partial_candidates);
        ranked
            .into_iter()
            .map(|candidate| scan_item(nodes, candidate.node_id))
            .collect()
    }
}

pub(crate) fn scan_path<F>(
    root: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: F,
) -> Result<ScanOutput, String>
where
    F: FnMut(ScanProgress),
{
    scan_path_with_backend(root, cancel, ScanBackend::Auto, on_progress)
}

pub(crate) fn validate_scan_root(root: &Path) -> Result<(PathBuf, Metadata), String> {
    #[cfg(target_os = "macos")]
    let _local_only = local_only::NoMaterialization::new().map_err(local_only::protection_error)?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("Could not open {}: {error}", root.display()))?;
    let metadata = root
        .metadata()
        .map_err(|error| format!("Could not read {}: {error}", root.display()))?;
    if !metadata.is_dir() {
        return Err("Choose a directory to scan.".to_string());
    }
    #[cfg(target_os = "macos")]
    if local_only::is_dataless(&metadata) {
        return Err(
            "This folder is stored in the cloud. Choose a folder available on this device."
                .to_string(),
        );
    }
    Ok((root, metadata))
}

pub(crate) fn scan_path_with_backend<F>(
    root: &Path,
    cancel: Arc<AtomicBool>,
    backend: ScanBackend,
    mut on_progress: F,
) -> Result<ScanOutput, String>
where
    F: FnMut(ScanProgress),
{
    #[cfg(target_os = "macos")]
    let _local_only = local_only::NoMaterialization::new().map_err(local_only::protection_error)?;
    match backend {
        ScanBackend::Auto => {
            #[cfg(target_os = "macos")]
            {
                match macos::scan_path(root, cancel.clone(), &mut on_progress) {
                    Ok(output) => return Ok(output),
                    Err(macos::NativeScanError::Unavailable) => {}
                    Err(macos::NativeScanError::Fatal(error)) => return Err(error),
                }
            }
            #[cfg(target_os = "linux")]
            {
                scan_path_auto_linux(root, cancel, &mut on_progress, |root, cancel, progress| {
                    linux::scan_path(root, cancel, progress)
                })
            }
            #[cfg(target_os = "windows")]
            {
                match windows::scan_path(root, cancel.clone(), &mut on_progress) {
                    Ok(output) => Ok(output),
                    Err(windows::NativeScanError::Unavailable) => {
                        scan_path_jwalk(root, cancel, on_progress)
                    }
                    Err(windows::NativeScanError::Fatal(error)) => Err(error),
                }
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                scan_path_jwalk(root, cancel, on_progress)
            }
        }
        ScanBackend::Jwalk => scan_path_jwalk(root, cancel, on_progress),
        ScanBackend::Getattrlistbulk => {
            #[cfg(target_os = "macos")]
            {
                macos::scan_path(root, cancel, &mut on_progress).map_err(|error| match error {
                    macos::NativeScanError::Unavailable => {
                        "getattrlistbulk is unavailable for this filesystem.".to_string()
                    }
                    macos::NativeScanError::Fatal(error) => error,
                })
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (root, cancel, on_progress);
                Err("getattrlistbulk is only available on macOS.".to_string())
            }
        }
        ScanBackend::Mft => {
            #[cfg(target_os = "windows")]
            {
                windows::scan_path(root, cancel, &mut on_progress).map_err(|error| match error {
                    windows::NativeScanError::Unavailable => {
                        "MFT traversal is unavailable for this Windows volume or process."
                            .to_string()
                    }
                    windows::NativeScanError::Fatal(error) => error,
                })
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = (root, cancel, on_progress);
                Err("MFT traversal is only available on Windows.".to_string())
            }
        }
        ScanBackend::Statx => {
            #[cfg(target_os = "linux")]
            {
                linux::scan_path(root, cancel, &mut on_progress).map_err(|error| match error {
                    linux::NativeScanError::Unavailable => {
                        "statx is unavailable on this Linux kernel or runtime.".to_string()
                    }
                    linux::NativeScanError::Fatal(error) => error,
                })
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (root, cancel, on_progress);
                Err("statx is only available on Linux.".to_string())
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn scan_path_auto_linux<F, N>(
    root: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: &mut F,
    native: N,
) -> Result<ScanOutput, String>
where
    F: FnMut(ScanProgress),
    N: FnOnce(&Path, Arc<AtomicBool>, &mut F) -> Result<ScanOutput, linux::NativeScanError>,
{
    match native(root, cancel.clone(), on_progress) {
        Ok(output) => Ok(output),
        Err(linux::NativeScanError::Unavailable) => scan_path_jwalk(root, cancel, on_progress),
        Err(linux::NativeScanError::Fatal(error)) => Err(error),
    }
}

#[derive(Clone, Copy, Debug)]
enum WalkMetadata {
    Local(MeasuredMetadata),
    #[cfg(target_os = "macos")]
    Cloud,
}

fn scan_path_jwalk<F>(
    root: &Path,
    cancel: Arc<AtomicBool>,
    mut on_progress: F,
) -> Result<ScanOutput, String>
where
    F: FnMut(ScanProgress),
{
    #[cfg(target_os = "macos")]
    let _local_only = local_only::NoMaterialization::new().map_err(local_only::protection_error)?;
    let (root, root_metadata) = validate_scan_root(root)?;

    if cancel.load(Ordering::Relaxed) {
        return Err("Scan cancelled.".to_string());
    }

    let started_at = Instant::now();
    let root_filesystem = filesystem_id(&root_metadata);
    let allocated_size_is_estimate = portable_allocated_size_is_estimate(&root);
    let root_node_path: Arc<Path> = Arc::from(root.clone());
    let mut nodes = vec![InternalNode::root(&root)];
    // jwalk reads directories in parallel but its public iterator yields them
    // depth-first. The stack therefore maps entry depth to a compact parent ID
    // without retaining a full-path lookup table.
    let mut ancestor_stack = vec![0];

    let mut counters = ScanCounters::default();
    let mut partial_ranking = PartialRanking::default();
    let mut progress_clock = TraversalProgressClock::new(Instant::now());

    let worker_cancel = cancel.clone();
    let threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .clamp(2, 32);

    #[cfg(target_os = "macos")]
    let parallelism = Parallelism::RayonExistingPool {
        pool: local_only::worker_pool(threads)?,
        busy_timeout: None,
    };
    #[cfg(not(target_os = "macos"))]
    let parallelism = Parallelism::RayonNewPool(threads);

    let walker = WalkDirGeneric::<((), Option<WalkMetadata>)>::new(&root)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(parallelism)
        .process_read_dir(move |_, _, _, children| {
            if worker_cancel.load(Ordering::Relaxed) {
                children.clear();
                return;
            }

            for child in children
                .iter_mut()
                .filter_map(|result| result.as_mut().ok())
            {
                match child.metadata() {
                    Ok(metadata) => {
                        #[cfg(target_os = "macos")]
                        if local_only::is_dataless(&metadata) {
                            child.read_children_path = None;
                            child.client_state = Some(WalkMetadata::Cloud);
                            continue;
                        }
                        let measured = measure_metadata(&metadata, child.file_type.is_file());
                        #[cfg(windows)]
                        let measured = if child.file_type.is_file() {
                            MeasuredMetadata {
                                scan_revision: crate::file_revision::snapshot_no_follow(
                                    &child.path(),
                                )
                                .ok()
                                .map(|snapshot| snapshot.scanned),
                                ..measured
                            }
                        } else {
                            measured
                        };
                        if child.file_type.is_dir()
                            && root_filesystem.is_some()
                            && measured.filesystem_id != root_filesystem
                        {
                            child.read_children_path = None;
                        }
                        child.client_state = Some(WalkMetadata::Local(measured));
                    }
                    #[cfg(target_os = "macos")]
                    Err(error)
                        if error
                            .io_error()
                            .is_some_and(local_only::materialization_blocked) =>
                    {
                        child.read_children_path = None;
                        child.client_state = Some(WalkMetadata::Cloud);
                    }
                    Err(_) if child.file_type.is_dir() => {
                        child.read_children_path = None;
                        child.client_state = Some(WalkMetadata::Local(MeasuredMetadata {
                            metadata_error: true,
                            ..MeasuredMetadata::default()
                        }));
                    }
                    Err(_) => child.client_state = None,
                }
            }
        });

    for entry_result in walker {
        if cancel.load(Ordering::Relaxed) {
            return Err("Scan cancelled.".to_string());
        }

        let entry = match entry_result {
            Ok(entry) => entry,
            #[cfg(target_os = "macos")]
            Err(error)
                if error
                    .io_error()
                    .is_some_and(local_only::materialization_blocked) =>
            {
                if error.depth() == 0 {
                    return Err(
                        "The selected folder requires a cloud download to scan.".to_string()
                    );
                }
                counters.skipped_cloud_entries += 1;
                continue;
            }
            Err(_) => {
                counters.skipped_entries += 1;
                continue;
            }
        };

        #[cfg(target_os = "macos")]
        if entry
            .read_children_error
            .as_ref()
            .and_then(jwalk::Error::io_error)
            .is_some_and(local_only::materialization_blocked)
        {
            if entry.depth == 0 {
                return Err("The selected folder requires a cloud download to scan.".to_string());
            }
            counters.skipped_cloud_entries += 1;
            continue;
        }

        if entry.depth == 0 {
            #[cfg(target_os = "macos")]
            if matches!(entry.client_state, Some(WalkMetadata::Cloud)) {
                return Err("The selected folder requires a cloud download to scan.".to_string());
            }
            if entry.read_children_error.is_some() {
                counters.skipped_entries += 1;
            }
            continue;
        }

        let measured = match entry.client_state {
            Some(WalkMetadata::Local(measured)) => measured,
            #[cfg(target_os = "macos")]
            Some(WalkMetadata::Cloud) => {
                counters.skipped_cloud_entries += 1;
                continue;
            }
            None => {
                counters.skipped_entries += 1;
                continue;
            }
        };

        if measured.metadata_error {
            counters.skipped_entries += 1;
        }

        let kind = if entry.file_type.is_dir() {
            EntryKind::Directory
        } else if entry.path_is_symlink() {
            EntryKind::Symlink
        } else if entry.file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };

        let is_other_filesystem = entry.file_type.is_dir()
            && matches!(
                (root_filesystem, measured.filesystem_id),
                (Some(root_id), Some(entry_id)) if root_id != entry_id
            );

        if is_other_filesystem {
            counters.skipped_filesystems += 1;
        }

        if entry.read_children_error.is_some() {
            counters.skipped_entries += 1;
        }

        ancestor_stack.truncate(entry.depth);
        let parent = ancestor_stack
            .get(entry.depth.saturating_sub(1))
            .copied()
            .ok_or_else(|| {
                format!(
                    "The scanner received {} without its depth-first parent.",
                    entry.path().display()
                )
            })?;
        let progress_now = progress_clock
            .should_check_clock()
            .then(Instant::now)
            .filter(|now| progress_clock.update_due_at(*now));
        let current_path = progress_now.map(|_| entry.path().to_string_lossy().into_owned());

        let (node_id, replaced_owner) =
            counters.push_node(&mut nodes, parent, entry.file_name, kind, measured);
        observe_partial_file(&mut partial_ranking, &nodes, node_id, replaced_owner);
        debug_assert_eq!(node_id, nodes.len() - 1);
        if matches!(kind, EntryKind::Directory) {
            ancestor_stack.push(node_id);
        }

        if let (Some(progress_now), Some(current_path)) = (progress_now, current_path) {
            on_progress(ScanProgress {
                phase: ScanPhase::Scanning,
                entries_scanned: counters.files_scanned + counters.directories_scanned,
                files_scanned: counters.files_scanned,
                directories_scanned: counters.directories_scanned,
                logical_bytes: counters.observed_logical_bytes,
                allocated_bytes: counters.observed_allocated_bytes,
                skipped_entries: counters.skipped_entries,
                current_path,
                elapsed_ms: elapsed_ms(started_at),
                largest_items: partial_ranking.items(&nodes),
            });
            progress_clock.mark_updated(progress_now);
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return Err("Scan cancelled.".to_string());
    }

    let traversal_completed_at = Instant::now();
    finish_scan(
        root,
        root_node_path,
        nodes,
        counters,
        partial_ranking,
        "jwalk",
        ScanSemantics {
            allocated_size_is_estimate,
            hard_link_deduplication_supported: cfg!(unix),
            same_filesystem_enforced: root_filesystem.is_some(),
        },
        started_at,
        traversal_completed_at,
        &cancel,
        &mut on_progress,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_scan<F>(
    root: PathBuf,
    root_node_path: Arc<Path>,
    mut nodes: Vec<InternalNode>,
    counters: ScanCounters,
    partial_ranking: PartialRanking,
    backend: &'static str,
    semantics: ScanSemantics,
    started_at: Instant,
    traversal_completed_at: Instant,
    cancel: &AtomicBool,
    on_progress: &mut F,
) -> Result<ScanOutput, String>
where
    F: FnMut(ScanProgress),
{
    let traversal_us = duration_us(traversal_completed_at.duration_since(started_at));

    let mut finishing_progress = ScanProgress {
        phase: ScanPhase::Finishing,
        entries_scanned: counters.files_scanned + counters.directories_scanned,
        files_scanned: counters.files_scanned,
        directories_scanned: counters.directories_scanned,
        logical_bytes: counters.observed_logical_bytes,
        allocated_bytes: counters.observed_allocated_bytes,
        skipped_entries: counters.skipped_entries,
        current_path: root.to_string_lossy().into_owned(),
        elapsed_ms: duration_ms(traversal_completed_at.duration_since(started_at)),
        largest_items: partial_ranking.items(&nodes),
    };
    on_progress(finishing_progress.clone());
    let mut last_progress_at = Instant::now();

    if let Err(error) = aggregate_nodes(&mut nodes, cancel, |_| {
        report_finishing_progress_if_due(
            &mut finishing_progress,
            started_at,
            &mut last_progress_at,
            Instant::now(),
            on_progress,
        );
    }) {
        if error == "Scan cancelled." {
            let _ = release_nodes_off_thread(nodes, || {});
        }
        return Err(error);
    }

    let aggregation_completed_at = Instant::now();
    let aggregation_us =
        duration_us(aggregation_completed_at.duration_since(traversal_completed_at));
    // Capture this once on the protected scan worker. View construction must
    // remain snapshot-only and never re-query the filesystem or discover disks.
    let root_is_macos_volume = is_macos_volume_root(&root);
    let indexing_completed_at = Instant::now();
    let indexing_us = duration_us(indexing_completed_at.duration_since(aggregation_completed_at));

    let root_totals = &nodes[0];
    let logical_bytes = root_totals.logical_bytes;
    let allocated_bytes = root_totals.allocated_bytes;
    let file_count = root_totals.file_count;
    let directory_count = root_totals.directory_count.saturating_sub(1);

    let elapsed_ms = duration_ms(indexing_completed_at.duration_since(started_at));

    let snapshot = ScanSnapshot {
        root: 0,
        root_path: root_node_path,
        root_is_macos_volume,
        nodes,
        allocated_size_is_estimate: semantics.allocated_size_is_estimate,
        #[cfg(unix)]
        revision_filesystem_id: counters.revision_filesystem_id,
    };

    Ok(ScanOutput {
        result: ScanResult {
            root: root.to_string_lossy().into_owned(),
            display_name: root
                .file_name()
                .unwrap_or(root.as_os_str())
                .to_string_lossy()
                .into_owned(),
            backend,
            logical_bytes,
            allocated_bytes,
            file_count,
            directory_count,
            skipped_entries: counters.skipped_entries,
            skipped_filesystems: counters.skipped_filesystems,
            skipped_cloud_entries: counters.skipped_cloud_entries,
            duplicate_hard_links: counters.duplicate_hard_links,
            traversal_us,
            aggregation_us,
            indexing_us,
            elapsed_ms,
            allocated_size_is_estimate: semantics.allocated_size_is_estimate,
            hard_link_deduplication_supported: semantics.hard_link_deduplication_supported,
            same_filesystem_enforced: semantics.same_filesystem_enforced,
        },
        snapshot,
    })
}

fn report_finishing_progress_if_due<F>(
    progress: &mut ScanProgress,
    started_at: Instant,
    last_progress_at: &mut Instant,
    now: Instant,
    on_progress: &mut F,
) where
    F: FnMut(ScanProgress),
{
    if now.saturating_duration_since(*last_progress_at) < PROGRESS_INTERVAL {
        return;
    }
    progress.elapsed_ms = duration_ms(now.duration_since(started_at));
    on_progress(progress.clone());
    *last_progress_at = Instant::now();
}

fn aggregate_nodes<F>(
    nodes: &mut [InternalNode],
    cancel: &AtomicBool,
    mut on_checkpoint: F,
) -> Result<(), String>
where
    F: FnMut(usize),
{
    for (processed, node_id) in (1..nodes.len()).rev().enumerate() {
        if processed.is_multiple_of(AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES) {
            let cancelled = cancel.load(Ordering::Relaxed);
            on_checkpoint(processed);
            if cancelled {
                return Err("Scan cancelled.".to_string());
            }
        }
        compact_child_indexes(&mut nodes[node_id]);
        let parent = nodes[node_id]
            .parent_id()
            .expect("non-root scan nodes always have a parent");
        debug_assert!(parent < node_id);
        let totals = (
            nodes[node_id].logical_bytes,
            nodes[node_id].allocated_bytes,
            nodes[node_id].file_count,
            nodes[node_id].directory_count,
        );

        nodes[parent].logical_bytes = nodes[parent].logical_bytes.saturating_add(totals.0);
        nodes[parent].allocated_bytes = nodes[parent].allocated_bytes.saturating_add(totals.1);
        nodes[parent].file_count = nodes[parent].file_count.saturating_add(totals.2);
        nodes[parent].directory_count = nodes[parent].directory_count.saturating_add(totals.3);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Scan cancelled.".to_string());
    }
    compact_child_indexes(&mut nodes[0]);
    Ok(())
}

fn compact_child_indexes(node: &mut InternalNode) {
    if node.children.len() < node.children.capacity() {
        node.children.shrink_to_fit();
    }
}

fn release_nodes_off_thread<F>(nodes: Vec<InternalNode>, on_released: F) -> Result<(), String>
where
    F: FnOnce() + Send + 'static,
{
    thread::Builder::new()
        .name("cepa-scan-arena-release".to_string())
        .spawn(move || {
            drop(nodes);
            on_released();
        })
        .map(|_| ())
        .map_err(|error| format!("could not release the cancelled scan in the background: {error}"))
}

pub(crate) fn aggregate_synthetic_nodes<R, F, D>(
    node_count: usize,
    cancel: &AtomicBool,
    on_ready: R,
    on_checkpoint: F,
    on_released: D,
) -> Result<(), String>
where
    R: FnOnce(),
    F: FnMut(usize),
    D: FnOnce() + Send + 'static,
{
    let arena_len = node_count
        .checked_add(1)
        .ok_or_else(|| "aggregation benchmark node count is too large".to_string())?;
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(arena_len)
        .map_err(|error| format!("could not allocate aggregation benchmark arena: {error}"))?;
    nodes.push(InternalNode::root(Path::new("aggregation-benchmark")));
    nodes.extend((0..node_count).map(|_| InternalNode {
        name: OsString::new(),
        parent: Some(ParentId::new(0)),
        children: Vec::new(),
        kind: EntryKind::File,
        logical_bytes: 1,
        allocated_bytes: 1,
        file_count: 1,
        directory_count: 0,
        scan_revision: None,
    }));
    on_ready();
    match aggregate_nodes(&mut nodes, cancel, on_checkpoint) {
        Err(error) if error == "Scan cancelled." => {
            release_nodes_off_thread(nodes, on_released)?;
            Err(error)
        }
        result => result,
    }
}

impl ScanSnapshot {
    pub(crate) fn retained_payload_bytes(&self) -> usize {
        let arc_header = 2_usize.saturating_mul(std::mem::size_of::<usize>());
        let root_path = arc_header.saturating_add(std::mem::size_of_val(self.root_path.as_ref()));
        let node_arena = self
            .nodes
            .capacity()
            .saturating_mul(std::mem::size_of::<InternalNode>());
        let node_allocations = self.nodes.iter().fold(0_usize, |total, node| {
            total.saturating_add(node.name.capacity()).saturating_add(
                node.children
                    .capacity()
                    .saturating_mul(std::mem::size_of::<NodeId>()),
            )
        });

        std::mem::size_of::<Self>()
            .saturating_add(root_path)
            .saturating_add(node_arena)
            .saturating_add(node_allocations)
    }

    pub(crate) fn root_path(&self) -> PathBuf {
        self.root_path.to_path_buf()
    }

    pub(crate) fn compression_target(&self, requested: u64) -> Result<CompressionTarget, String> {
        let node_id = self.valid_node_id(requested)?;
        let node = &self.nodes[node_id];
        Ok(CompressionTarget {
            path: self.node_path(node_id),
            kind: node.kind,
            logical_bytes: node.logical_bytes,
            allocated_bytes: node.allocated_bytes,
            allocated_size_is_estimate: self.allocated_size_is_estimate,
            scan_revision: self.scan_revision(node),
        })
    }

    fn scan_revision(&self, node: &InternalNode) -> Option<ScannedFileRevision> {
        #[cfg(unix)]
        {
            Some(node.scan_revision?.expand(self.revision_filesystem_id?))
        }
        #[cfg(not(unix))]
        {
            node.scan_revision
        }
    }

    pub(crate) fn directory_view(
        &self,
        scan_id: u64,
        requested: u64,
    ) -> Result<DirectoryView, String> {
        self.directory_view_with_metric(scan_id, requested, SizeMetric::Allocated)
    }

    pub(crate) fn directory_view_with_metric(
        &self,
        scan_id: u64,
        requested: u64,
        metric: SizeMetric,
    ) -> Result<DirectoryView, String> {
        let node_id = self.valid_node_id(requested)?;
        let node = &self.nodes[node_id];

        if !matches!(node.kind, EntryKind::Directory) {
            return Err("Only folders can be opened in the scan map.".to_string());
        }

        let total_items = node.children.len();
        let suppressed_items = node
            .children
            .iter()
            .filter(|child| {
                !show_in_storage_view(
                    &self.nodes[**child],
                    metric,
                    self.root_is_macos_volume && node_id == self.root,
                )
            })
            .count();
        let ranked_ids = self.ranked_child_ids(node_id, MAX_LIST_ITEMS, metric);
        let items = ranked_ids
            .iter()
            .map(|child| self.scan_item(*child))
            .collect();
        let chart_item_count = ranked_ids.len().min(MAX_CHART_ITEMS_PER_DIRECTORY);
        let mut chart_budget = MAX_CHART_ITEMS_TOTAL;

        Ok(DirectoryView {
            scan_id,
            node_id: wire_id(node_id),
            root: self.root_path.to_string_lossy().into_owned(),
            path: self.node_path(node_id).to_string_lossy().into_owned(),
            display_name: node.name().to_string_lossy().into_owned(),
            logical_bytes: node.logical_bytes,
            allocated_bytes: node.allocated_bytes,
            total_items,
            suppressed_items,
            items_truncated: total_items - suppressed_items > MAX_LIST_ITEMS,
            breadcrumbs: self.breadcrumbs(node_id),
            items,
            chart_items: self.chart_items(
                node_id,
                0,
                &ranked_ids[..chart_item_count],
                total_items,
                metric,
                &mut chart_budget,
            ),
        })
    }

    pub(crate) fn reveal_path(&self, requested: u64) -> Result<PathBuf, String> {
        let node_id = self.valid_node_id(requested)?;
        if matches!(self.nodes[node_id].kind, EntryKind::Symlink) {
            return Err("Symbolic links cannot be revealed without following their target.".into());
        }
        Ok(self.node_path(node_id))
    }

    pub(crate) fn search_directory(
        &self,
        scan_id: u64,
        requested: u64,
        metric: SizeMetric,
        query: &str,
        cancel: &AtomicBool,
    ) -> Result<DirectorySearchResult, String> {
        let node_id = self.valid_node_id(requested)?;
        let node = &self.nodes[node_id];
        if !matches!(node.kind, EntryKind::Directory) {
            return Err("Only folders can be searched.".to_string());
        }

        let query = query.trim();
        if query.is_empty() {
            return Err("Enter a file or folder name to search for.".to_string());
        }
        if query.chars().count() > MAX_SEARCH_QUERY_CHARS {
            return Err(format!(
                "Search terms are limited to {MAX_SEARCH_QUERY_CHARS} characters."
            ));
        }
        let matcher = RegexBuilder::new(&regex::escape(query))
            .case_insensitive(true)
            .build()
            .map_err(|_| "That search term could not be prepared.".to_string())?;

        let mut matches = BinaryHeap::with_capacity(MAX_LIST_ITEMS + 1);
        let mut total_matches = 0_usize;
        for (index, child_id) in node.children.iter().enumerate() {
            if index % CANCELLATION_CHECK_INTERVAL_ENTRIES as usize == 0
                && cancel.load(Ordering::Relaxed)
            {
                return Err("Folder search cancelled.".to_string());
            }
            if !matcher.is_match(&self.nodes[*child_id].name().to_string_lossy()) {
                continue;
            }
            total_matches += 1;
            matches.push(RankedNode {
                node_id: *child_id,
                nodes: &self.nodes,
                metric,
            });
            if matches.len() > MAX_LIST_ITEMS {
                matches.pop();
            }
        }

        let mut ranked_ids = matches
            .into_iter()
            .map(|candidate| candidate.node_id)
            .collect::<Vec<_>>();
        ranked_ids.sort_unstable_by(|left, right| {
            compare_node_ids_by_metric(&self.nodes, *left, *right, metric)
        });

        Ok(DirectorySearchResult {
            scan_id,
            node_id: wire_id(node_id),
            query: query.to_string(),
            metric,
            total_matches,
            items_truncated: total_matches > MAX_LIST_ITEMS,
            items: ranked_ids
                .into_iter()
                .map(|child_id| self.scan_item(child_id))
                .collect(),
        })
    }

    fn valid_node_id(&self, requested: u64) -> Result<NodeId, String> {
        usize::try_from(requested)
            .ok()
            .filter(|node_id| *node_id < self.nodes.len())
            .ok_or_else(|| "That item is not part of this scan.".to_string())
    }

    fn scan_item(&self, node_id: NodeId) -> ScanItem {
        scan_item(&self.nodes, node_id)
    }

    fn breadcrumbs(&self, node_id: NodeId) -> Vec<Breadcrumb> {
        let mut breadcrumbs = Vec::new();
        let mut current = Some(node_id);

        while let Some(current_id) = current {
            let node = &self.nodes[current_id];
            breadcrumbs.push(Breadcrumb {
                id: wire_id(current_id),
                name: node.name().to_string_lossy().into_owned(),
            });
            if current_id == self.root {
                break;
            }
            current = node.parent_id();
        }

        breadcrumbs.reverse();
        breadcrumbs
    }

    fn chart_children(
        &self,
        parent: NodeId,
        depth: usize,
        metric: SizeMetric,
        budget: &mut usize,
    ) -> Vec<ChartItem> {
        if depth >= MAX_CHART_DEPTH || *budget == 0 {
            return Vec::new();
        }

        let total_items = self.nodes[parent].children.len();
        let ranked_ids = self.ranked_child_ids(parent, MAX_CHART_ITEMS_PER_DIRECTORY, metric);
        self.chart_items(parent, depth, &ranked_ids, total_items, metric, budget)
    }

    fn chart_items(
        &self,
        parent: NodeId,
        depth: usize,
        ranked_ids: &[NodeId],
        total_items: usize,
        metric: SizeMetric,
        budget: &mut usize,
    ) -> Vec<ChartItem> {
        if *budget == 0 || total_items == 0 {
            return Vec::new();
        }

        let mut selected_count = ranked_ids.len().min(*budget);
        if total_items > selected_count {
            selected_count = selected_count.min(budget.saturating_sub(1));
        }
        let include_aggregate = total_items > selected_count;
        let current_level_items = selected_count + usize::from(include_aggregate);
        *budget -= current_level_items;
        let selected_ids = &ranked_ids[..selected_count];

        let mut items: Vec<_> = selected_ids
            .iter()
            .map(|node_id| {
                let node = &self.nodes[*node_id];
                ChartItem {
                    id: Some(wire_id(*node_id)),
                    name: node.name().to_string_lossy().into_owned(),
                    kind: node.kind,
                    logical_bytes: node.logical_bytes,
                    allocated_bytes: node.allocated_bytes,
                    children: matches!(node.kind, EntryKind::Directory)
                        .then(|| self.chart_children(*node_id, depth + 1, metric, budget))
                        .unwrap_or_default(),
                }
            })
            .collect();

        if include_aggregate {
            let total =
                self.nodes[parent]
                    .children
                    .iter()
                    .fold((0_u64, 0_u64), |total, node_id| {
                        let node = &self.nodes[*node_id];
                        (
                            total.0.saturating_add(node.logical_bytes),
                            total.1.saturating_add(node.allocated_bytes),
                        )
                    });
            let selected = selected_ids.iter().fold((0_u64, 0_u64), |total, node_id| {
                let node = &self.nodes[*node_id];
                (
                    total.0.saturating_add(node.logical_bytes),
                    total.1.saturating_add(node.allocated_bytes),
                )
            });
            items.push(ChartItem {
                id: None,
                name: format!("{} more items", total_items - selected_count),
                kind: EntryKind::Other,
                logical_bytes: total.0.saturating_sub(selected.0),
                allocated_bytes: total.1.saturating_sub(selected.1),
                children: Vec::new(),
            });
        }

        items
    }

    fn ranked_child_ids(&self, parent: NodeId, limit: usize, metric: SizeMetric) -> Vec<NodeId> {
        ranked_node_ids(
            &self.nodes[parent].children,
            limit,
            &self.nodes,
            metric,
            self.root_is_macos_volume && parent == self.root,
        )
    }

    fn node_path(&self, node_id: NodeId) -> PathBuf {
        if node_id == self.root {
            return self.root_path.to_path_buf();
        }

        let mut ancestors = Vec::new();
        let mut current = node_id;
        while current != self.root {
            ancestors.push(current);
            current = self.nodes[current]
                .parent_id()
                .expect("non-root scan nodes always have a parent");
        }

        let mut path = self.root_path.to_path_buf();
        for ancestor in ancestors.into_iter().rev() {
            path.push(&self.nodes[ancestor].name);
        }
        path
    }
}

fn wire_id(node_id: NodeId) -> u64 {
    u64::try_from(node_id).expect("scan node IDs fit in the wire representation")
}

fn scan_item(nodes: &[InternalNode], node_id: NodeId) -> ScanItem {
    let node = &nodes[node_id];
    ScanItem {
        id: wire_id(node_id),
        name: node.name().to_string_lossy().into_owned(),
        kind: node.kind,
        logical_bytes: node.logical_bytes,
        allocated_bytes: node.allocated_bytes,
        file_count: node.file_count,
        directory_count: node.directory_count,
    }
}

#[inline(always)]
fn observe_partial_file(
    partial_ranking: &mut PartialRanking,
    nodes: &[InternalNode],
    node_id: NodeId,
    replaced_owner: Option<NodeId>,
) {
    let node = &nodes[node_id];
    // Empty files cannot contribute to a storage ranking. Skipping them is
    // especially important for metadata-heavy trees where traversal can
    // otherwise spend more time ranking than reading directory records.
    if matches!(node.kind, EntryKind::File)
        && (node.logical_bytes != 0 || node.allocated_bytes != 0)
    {
        let candidate = PartialCandidate {
            node_id,
            logical_bytes: node.logical_bytes,
            allocated_bytes: node.allocated_bytes,
        };
        if replaced_owner
            .is_some_and(|replaced_owner| partial_ranking.replace(replaced_owner, candidate))
        {
            return;
        }
        partial_ranking.observe(candidate);
    }
}

fn compare_relative_node_paths(
    nodes: &[InternalNode],
    left: NodeId,
    right: NodeId,
    left_components: &mut Vec<NodeId>,
    right_components: &mut Vec<NodeId>,
) -> std::cmp::Ordering {
    relative_node_path(nodes, left, left_components);
    relative_node_path(nodes, right, right_components);
    left_components
        .iter()
        .rev()
        .map(|node_id| nodes[*node_id].name())
        .cmp(
            right_components
                .iter()
                .rev()
                .map(|node_id| nodes[*node_id].name()),
        )
}

fn relative_node_path(nodes: &[InternalNode], mut node_id: NodeId, components: &mut Vec<NodeId>) {
    components.clear();
    while let Some(parent) = nodes[node_id].parent_id() {
        components.push(node_id);
        node_id = parent;
    }
}

fn compare_node_ids_by_metric(
    nodes: &[InternalNode],
    left: NodeId,
    right: NodeId,
    metric: SizeMetric,
) -> std::cmp::Ordering {
    let left_node = &nodes[left];
    let right_node = &nodes[right];
    metric
        .bytes(right_node)
        .cmp(&metric.bytes(left_node))
        .then_with(|| {
            metric
                .secondary_bytes(right_node)
                .cmp(&metric.secondary_bytes(left_node))
        })
        .then_with(|| left_node.name().cmp(right_node.name()))
        .then_with(|| left.cmp(&right))
}

fn retain_best_node_ids(
    node_ids: &mut Vec<NodeId>,
    limit: usize,
    nodes: &[InternalNode],
    metric: SizeMetric,
) {
    if node_ids.len() <= limit {
        return;
    }
    node_ids.select_nth_unstable_by(limit, |left, right| {
        compare_node_ids_by_metric(nodes, *left, *right, metric)
    });
    node_ids.truncate(limit);
}

fn ranked_node_ids(
    children: &[NodeId],
    limit: usize,
    nodes: &[InternalNode],
    metric: SizeMetric,
    at_macos_volume_root: bool,
) -> Vec<NodeId> {
    ranked_node_ids_with_clone_limit(
        children,
        limit,
        nodes,
        metric,
        at_macos_volume_root,
        MAX_RANKING_CLONE_IDS,
    )
}

// Presentation only: retain every scanned item for accounting, explicit name
// searches, and scan-authorized operations. Evaluate folders after aggregation
// using the requested metric; zero accounted bytes do not prove emptiness.
fn show_in_storage_view(
    node: &InternalNode,
    metric: SizeMetric,
    at_macos_volume_root: bool,
) -> bool {
    match node.kind {
        EntryKind::Directory => {
            metric.bytes(node) != 0 && !(at_macos_volume_root && node.name() == ".fseventsd")
        }
        EntryKind::File => !SUPPRESSED_FILE_NAMES
            .iter()
            .any(|name| node.name() == *name),
        EntryKind::Symlink | EntryKind::Other => true,
    }
}

const SUPPRESSED_FILE_NAMES: &[&str] = &[".DS_Store"];

fn ranked_node_ids_with_clone_limit(
    children: &[NodeId],
    limit: usize,
    nodes: &[InternalNode],
    metric: SizeMetric,
    at_macos_volume_root: bool,
    max_clone_ids: usize,
) -> Vec<NodeId> {
    if limit == 0 {
        return Vec::new();
    }

    // A single partition has the lowest measured latency for ordinary and
    // moderately wide folders. Cap that transient clone at 2 MiB, then reduce
    // fixed-size groups so pathological directories cannot allocate one ID for
    // every child. The bounded top-500 path retains 8,000 IDs (about 64 KiB on
    // 64-bit platforms) while preserving O(n) selection work.
    let mut ranked = if children.len() > max_clone_ids {
        bounded_ranked_node_ids(children, limit, nodes, metric, at_macos_volume_root)
    } else {
        children
            .iter()
            .copied()
            .filter(|child| show_in_storage_view(&nodes[*child], metric, at_macos_volume_root))
            .collect()
    };
    retain_best_node_ids(&mut ranked, limit, nodes, metric);
    ranked.sort_unstable_by(|left, right| compare_node_ids_by_metric(nodes, *left, *right, metric));
    ranked
}

fn bounded_ranked_node_ids(
    children: &[NodeId],
    limit: usize,
    nodes: &[InternalNode],
    metric: SizeMetric,
    at_macos_volume_root: bool,
) -> Vec<NodeId> {
    let buffer_capacity = limit.saturating_mul(RANKING_BUFFER_MULTIPLIER);
    let mut ranked = Vec::with_capacity(buffer_capacity);
    for child in children.iter().copied() {
        if !show_in_storage_view(&nodes[child], metric, at_macos_volume_root) {
            continue;
        }
        ranked.push(child);
        if ranked.len() == buffer_capacity {
            retain_best_node_ids(&mut ranked, limit, nodes, metric);
        }
    }
    retain_best_node_ids(&mut ranked, limit, nodes, metric);
    ranked
}

#[derive(Clone, Copy)]
struct RankedNode<'a> {
    node_id: NodeId,
    nodes: &'a [InternalNode],
    metric: SizeMetric,
}

impl PartialEq for RankedNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
    }
}

impl Eq for RankedNode<'_> {}

impl PartialOrd for RankedNode<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankedNode<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        debug_assert!(std::ptr::eq(self.nodes, other.nodes));
        debug_assert_eq!(self.metric, other.metric);
        compare_node_ids_by_metric(self.nodes, self.node_id, other.node_id, self.metric)
    }
}

impl SizeMetric {
    fn bytes(self, node: &InternalNode) -> u64 {
        match self {
            Self::Allocated => node.allocated_bytes,
            Self::Logical => node.logical_bytes,
        }
    }

    fn secondary_bytes(self, node: &InternalNode) -> u64 {
        match self {
            Self::Allocated => node.logical_bytes,
            Self::Logical => node.allocated_bytes,
        }
    }
}

fn compare_partial_candidates(
    left: &PartialCandidate,
    right: &PartialCandidate,
) -> std::cmp::Ordering {
    right
        .allocated_bytes
        .cmp(&left.allocated_bytes)
        .then_with(|| right.logical_bytes.cmp(&left.logical_bytes))
        .then_with(|| left.node_id.cmp(&right.node_id))
}

fn elapsed_ms(started_at: Instant) -> u64 {
    duration_ms(started_at.elapsed())
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn duration_us(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

fn measure_metadata(metadata: &Metadata, is_file: bool) -> MeasuredMetadata {
    let (logical_bytes, allocated_bytes) = if is_file {
        (metadata.len(), allocated_bytes(metadata))
    } else {
        (0, 0)
    };

    MeasuredMetadata {
        logical_bytes,
        allocated_bytes,
        filesystem_id: filesystem_id(metadata),
        file_identity: is_file.then(|| file_identity(metadata)).flatten(),
        scan_revision: is_file.then(|| scanned_file_revision(metadata)).flatten(),
        metadata_error: false,
    }
}

#[cfg(unix)]
fn allocated_bytes(metadata: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated_bytes(metadata: &Metadata) -> u64 {
    metadata.len()
}

#[cfg(target_os = "macos")]
fn is_macos_volume_root(root: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let Ok(path) = std::ffi::CString::new(root.as_os_str().as_bytes()) else {
        return false;
    };
    let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: path is null-terminated and filesystem points to writable storage.
    if unsafe { libc::statfs(path.as_ptr(), filesystem.as_mut_ptr()) } != 0 {
        return false;
    }
    // SAFETY: statfs succeeded and initialized the output structure.
    let filesystem = unsafe { filesystem.assume_init() };
    // Compare the canonical scan root to the actual mount path, not a guessed
    // /Volumes prefix. In particular, the APFS Data volume is its own root.
    let mount_bytes: Vec<u8> = filesystem
        .f_mntonname
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    root == Path::new(OsStr::from_bytes(&mount_bytes))
}

#[cfg(not(target_os = "macos"))]
fn is_macos_volume_root(_: &Path) -> bool {
    false
}

fn portable_allocated_size_is_estimate(root: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        linux_path_allocated_size_is_estimate(root)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        !cfg!(unix)
    }
}

#[cfg(target_os = "linux")]
fn linux_path_allocated_size_is_estimate(root: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let Ok(root) = std::ffi::CString::new(root.as_os_str().as_bytes()) else {
        return true;
    };
    let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: root is null-terminated and filesystem points to writable storage.
    if unsafe { libc::statfs(root.as_ptr(), filesystem.as_mut_ptr()) } != 0 {
        return true;
    }
    // SAFETY: statfs succeeded and initialized the output structure.
    linux_filesystem_allocation_is_estimate(unsafe { filesystem.assume_init() }.f_type)
}

#[cfg(target_os = "linux")]
fn linux_file_descriptor_allocated_size_is_estimate(fd: std::os::fd::RawFd) -> bool {
    let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: fd is open and filesystem points to writable storage.
    if unsafe { libc::fstatfs(fd, filesystem.as_mut_ptr()) } != 0 {
        return true;
    }
    // SAFETY: fstatfs succeeded and initialized the output structure.
    linux_filesystem_allocation_is_estimate(unsafe { filesystem.assume_init() }.f_type)
}

#[cfg(target_os = "linux")]
fn linux_filesystem_allocation_is_estimate(filesystem_type: libc::c_long) -> bool {
    // Btrfs reports the uncompressed referenced extent length through st_blocks
    // for encoded extents. Exact compressed physical length requires privileged
    // Btrfs-internal inspection, so ordinary scans must not label it exact.
    filesystem_type == libc::BTRFS_SUPER_MAGIC
}

#[cfg(unix)]
fn filesystem_id(metadata: &Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.dev())
}

#[cfg(not(unix))]
fn filesystem_id(_: &Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn file_identity(metadata: &Metadata) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    (metadata.nlink() > 1).then(|| FileIdentity(metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_identity(_: &Metadata) -> Option<FileIdentity> {
    None
}

#[cfg(unix)]
fn scanned_file_revision(metadata: &Metadata) -> Option<ScannedFileRevision> {
    ScannedFileRevision::from_unix_metadata(metadata)
}

#[cfg(not(unix))]
fn scanned_file_revision(_: &Metadata) -> Option<ScannedFileRevision> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn compact_parent_id_round_trips_in_one_machine_word() {
        assert_eq!(
            std::mem::size_of::<Option<ParentId>>(),
            std::mem::size_of::<NodeId>()
        );

        for node_id in [0, 1, usize::MAX - 1] {
            assert_eq!(ParentId::new(node_id).node_id(), node_id);
        }
    }

    #[test]
    fn traversal_progress_is_time_bounded() {
        let last_progress_at = Instant::now();
        let mut clock = TraversalProgressClock::new(last_progress_at);
        assert!(
            !clock.update_due_at(last_progress_at + PROGRESS_INTERVAL - Duration::from_millis(1),)
        );
        assert!(clock.update_due_at(last_progress_at + PROGRESS_INTERVAL));
    }

    #[test]
    fn traversal_progress_amortizes_clock_checks() {
        let mut clock = TraversalProgressClock::new(Instant::now());
        assert!(clock.should_check_clock());
        assert!(!clock.update_due_at(clock.last_update_at + Duration::from_millis(1)));
        for _ in 1..PROGRESS_CLOCK_CHECK_INTERVAL_ENTRIES {
            assert!(!clock.should_check_clock());
        }
        assert!(clock.should_check_clock());
    }

    #[test]
    fn traversal_progress_adapts_to_slow_entries() {
        let started_at = Instant::now();
        let mut clock = TraversalProgressClock::new(started_at);
        assert!(clock.should_check_clock());
        assert!(!clock.update_due_at(started_at + Duration::from_millis(50)));
        assert_eq!(clock.entries_until_check, 1);
        assert!(clock.should_check_clock());
        let update_at = started_at + PROGRESS_INTERVAL;
        assert!(clock.update_due_at(update_at));
        clock.mark_updated(update_at);
        assert_eq!(clock.entries_until_check, 2);
    }

    #[test]
    fn traversal_progress_keeps_fast_clock_reads_and_updates_bounded() {
        let started_at = Instant::now();
        let mut clock = TraversalProgressClock::new(started_at);
        let entries = 101_011_u32;
        let mut clock_checks = 0_u32;
        let mut updates = Vec::new();
        for entry in 1..=entries {
            if !clock.should_check_clock() {
                continue;
            }
            clock_checks += 1;
            let now = started_at + Duration::from_micros(u64::from(entry));
            if clock.update_due_at(now) {
                clock.mark_updated(now);
                updates.push(now);
            }
        }

        assert!(clock_checks <= entries.div_ceil(32) + 2);
        assert!(clock_checks * 30 < entries);
        assert_eq!(updates.len(), 1);
        assert!(updates[0].duration_since(started_at) >= PROGRESS_INTERVAL);
    }

    #[test]
    fn aggregation_cancellation_bounds_work_after_the_request() {
        let cancel = AtomicBool::new(false);
        let mut nodes_processed = 0;
        let caller_thread = thread::current().id();
        let (released_tx, released_rx) = std::sync::mpsc::sync_channel(1);
        let error = aggregate_synthetic_nodes(
            AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES * 3,
            &cancel,
            || {},
            |processed| {
                nodes_processed = processed;
                if processed == 0 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            move || {
                let _ = released_tx.send(thread::current().id());
            },
        )
        .expect_err("aggregation must observe cancellation");

        assert_eq!(error, "Scan cancelled.");
        assert_eq!(
            nodes_processed,
            AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES
        );
        let release_thread = released_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("cancelled arena must be released in the background");
        assert_ne!(release_thread, caller_thread);
    }

    #[test]
    fn compacts_excess_child_index_capacity() {
        let mut root = InternalNode::root(Path::new("compact-children"));
        root.children = Vec::with_capacity(32);
        root.children.extend([1, 2]);
        let original_capacity = root.children.capacity();
        compact_child_indexes(&mut root);

        assert_eq!(root.children, [1, 2]);
        assert!(root.children.capacity() < original_capacity);
    }

    #[test]
    fn aggregation_cancellation_precedes_child_index_compaction() {
        let mut root = InternalNode::root(Path::new("cancelled-compaction"));
        root.children = Vec::with_capacity(32);
        root.children.push(1);
        let original_capacity = root.children.capacity();
        let mut nodes = vec![root, InternalNode::root(Path::new("child"))];

        let error = aggregate_nodes(&mut nodes, &AtomicBool::new(true), |_| {})
            .expect_err("aggregation must observe cancellation before compacting");

        assert_eq!(error, "Scan cancelled.");
        assert_eq!(nodes[0].children.capacity(), original_capacity);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn treats_btrfs_allocation_metadata_as_an_estimate() {
        assert!(linux_filesystem_allocation_is_estimate(
            libc::BTRFS_SUPER_MAGIC
        ));
        assert!(!linux_filesystem_allocation_is_estimate(0));
    }

    #[test]
    fn scans_and_aggregates_a_directory_tree() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let nested = temp.path().join("nested");
        let root_file = temp.path().join("root.bin");
        fs::create_dir(&nested).expect("create nested directory");
        fs::write(&root_file, vec![1_u8; 17]).expect("write root file");
        fs::write(nested.join("child.bin"), vec![2_u8; 31]).expect("write nested file");
        let expected_revision = crate::file_revision::snapshot_no_follow(&root_file)
            .expect("snapshot root file revision")
            .scanned;

        let mut progress = Vec::new();
        let output = scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |update| {
            progress.push(update)
        })
        .expect("scan fixture");
        let result = output.result;

        assert_eq!(
            result.backend,
            if cfg!(target_os = "macos") {
                "getattrlistbulk"
            } else if cfg!(target_os = "linux") {
                "statx"
            } else {
                "jwalk"
            }
        );
        assert_eq!(result.logical_bytes, 48);
        assert_eq!(result.file_count, 2);
        assert_eq!(result.directory_count, 1);
        let final_progress = progress.last().expect("final progress");
        assert_eq!(final_progress.phase, ScanPhase::Finishing);
        assert_eq!(
            serde_json::to_value(final_progress).expect("serialize finishing progress")["phase"],
            "finishing"
        );
        assert_eq!(final_progress.logical_bytes, 48);
        assert_eq!(final_progress.largest_items.len(), 2);
        assert_eq!(final_progress.largest_items[0].name, "child.bin");
        assert_eq!(final_progress.largest_items[0].logical_bytes, 31);
        assert_arena_invariants(&output.snapshot);

        let view = output
            .snapshot
            .directory_view(7, 0)
            .expect("build root view");
        assert_eq!(view.scan_id, 7);
        assert_eq!(view.items.len(), 2);
        assert_eq!(view.items[0].logical_bytes, 31);
        assert_eq!(view.items[0].file_count, 1);
        assert_eq!(view.chart_items.len(), 2);

        let nested_id = view
            .items
            .iter()
            .find(|item| item.name == "nested")
            .expect("nested directory item")
            .id;
        let nested_view = output
            .snapshot
            .directory_view(7, nested_id)
            .expect("open nested view");
        assert_eq!(nested_view.items.len(), 1);
        assert_eq!(nested_view.items[0].name, "child.bin");
        assert_eq!(nested_view.breadcrumbs.len(), 2);
        assert_eq!(
            nested_view.path,
            nested
                .canonicalize()
                .expect("canonical path")
                .to_string_lossy()
        );

        let file_id = view
            .items
            .iter()
            .find(|item| item.name == "root.bin")
            .expect("root file item")
            .id;
        let compression_target = output
            .snapshot
            .compression_target(file_id)
            .expect("build compression target");
        assert_eq!(
            compression_target.scan_revision,
            Some(expected_revision),
            "the retained revision must reconstruct the exact planning identity"
        );
        assert_eq!(
            output
                .snapshot
                .directory_view(7, file_id)
                .expect_err("files cannot be opened"),
            "Only folders can be opened in the scan map."
        );
        assert_eq!(
            output
                .snapshot
                .directory_view(7, u64::MAX)
                .expect_err("unknown node IDs must fail"),
            "That item is not part of this scan."
        );
    }

    #[test]
    fn finishing_progress_precedes_and_can_cancel_aggregation() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        fs::write(temp.path().join("file.bin"), vec![1_u8; 17]).expect("write fixture file");

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_from_progress = cancel.clone();
        let error =
            scan_path_with_backend(temp.path(), cancel, ScanBackend::Jwalk, move |progress| {
                if progress.phase == ScanPhase::Finishing {
                    cancel_from_progress.store(true, Ordering::Relaxed);
                }
            })
            .expect_err("finishing progress should arrive before aggregation completes");

        assert_eq!(error, "Scan cancelled.");
    }

    #[test]
    fn finishing_progress_heartbeats_are_time_bounded_and_refresh_elapsed_time() {
        let mut progress = ScanProgress {
            phase: ScanPhase::Finishing,
            entries_scanned: 17,
            files_scanned: 11,
            directories_scanned: 6,
            logical_bytes: 23,
            allocated_bytes: 29,
            skipped_entries: 0,
            current_path: "/fixture".to_string(),
            elapsed_ms: 0,
            largest_items: Vec::new(),
        };
        let now = Instant::now();
        let started_at = now - Duration::from_millis(250);
        let mut last_progress_at = now;
        let mut updates = Vec::new();

        report_finishing_progress_if_due(
            &mut progress,
            started_at,
            &mut last_progress_at,
            now,
            &mut |update| updates.push(update),
        );
        assert!(updates.is_empty(), "an early checkpoint must not emit");

        last_progress_at = now - PROGRESS_INTERVAL;
        report_finishing_progress_if_due(
            &mut progress,
            started_at,
            &mut last_progress_at,
            now,
            &mut |update| updates.push(update),
        );
        assert_eq!(updates.len(), 1);
        assert!(updates[0].elapsed_ms >= 250);
        assert_eq!(updates[0].phase, ScanPhase::Finishing);

        report_finishing_progress_if_due(
            &mut progress,
            started_at,
            &mut last_progress_at,
            now,
            &mut |update| updates.push(update),
        );
        assert_eq!(updates.len(), 1, "back-to-back checkpoints must coalesce");
    }

    #[cfg(unix)]
    #[test]
    fn retained_revisions_refuse_a_second_filesystem_identity() {
        let mut counters = ScanCounters::default();
        let first = ScannedFileRevision::from_raw_parts(17, 3, 5, 7).expect("build first revision");
        let other_filesystem =
            ScannedFileRevision::from_raw_parts(19, 11, 13, 17).expect("build second revision");

        let retained = counters
            .retain_scan_revision(Some(first))
            .expect("retain first filesystem revision");
        assert_eq!(retained.expand(17), first);
        assert!(
            counters
                .retain_scan_revision(Some(other_filesystem))
                .is_none(),
            "a compact revision must never inherit the wrong filesystem identity"
        );
        assert_eq!(counters.revision_filesystem_id, Some(17));
    }

    #[test]
    fn applies_a_native_volume_name_without_changing_the_scan_root() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let canonical_root = temp.path().canonicalize().expect("canonical fixture root");
        let mut output =
            scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {}).expect("scan fixture");

        output.set_root_display_name("Archive".to_string());
        let view = output
            .snapshot
            .directory_view(7, 0)
            .expect("build renamed root view");

        assert_eq!(output.result.display_name, "Archive");
        assert_eq!(output.result.root, canonical_root.to_string_lossy());
        assert_eq!(view.display_name, "Archive");
        assert_eq!(view.breadcrumbs[0].name, "Archive");
        assert_eq!(view.root, canonical_root.to_string_lossy());
    }

    #[test]
    fn bounds_and_orders_partial_results() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        for size in 1..=12_u8 {
            fs::write(
                temp.path().join(format!("file-{size:02}.bin")),
                vec![size; usize::from(size)],
            )
            .expect("write ranked fixture file");
        }

        let mut progress = Vec::new();
        scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |update| {
            progress.push(update)
        })
        .expect("scan ranked fixture");

        let largest = &progress.last().expect("final progress").largest_items;
        assert_eq!(largest.len(), MAX_PARTIAL_ITEMS);
        assert!(
            largest
                .windows(2)
                .all(|pair| pair[0].allocated_bytes >= pair[1].allocated_bytes)
        );
        assert_eq!(largest[0].logical_bytes, 12);
        assert_eq!(largest.last().expect("eighth item").logical_bytes, 5);
    }

    #[test]
    fn searches_all_direct_children_before_bounding_results() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        for index in 0..=MAX_LIST_ITEMS {
            fs::write(temp.path().join(format!("ordinary-{index:03}.bin")), b"xx")
                .expect("write ordinary fixture file");
        }
        fs::write(temp.path().join("hidden-needle.bin"), b"x")
            .expect("write below-cutoff fixture file");
        fs::write(temp.path().join("Résumé.txt"), b"x").expect("write Unicode fixture file");
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).expect("create nested fixture directory");
        fs::write(nested.join("child-needle.bin"), b"x").expect("write nested fixture file");

        let output =
            scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {}).expect("scan fixture");
        let view = output
            .snapshot
            .directory_view(7, 0)
            .expect("build bounded root view");
        assert_eq!(view.items.len(), MAX_LIST_ITEMS);
        assert!(view.items_truncated);
        assert!(
            !view
                .items
                .iter()
                .any(|item| item.name == "hidden-needle.bin")
        );

        let needle = output
            .snapshot
            .search_directory(
                7,
                0,
                SizeMetric::Allocated,
                " NEEDLE ",
                &AtomicBool::new(false),
            )
            .expect("search below the normal list cutoff");
        assert_eq!(needle.query, "NEEDLE");
        assert_eq!(needle.total_matches, 1);
        assert_eq!(needle.items[0].name, "hidden-needle.bin");
        assert!(!needle.items_truncated);

        let bounded = output
            .snapshot
            .search_directory(
                7,
                0,
                SizeMetric::Logical,
                "ordinary",
                &AtomicBool::new(false),
            )
            .expect("search a large matching set");
        assert_eq!(bounded.total_matches, MAX_LIST_ITEMS + 1);
        assert_eq!(bounded.items.len(), MAX_LIST_ITEMS);
        assert!(bounded.items_truncated);
        assert_eq!(bounded.metric, SizeMetric::Logical);
        assert_eq!(
            bounded.items.first().expect("first match").name,
            "ordinary-000.bin"
        );
        assert_eq!(
            bounded.items.last().expect("last retained match").name,
            "ordinary-499.bin"
        );

        let unicode = output
            .snapshot
            .search_directory(
                7,
                0,
                SizeMetric::Allocated,
                "RÉSUMÉ",
                &AtomicBool::new(false),
            )
            .expect("search Unicode names case-insensitively");
        assert_eq!(unicode.total_matches, 1);
        assert_eq!(unicode.items[0].name, "Résumé.txt");

        let nested_only = output
            .snapshot
            .search_directory(
                7,
                0,
                SizeMetric::Allocated,
                "child-needle",
                &AtomicBool::new(false),
            )
            .expect("search only direct children");
        assert_eq!(nested_only.total_matches, 0);
        assert!(nested_only.items.is_empty());

        assert_eq!(
            output
                .snapshot
                .search_directory(7, 0, SizeMetric::Allocated, "  ", &AtomicBool::new(false),)
                .expect_err("reject an empty query"),
            "Enter a file or folder name to search for."
        );
        assert_eq!(
            output
                .snapshot
                .search_directory(
                    7,
                    0,
                    SizeMetric::Allocated,
                    &"x".repeat(MAX_SEARCH_QUERY_CHARS + 1),
                    &AtomicBool::new(false),
                )
                .expect_err("reject an oversized query"),
            "Search terms are limited to 128 characters."
        );

        let cancelled = AtomicBool::new(true);
        assert_eq!(
            output
                .snapshot
                .search_directory(7, 0, SizeMetric::Allocated, "ordinary", &cancelled)
                .expect_err("cancel before matching"),
            "Folder search cancelled."
        );
    }

    #[test]
    fn reconstructs_deep_paths_and_breadcrumbs_from_node_ids() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let deepest = temp.path().join("alpha").join("beta").join("gamma");
        fs::create_dir_all(&deepest).expect("create deep directory tree");
        fs::write(deepest.join("payload.bin"), vec![9_u8; 23]).expect("write deep file");

        let output =
            scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {}).expect("scan fixture");
        assert_arena_invariants(&output.snapshot);

        let mut view = output
            .snapshot
            .directory_view(11, 0)
            .expect("open root view");
        for expected_name in ["alpha", "beta", "gamma"] {
            let child_id = view
                .items
                .iter()
                .find(|item| item.name == expected_name)
                .expect("expected child directory")
                .id;
            view = output
                .snapshot
                .directory_view(11, child_id)
                .expect("open child directory");
        }

        assert_eq!(
            view.path,
            deepest
                .canonicalize()
                .expect("canonical path")
                .to_string_lossy()
        );
        assert_eq!(
            view.breadcrumbs
                .iter()
                .map(|breadcrumb| breadcrumb.name.as_str())
                .skip(1)
                .collect::<Vec<_>>(),
            ["alpha", "beta", "gamma"]
        );
        assert_eq!(view.items[0].name, "payload.bin");
        assert_eq!(view.logical_bytes, 23);
    }

    #[test]
    fn rejects_a_file_as_the_scan_root() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let file = temp.path().join("file.bin");
        fs::write(&file, [1_u8]).expect("write fixture file");

        let error = scan_path(&file, Arc::new(AtomicBool::new(false)), |_| {})
            .expect_err("file roots must fail");

        assert_eq!(error, "Choose a directory to scan.");
    }

    #[test]
    fn validates_a_dropped_scan_root_without_starting_traversal() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let file = temp.path().join("file.bin");
        fs::write(&file, [1_u8]).expect("write fixture file");

        assert_eq!(
            validate_scan_root(temp.path())
                .expect("validate directory root")
                .0,
            temp.path().canonicalize().expect("canonical fixture root")
        );
        assert_eq!(
            validate_scan_root(&file).expect_err("reject a dropped file"),
            "Choose a directory to scan."
        );
    }

    #[test]
    fn honors_cancellation_before_traversal() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let cancel = Arc::new(AtomicBool::new(true));

        let error = scan_path(temp.path(), cancel, |_| {}).expect_err("scan must cancel");

        assert_eq!(error, "Scan cancelled.");
    }

    #[test]
    fn accounting_identity_excludes_backend_and_timings() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        fs::write(temp.path().join("file.bin"), [1_u8]).expect("write fixture file");
        let result = scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {})
            .expect("scan fixture")
            .result;
        let mut observed = result.clone();

        observed.backend = "different-backend";
        observed.traversal_us = observed.traversal_us.saturating_add(1);
        observed.elapsed_ms = observed.elapsed_ms.saturating_add(1);
        assert!(result.accounting_mismatches(&observed).is_empty());

        observed.skipped_entries = observed.skipped_entries.saturating_add(1);
        assert_eq!(result.accounting_mismatches(&observed), ["skipped_entries"]);
    }

    #[test]
    fn relative_path_comparison_reuses_scratch_buffers() {
        let mut nodes = vec![InternalNode::root(Path::new("root"))];
        let mut counters = ScanCounters::default();
        let alpha = counters
            .push_node(
                &mut nodes,
                0,
                OsString::from("alpha"),
                EntryKind::Directory,
                MeasuredMetadata::default(),
            )
            .0;
        let zeta = counters
            .push_node(
                &mut nodes,
                0,
                OsString::from("zeta"),
                EntryKind::Directory,
                MeasuredMetadata::default(),
            )
            .0;
        let left = counters
            .push_node(
                &mut nodes,
                alpha,
                OsString::from("z-last"),
                EntryKind::File,
                MeasuredMetadata::default(),
            )
            .0;
        let right = counters
            .push_node(
                &mut nodes,
                zeta,
                OsString::from("a-first"),
                EntryKind::File,
                MeasuredMetadata::default(),
            )
            .0;
        let mut left_scratch = Vec::new();
        let mut right_scratch = Vec::new();

        assert!(
            compare_relative_node_paths(
                &nodes,
                left,
                right,
                &mut left_scratch,
                &mut right_scratch,
            )
            .is_lt()
        );
        let capacities = (left_scratch.capacity(), right_scratch.capacity());

        assert!(
            compare_relative_node_paths(
                &nodes,
                right,
                left,
                &mut left_scratch,
                &mut right_scratch,
            )
            .is_gt()
        );
        assert_eq!(
            (left_scratch.capacity(), right_scratch.capacity()),
            capacities
        );
    }

    #[cfg(unix)]
    #[test]
    fn assigns_hard_linked_content_to_the_first_relative_path() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let later = temp.path().join("z-owner");
        let earlier = temp.path().join("a-owner");
        fs::create_dir(&later).expect("create later owner directory first");
        fs::create_dir(&earlier).expect("create earlier owner directory second");
        let original = later.join("original.bin");
        fs::write(&original, vec![7_u8; 64]).expect("write fixture file");
        fs::hard_link(&original, earlier.join("copy.bin")).expect("create hard link");

        let mut backends = vec![ScanBackend::Jwalk];
        #[cfg(target_os = "macos")]
        backends.push(ScanBackend::Getattrlistbulk);
        #[cfg(target_os = "linux")]
        backends.push(ScanBackend::Statx);

        for backend in backends {
            let mut progress = Vec::new();
            let output = scan_path_with_backend(
                temp.path(),
                Arc::new(AtomicBool::new(false)),
                backend,
                |update| progress.push(update),
            )
            .expect("scan fixture");
            let result = &output.result;

            assert_eq!(result.logical_bytes, 64);
            assert_eq!(result.file_count, 2);
            assert_eq!(result.duplicate_hard_links, 1);

            let root_view = output
                .snapshot
                .directory_view(1, 0)
                .expect("build root view");
            let earlier_id = root_view
                .items
                .iter()
                .find(|item| item.name == "a-owner")
                .expect("find deterministic owner")
                .id;
            assert!(!root_view.items.iter().any(|item| item.name == "z-owner"));
            let later_id = output
                .snapshot
                .search_directory(
                    1,
                    0,
                    SizeMetric::Allocated,
                    "z-owner",
                    &AtomicBool::new(false),
                )
                .expect("find suppressed non-owner through search")
                .items
                .iter()
                .find(|item| item.name == "z-owner")
                .expect("find non-owner")
                .id;
            assert_eq!(
                output
                    .snapshot
                    .directory_view(1, earlier_id)
                    .expect("open deterministic owner")
                    .logical_bytes,
                64
            );
            assert_eq!(
                output
                    .snapshot
                    .directory_view(1, later_id)
                    .expect("open non-owner")
                    .logical_bytes,
                0
            );
            assert_eq!(
                progress
                    .last()
                    .expect("final progress")
                    .largest_items
                    .first()
                    .expect("largest hard link")
                    .name,
                "copy.bin"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_directory_symlinks() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("create fixture directory");
        let outside = tempfile::tempdir().expect("create outside directory");
        fs::write(outside.path().join("outside.bin"), vec![3_u8; 128]).expect("write outside file");
        symlink(outside.path(), temp.path().join("linked-folder")).expect("create symlink");

        let output =
            scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {}).expect("scan fixture");
        let view = output
            .snapshot
            .directory_view(1, 0)
            .expect("build root view");
        assert_arena_invariants(&output.snapshot);

        assert_eq!(output.result.logical_bytes, 0);
        assert_eq!(output.result.file_count, 0);
        assert_eq!(view.items.len(), 1);
        assert!(matches!(view.items[0].kind, EntryKind::Symlink));
        assert_eq!(
            output
                .snapshot
                .reveal_path(view.items[0].id)
                .expect_err("revealing a symlink would follow its target"),
            "Symbolic links cannot be revealed without following their target."
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_backend_matches_portable_accounting() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        fs::write(temp.path().join("root.bin"), vec![1_u8; 17]).expect("write root file");
        fs::write(nested.join("child.bin"), vec![2_u8; 31]).expect("write nested file");

        let portable = scan_path_with_backend(
            temp.path(),
            Arc::new(AtomicBool::new(false)),
            ScanBackend::Jwalk,
            |_| {},
        )
        .expect("scan with jwalk");
        let native = scan_path_with_backend(
            temp.path(),
            Arc::new(AtomicBool::new(false)),
            ScanBackend::Getattrlistbulk,
            |_| {},
        )
        .expect("scan with getattrlistbulk");

        assert_eq!(portable.result.logical_bytes, native.result.logical_bytes);
        assert_eq!(
            portable.result.allocated_bytes,
            native.result.allocated_bytes
        );
        assert_eq!(portable.result.file_count, native.result.file_count);
        assert_eq!(
            portable.result.directory_count,
            native.result.directory_count
        );
        assert_eq!(
            portable.result.duplicate_hard_links,
            native.result.duplicate_hard_links
        );

        let portable_view = portable
            .snapshot
            .directory_view(1, 0)
            .expect("build portable view");
        let native_view = native
            .snapshot
            .directory_view(1, 0)
            .expect("build native view");
        let portable_items = portable_view
            .items
            .iter()
            .map(|item| (&item.name, item.logical_bytes, item.allocated_bytes))
            .collect::<Vec<_>>();
        let native_items = native_view
            .items
            .iter()
            .map(|item| (&item.name, item.logical_bytes, item.allocated_bytes))
            .collect::<Vec<_>>();
        assert_eq!(portable_items, native_items);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_backend_matches_portable_accounting_and_auto_selects_statx() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        fs::write(temp.path().join("root.bin"), vec![1_u8; 17]).expect("write root file");
        fs::write(nested.join("child.bin"), vec![2_u8; 31]).expect("write nested file");

        let portable = scan_path_with_backend(
            temp.path(),
            Arc::new(AtomicBool::new(false)),
            ScanBackend::Jwalk,
            |_| {},
        )
        .expect("scan with jwalk");
        let native = scan_path_with_backend(
            temp.path(),
            Arc::new(AtomicBool::new(false)),
            ScanBackend::Statx,
            |_| {},
        )
        .expect("scan with statx");
        let automatic = scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {})
            .expect("scan with automatic backend");

        assert!(
            portable
                .result
                .accounting_mismatches(&native.result)
                .is_empty()
        );
        assert_eq!(native.result.backend, "statx");
        assert_eq!(automatic.result.backend, "statx");

        let portable_view = portable
            .snapshot
            .directory_view(1, 0)
            .expect("build portable view");
        let native_view = native
            .snapshot
            .directory_view(1, 0)
            .expect("build native view");
        let portable_items = portable_view
            .items
            .iter()
            .map(|item| (&item.name, item.logical_bytes, item.allocated_bytes))
            .collect::<Vec<_>>();
        let native_items = native_view
            .items
            .iter()
            .map(|item| (&item.name, item.logical_bytes, item.allocated_bytes))
            .collect::<Vec<_>>();
        assert_eq!(portable_items, native_items);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_auto_falls_back_when_statx_is_unavailable() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        fs::write(temp.path().join("file.bin"), vec![1_u8; 17]).expect("write fixture file");

        let output = scan_path_auto_linux(
            temp.path(),
            Arc::new(AtomicBool::new(false)),
            &mut |_| {},
            |_, _, _| Err(linux::NativeScanError::Unavailable),
        )
        .expect("fall back to portable scan");

        assert_eq!(output.result.backend, "jwalk");
        assert_eq!(output.result.logical_bytes, 17);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_auto_uses_portable_traversal_for_subfolders() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        fs::write(temp.path().join("file.bin"), vec![1_u8; 17]).expect("write fixture file");

        let output = scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {})
            .expect("scan subfolder with automatic backend");

        assert_eq!(output.result.backend, "jwalk");
        assert_eq!(output.result.logical_bytes, 17);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_backend_cancels_during_result_ingestion() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        for index in 0..=CANCELLATION_CHECK_INTERVAL_ENTRIES {
            fs::write(nested.join(format!("file-{index}")), []).expect("write fixture file");
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_from_progress = cancel.clone();
        let error = scan_path_with_backend(
            temp.path(),
            cancel,
            ScanBackend::Getattrlistbulk,
            move |_| cancel_from_progress.store(true, Ordering::Relaxed),
        )
        .expect_err("scan should stop after progress callback cancels it");

        assert_eq!(error, "Scan cancelled.");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_backend_cancels_during_result_ingestion() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        for index in 0..=CANCELLATION_CHECK_INTERVAL_ENTRIES {
            fs::write(nested.join(format!("file-{index}")), []).expect("write fixture file");
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_from_progress = cancel.clone();
        let error = scan_path_with_backend(temp.path(), cancel, ScanBackend::Statx, move |_| {
            cancel_from_progress.store(true, Ordering::Relaxed)
        })
        .expect_err("scan should stop after progress callback cancels it");

        assert_eq!(error, "Scan cancelled.");
    }

    #[test]
    fn bounds_large_directory_views() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        for index in 0..=MAX_LIST_ITEMS {
            fs::write(temp.path().join(format!("file-{index:04}.bin")), [1_u8])
                .expect("write fixture file");
        }

        let output =
            scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {}).expect("scan fixture");
        let view = output
            .snapshot
            .directory_view(1, 0)
            .expect("build root view");
        assert_arena_invariants(&output.snapshot);

        assert_eq!(view.total_items, MAX_LIST_ITEMS + 1);
        assert_eq!(view.items.len(), MAX_LIST_ITEMS);
        assert!(view.items_truncated);
        assert_eq!(
            view.items.last().expect("last listed item").name,
            "file-0499.bin"
        );
        assert_eq!(view.chart_items.len(), MAX_CHART_ITEMS_PER_DIRECTORY + 1);
        assert!(
            view.chart_items
                .last()
                .expect("aggregate item")
                .id
                .is_none()
        );
        assert_eq!(
            view.chart_items
                .iter()
                .map(|item| item.logical_bytes)
                .sum::<u64>(),
            output.result.logical_bytes
        );
    }

    #[test]
    fn suppresses_storage_clutter_without_losing_accounting_or_search_access() {
        let temp = tempfile::tempdir().expect("create fixture");
        for name in [
            ".DocumentRevisions-V100",
            ".Spotlight-V100",
            "TemporaryItems",
        ] {
            fs::create_dir(temp.path().join(name)).expect("create empty folder");
        }
        fs::create_dir(temp.path().join("nested")).expect("create nested folder");
        fs::write(temp.path().join(".DS_Store"), [1; 40]).expect("write housekeeping file");
        fs::write(temp.path().join("nested/.DS_Store"), [2; 60])
            .expect("write nested housekeeping file");
        fs::write(temp.path().join(".DS_Store.backup"), [3]).expect("write ordinary dotfile");
        fs::write(temp.path().join("empty.txt"), []).expect("write ordinary empty file");

        let mut backends = vec![ScanBackend::Jwalk];
        #[cfg(target_os = "macos")]
        backends.push(ScanBackend::Getattrlistbulk);
        #[cfg(target_os = "linux")]
        backends.push(ScanBackend::Statx);
        for backend in backends {
            let output = scan_path_with_backend(
                temp.path(),
                Arc::new(AtomicBool::new(false)),
                backend,
                |_| {},
            )
            .expect("scan clutter fixture");
            assert_eq!(output.result.logical_bytes, 101);
            assert_eq!(output.result.file_count, 4);
            assert_eq!(output.result.directory_count, 4);
            for metric in [SizeMetric::Allocated, SizeMetric::Logical] {
                let view = output
                    .snapshot
                    .directory_view_with_metric(1, 0, metric)
                    .unwrap();
                assert_eq!(view.total_items, 7);
                assert_eq!(view.suppressed_items, 4);
                assert_eq!(view.items.len(), 3);
                assert!(!view.items_truncated);
                assert!(
                    view.items
                        .iter()
                        .any(|item| item.name == ".DS_Store.backup")
                );
                assert!(view.items.iter().any(|item| item.name == "empty.txt"));
                assert_eq!(
                    view.chart_items
                        .iter()
                        .map(|item| item.logical_bytes)
                        .sum::<u64>(),
                    view.logical_bytes
                );
                assert_eq!(
                    view.chart_items
                        .iter()
                        .map(|item| item.allocated_bytes)
                        .sum::<u64>(),
                    view.allocated_bytes
                );
                let nested = view
                    .chart_items
                    .iter()
                    .find(|item| item.name == "nested")
                    .unwrap();
                assert_eq!(nested.children.len(), 1);
                assert!(nested.children[0].id.is_none());
                assert_eq!(nested.children[0].logical_bytes, 60);
                let nested_view = output
                    .snapshot
                    .directory_view_with_metric(1, nested.id.unwrap(), metric)
                    .unwrap();
                assert!(nested_view.items.is_empty());
                assert_eq!(nested_view.suppressed_items, 1);
                for query in [".DS_Store", ".Spotlight-V100"] {
                    let matches = output
                        .snapshot
                        .search_directory(1, 0, metric, query, &AtomicBool::new(false))
                        .unwrap();
                    let found = matches
                        .items
                        .iter()
                        .find(|item| item.name == query)
                        .unwrap();
                    assert_eq!(
                        output.snapshot.reveal_path(found.id).unwrap(),
                        temp.path().canonicalize().unwrap().join(query)
                    );
                    if matches!(found.kind, EntryKind::Directory) {
                        let empty = output
                            .snapshot
                            .directory_view_with_metric(1, found.id, metric)
                            .unwrap();
                        assert!(empty.items.is_empty());
                        assert_eq!(empty.total_items, 0);
                        assert_eq!(empty.suppressed_items, 0);
                    }
                }
            }
        }
    }

    #[test]
    fn filters_before_both_ranking_paths_and_rechecks_folder_metric() {
        let mut root = InternalNode::root(Path::new("/fixture"));
        let mut nodes = vec![InternalNode::root(Path::new("/fixture"))];
        for index in 0..MAX_LIST_ITEMS - 1 {
            let mut file = test_directory_node(format!("file-{index:04}"), 0, 1);
            file.kind = EntryKind::File;
            nodes.push(file);
        }
        let mut housekeeping = test_directory_node(".DS_Store".into(), 0, 1_000_000);
        housekeeping.kind = EntryKind::File;
        nodes.push(housekeeping);
        nodes.push(test_directory_node("empty".into(), 0, 0));
        let mut sparse = test_directory_node("sparse-folder".into(), 0, 10_000);
        sparse.allocated_bytes = 0;
        let sparse_id = nodes.len();
        nodes.push(sparse);
        // A directory with this name is not a housekeeping file.
        nodes.push(test_directory_node(".DS_Store".into(), 0, 2));
        root.children = (1..nodes.len()).collect();
        nodes[0] = root;
        for metric in [SizeMetric::Allocated, SizeMetric::Logical] {
            let mut expected: Vec<_> = (1..MAX_LIST_ITEMS).collect();
            if metric == SizeMetric::Logical {
                expected.push(sparse_id);
            }
            expected.push(nodes.len() - 1);
            expected.sort_unstable_by(|left, right| {
                compare_node_ids_by_metric(&nodes, *left, *right, metric)
            });
            expected.truncate(MAX_LIST_ITEMS);
            for clone_limit in [0, usize::MAX] {
                let ranked = ranked_node_ids_with_clone_limit(
                    &nodes[0].children,
                    MAX_LIST_ITEMS,
                    &nodes,
                    metric,
                    false,
                    clone_limit,
                );
                assert_eq!(ranked, expected);
                assert!(ranked.capacity() <= MAX_LIST_ITEMS * RANKING_BUFFER_MULTIPLIER);
            }
        }
        let snapshot = ScanSnapshot {
            root: 0,
            root_path: Arc::from(Path::new("/fixture")),
            root_is_macos_volume: false,
            nodes,
            allocated_size_is_estimate: false,
            #[cfg(unix)]
            revision_filesystem_id: None,
        };
        let allocated = snapshot
            .directory_view_with_metric(1, 0, SizeMetric::Allocated)
            .unwrap();
        assert_eq!(allocated.items.len(), MAX_LIST_ITEMS);
        assert_eq!(allocated.suppressed_items, 3);
        assert!(!allocated.items_truncated);
        let logical = snapshot
            .directory_view_with_metric(1, 0, SizeMetric::Logical)
            .unwrap();
        assert_eq!(logical.items.len(), MAX_LIST_ITEMS);
        assert_eq!(logical.suppressed_items, 2);
        assert!(logical.items_truncated);
        assert_eq!(logical.items[0].id, wire_id(sparse_id));
    }

    #[test]
    fn volume_event_logs_remain_accounted_and_searchable_without_hiding_nested_names() {
        let temp = tempfile::tempdir().expect("create fixture");
        fs::create_dir(temp.path().join(".fseventsd")).unwrap();
        fs::write(temp.path().join(".fseventsd/events"), [1; 80]).unwrap();
        fs::create_dir_all(temp.path().join("ordinary/.fseventsd")).unwrap();
        fs::write(temp.path().join("ordinary/.fseventsd/user.txt"), [2; 20]).unwrap();
        let mut backends = vec![ScanBackend::Jwalk];
        #[cfg(target_os = "macos")]
        backends.push(ScanBackend::Getattrlistbulk);
        #[cfg(target_os = "linux")]
        backends.push(ScanBackend::Statx);
        for backend in backends {
            let mut output = scan_path_with_backend(
                temp.path(),
                Arc::new(AtomicBool::new(false)),
                backend,
                |_| {},
            )
            .unwrap();
            // The real temporary scan is not a volume root, so its names remain
            // visible. Simulate a qualified root on the same retained fixture
            // below without creating or altering any real volume metadata.
            assert!(!output.snapshot.root_is_macos_volume);
            assert_eq!(output.result.logical_bytes, 100);
            assert_eq!(output.result.file_count, 2);
            assert_eq!(output.result.directory_count, 3);
            let ordinary_view = output.snapshot.directory_view(1, 0).unwrap();
            assert_eq!(ordinary_view.items.len(), 2);
            assert_eq!(ordinary_view.suppressed_items, 0);
            output.snapshot.root_is_macos_volume = true;
            for metric in [SizeMetric::Allocated, SizeMetric::Logical] {
                let view = output
                    .snapshot
                    .directory_view_with_metric(1, 0, metric)
                    .unwrap();
                assert_eq!(view.total_items, 2);
                assert_eq!(view.suppressed_items, 1);
                assert_eq!(view.items.len(), 1);
                assert_eq!(view.items[0].name, "ordinary");
                assert_eq!(view.logical_bytes, 100);
                assert_eq!(view.allocated_bytes, output.result.allocated_bytes);
                assert_eq!(
                    view.chart_items
                        .iter()
                        .map(|item| item.logical_bytes)
                        .sum::<u64>(),
                    view.logical_bytes
                );
                assert_eq!(
                    view.chart_items
                        .iter()
                        .map(|item| item.allocated_bytes)
                        .sum::<u64>(),
                    view.allocated_bytes
                );
                assert!(
                    !view
                        .chart_items
                        .iter()
                        .any(|item| item.name == ".fseventsd")
                );
                let ordinary_chart = view
                    .chart_items
                    .iter()
                    .find(|item| item.name == "ordinary")
                    .unwrap();
                assert_eq!(ordinary_chart.children[0].name, ".fseventsd");
                let nested = output
                    .snapshot
                    .directory_view_with_metric(1, view.items[0].id, metric)
                    .unwrap();
                assert_eq!(nested.items[0].name, ".fseventsd");
                assert_eq!(nested.suppressed_items, 0);
                let search = output
                    .snapshot
                    .search_directory(1, 0, metric, ".fseventsd", &AtomicBool::new(false))
                    .unwrap();
                assert_eq!(search.total_matches, 1);
                let events = output
                    .snapshot
                    .directory_view_with_metric(1, search.items[0].id, metric)
                    .unwrap();
                assert_eq!(events.items[0].name, "events");
                assert_eq!(
                    output.snapshot.reveal_path(search.items[0].id).unwrap(),
                    temp.path().canonicalize().unwrap().join(".fseventsd")
                );
            }
        }
    }

    #[test]
    fn volume_event_directory_rule_precedes_both_ranking_limits_and_requires_directory_kind() {
        let nodes = vec![
            InternalNode::root(Path::new("/fixture")),
            test_directory_node(".fseventsd".into(), 0, 1_000),
            test_directory_node("ordinary".into(), 0, 1),
        ];
        for metric in [SizeMetric::Allocated, SizeMetric::Logical] {
            for clone_limit in [0, usize::MAX] {
                for qualified in [false, true] {
                    assert_eq!(
                        ranked_node_ids_with_clone_limit(
                            &[1, 2],
                            1,
                            &nodes,
                            metric,
                            qualified,
                            clone_limit
                        ),
                        vec![if qualified { 2 } else { 1 }]
                    );
                }
            }
            let mut named_entry = test_directory_node(".fseventsd".into(), 0, 1);
            for kind in [EntryKind::File, EntryKind::Symlink, EntryKind::Other] {
                named_entry.kind = kind;
                assert!(show_in_storage_view(&named_entry, metric, true));
            }
            named_entry.kind = EntryKind::Directory;
            named_entry.name = ".fseventsd.backup".into();
            assert!(show_in_storage_view(&named_entry, metric, true));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn qualifies_actual_macos_mount_roots_only() {
        let _protection = local_only::NoMaterialization::new().unwrap();
        assert!(is_macos_volume_root(Path::new("/")));
        // APFS startup scans use the Data volume instead of the sealed root.
        let data = Path::new("/System/Volumes/Data");
        if data.exists() {
            assert!(is_macos_volume_root(data));
        }
        let temp = tempfile::tempdir().unwrap();
        assert!(!is_macos_volume_root(&temp.path().canonicalize().unwrap()));
        assert!(!is_macos_volume_root(&temp.path().join("missing")));
    }

    #[test]
    fn globally_bounds_deep_chart_views_without_losing_accounted_bytes() {
        const BRANCHES: usize = MAX_CHART_ITEMS_PER_DIRECTORY + 1;

        let temp = tempfile::tempdir().expect("create fixture directory");
        let root_path = temp.path().canonicalize().expect("canonical fixture root");
        let mut nodes = vec![InternalNode::root(&root_path)];

        for outer in 0..BRANCHES {
            let outer_id = nodes.len();
            nodes.push(test_directory_node(
                format!("outer-{outer:02}"),
                0,
                (BRANCHES * BRANCHES) as u64,
            ));
            nodes[0].children.push(outer_id);

            for middle in 0..BRANCHES {
                let middle_id = nodes.len();
                nodes.push(test_directory_node(
                    format!("middle-{middle:02}"),
                    outer_id,
                    BRANCHES as u64,
                ));
                nodes[outer_id].children.push(middle_id);

                for inner in 0..BRANCHES {
                    let inner_id = nodes.len();
                    nodes.push(test_directory_node(
                        format!("inner-{inner:02}"),
                        middle_id,
                        1,
                    ));
                    nodes[middle_id].children.push(inner_id);
                }
            }
        }

        let total_bytes = BRANCHES.pow(3) as u64;
        nodes[0].logical_bytes = total_bytes;
        nodes[0].allocated_bytes = total_bytes;
        let snapshot = ScanSnapshot {
            root: 0,
            root_path: Arc::from(root_path),
            root_is_macos_volume: false,
            nodes,
            allocated_size_is_estimate: false,
            #[cfg(unix)]
            revision_filesystem_id: None,
        };

        let view = snapshot
            .directory_view(1, 0)
            .expect("build bounded deep view");
        let chart_items = view.chart_items.iter().map(chart_item_count).sum::<usize>();

        assert_eq!(chart_items, MAX_CHART_ITEMS_TOTAL);
        assert_eq!(view.chart_items.len(), MAX_CHART_ITEMS_PER_DIRECTORY + 1);
        assert_eq!(
            view.chart_items
                .iter()
                .map(|item| item.logical_bytes)
                .sum::<u64>(),
            total_bytes
        );
        for item in &view.chart_items {
            assert_chart_children_cover_parent(item);
        }
    }

    fn test_directory_node(name: String, parent: NodeId, bytes: u64) -> InternalNode {
        InternalNode {
            name: OsString::from(name),
            parent: Some(ParentId::new(parent)),
            children: Vec::new(),
            kind: EntryKind::Directory,
            logical_bytes: bytes,
            allocated_bytes: bytes,
            file_count: 0,
            directory_count: 1,
            scan_revision: None,
        }
    }

    fn assert_chart_children_cover_parent(item: &ChartItem) {
        if item.children.is_empty() {
            return;
        }
        assert_eq!(
            item.children
                .iter()
                .map(|child| child.logical_bytes)
                .sum::<u64>(),
            item.logical_bytes
        );
        assert_eq!(
            item.children
                .iter()
                .map(|child| child.allocated_bytes)
                .sum::<u64>(),
            item.allocated_bytes
        );
        for child in &item.children {
            assert_chart_children_cover_parent(child);
        }
    }

    #[test]
    fn metric_controls_ranking_and_bounded_selection() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let root_path = temp.path().canonicalize().expect("canonical fixture root");
        let mut root = InternalNode::root(&root_path);
        let mut nodes = Vec::with_capacity(MAX_LIST_ITEMS + 3);
        nodes.push(InternalNode::root(&root_path));

        for index in 0..=MAX_LIST_ITEMS {
            let node_id = nodes.len();
            root.children.push(node_id);
            nodes.push(InternalNode {
                name: OsString::from(format!("dense-{index:04}.bin")),
                parent: Some(ParentId::new(0)),
                children: Vec::new(),
                kind: EntryKind::File,
                logical_bytes: 10,
                allocated_bytes: 100,
                file_count: 1,
                directory_count: 0,
                scan_revision: None,
            });
        }
        let sparse_id = nodes.len();
        root.children.push(sparse_id);
        nodes.push(InternalNode {
            name: OsString::from("sparse.bin"),
            parent: Some(ParentId::new(0)),
            children: Vec::new(),
            kind: EntryKind::File,
            logical_bytes: 10_000,
            allocated_bytes: 0,
            file_count: 1,
            directory_count: 0,
            scan_revision: None,
        });
        root.logical_bytes = (MAX_LIST_ITEMS as u64 + 1) * 10 + 10_000;
        root.allocated_bytes = (MAX_LIST_ITEMS as u64 + 1) * 100;
        root.file_count = MAX_LIST_ITEMS as u64 + 2;
        nodes[0] = root;

        let snapshot = ScanSnapshot {
            root: 0,
            root_path: Arc::from(root_path),
            root_is_macos_volume: false,
            nodes,
            allocated_size_is_estimate: false,
            #[cfg(unix)]
            revision_filesystem_id: None,
        };
        let allocated = snapshot
            .directory_view_with_metric(1, 0, SizeMetric::Allocated)
            .expect("build allocated view");
        let logical = snapshot
            .directory_view_with_metric(1, 0, SizeMetric::Logical)
            .expect("build logical view");

        assert!(allocated.items_truncated);
        assert_eq!(allocated.items.len(), MAX_LIST_ITEMS);
        assert!(
            !allocated
                .items
                .iter()
                .any(|item| item.id == wire_id(sparse_id))
        );
        assert_eq!(logical.items[0].id, wire_id(sparse_id));
        assert_eq!(logical.chart_items[0].id, Some(wire_id(sparse_id)));
    }

    #[test]
    fn chunked_child_ranking_matches_a_complete_sort_with_bounded_capacity() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let root_path = temp.path().canonicalize().expect("canonical fixture root");
        let child_count = MAX_LIST_ITEMS * RANKING_BUFFER_MULTIPLIER * 2 + 17;
        let mut root = InternalNode::root(&root_path);
        let mut nodes = Vec::with_capacity(child_count + 1);
        nodes.push(InternalNode::root(&root_path));

        for index in 0..child_count {
            let node_id = nodes.len();
            root.children.push(node_id);
            nodes.push(InternalNode {
                name: OsString::from(format!("item-{:05}.bin", child_count - index)),
                parent: Some(ParentId::new(0)),
                children: Vec::new(),
                kind: EntryKind::File,
                logical_bytes: ((index * 3_571) % 15_000) as u64,
                allocated_bytes: ((index * 7_919) % 10_000) as u64,
                file_count: 1,
                directory_count: 0,
                scan_revision: None,
            });
        }
        nodes[0] = root;
        let snapshot = ScanSnapshot {
            root: 0,
            root_path: Arc::from(root_path),
            root_is_macos_volume: false,
            nodes,
            allocated_size_is_estimate: false,
            #[cfg(unix)]
            revision_filesystem_id: None,
        };

        for metric in [SizeMetric::Allocated, SizeMetric::Logical] {
            let mut expected = snapshot.nodes[0].children.clone();
            expected.sort_unstable_by(|left, right| {
                compare_node_ids_by_metric(&snapshot.nodes, *left, *right, metric)
            });
            expected.truncate(MAX_LIST_ITEMS);

            let ranked = ranked_node_ids_with_clone_limit(
                &snapshot.nodes[0].children,
                MAX_LIST_ITEMS,
                &snapshot.nodes,
                metric,
                false,
                MAX_LIST_ITEMS,
            );
            assert_eq!(ranked, expected);
            assert!(ranked.capacity() <= MAX_LIST_ITEMS * RANKING_BUFFER_MULTIPLIER);
        }
    }

    #[cfg(unix)]
    #[test]
    fn preserves_sparse_file_semantics_across_metrics() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let sparse_path = temp.path().join("sparse.bin");
        fs::File::create(&sparse_path)
            .expect("create sparse file")
            .set_len(8 * 1024 * 1024)
            .expect("extend sparse file");
        fs::write(temp.path().join("dense.bin"), vec![0x5a; 1024 * 1024])
            .expect("write dense file");

        let output = scan_path_with_backend(
            temp.path(),
            Arc::new(AtomicBool::new(false)),
            ScanBackend::Jwalk,
            |_| {},
        )
        .expect("scan sparse fixture");
        let allocated = output
            .snapshot
            .directory_view_with_metric(1, 0, SizeMetric::Allocated)
            .expect("build allocated view");
        let logical = output
            .snapshot
            .directory_view_with_metric(1, 0, SizeMetric::Logical)
            .expect("build logical view");
        let sparse = logical
            .items
            .iter()
            .find(|item| item.name == "sparse.bin")
            .expect("find sparse file");

        assert_eq!(sparse.logical_bytes, 8 * 1024 * 1024);
        assert!(sparse.allocated_bytes < sparse.logical_bytes);
        assert_eq!(logical.items[0].name, "sparse.bin");
        assert_eq!(allocated.items[0].name, "dense.bin");
    }

    fn assert_arena_invariants(snapshot: &ScanSnapshot) {
        assert_eq!(snapshot.root, 0);
        assert!(snapshot.root_path.is_absolute());

        for (node_id, node) in snapshot.nodes.iter().enumerate() {
            if let Some(parent_id) = node.parent_id() {
                assert!(parent_id < node_id);
                assert!(snapshot.nodes[parent_id].children.contains(&node_id));
            } else {
                assert_eq!(node_id, snapshot.root);
            }

            for child_id in &node.children {
                assert_eq!(snapshot.nodes[*child_id].parent_id(), Some(node_id));
            }

            assert!(!node.name.is_empty());
        }
    }
}
