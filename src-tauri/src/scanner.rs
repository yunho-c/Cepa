use jwalk::{Parallelism, WalkDirGeneric};
use serde::Serialize;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
mod macos;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const PROGRESS_ENTRY_INTERVAL: u64 = 2_048;
const MAX_LIST_ITEMS: usize = 500;
const MAX_CHART_ITEMS_PER_DIRECTORY: usize = 16;
const MAX_CHART_DEPTH: usize = 3;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub entries_scanned: u64,
    pub files_scanned: u64,
    pub directories_scanned: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub skipped_entries: u64,
    pub current_path: String,
    pub elapsed_ms: u64,
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
    pub items_truncated: bool,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub items: Vec<ScanItem>,
    pub chart_items: Vec<ChartItem>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
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
    pub duplicate_hard_links: u64,
    pub traversal_us: u64,
    pub aggregation_us: u64,
    pub indexing_us: u64,
    pub elapsed_ms: u64,
    pub allocated_size_is_estimate: bool,
    pub hard_link_deduplication_supported: bool,
    pub same_filesystem_enforced: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanBackend {
    Auto,
    Jwalk,
    Getattrlistbulk,
}

#[derive(Debug)]
pub(crate) struct ScanOutput {
    pub result: ScanResult,
    pub snapshot: ScanSnapshot,
}

#[derive(Debug)]
pub(crate) struct ScanSnapshot {
    root: NodeId,
    root_path: Arc<Path>,
    nodes: Vec<InternalNode>,
}

type NodeId = usize;

#[derive(Clone, Copy, Debug, Default)]
struct MeasuredMetadata {
    logical_bytes: u64,
    allocated_bytes: u64,
    filesystem_id: Option<u64>,
    file_identity: Option<FileIdentity>,
    metadata_error: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct FileIdentity(u64, u64);

#[derive(Debug, Default)]
struct ScanCounters {
    files_scanned: u64,
    directories_scanned: u64,
    skipped_entries: u64,
    skipped_filesystems: u64,
    duplicate_hard_links: u64,
    observed_logical_bytes: u64,
    observed_allocated_bytes: u64,
    seen_files: HashSet<FileIdentity>,
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
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    kind: EntryKind,
    logical_bytes: u64,
    allocated_bytes: u64,
    file_count: u64,
    directory_count: u64,
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
        }
    }

    fn name(&self) -> &OsStr {
        &self.name
    }
}

impl ScanCounters {
    fn push_node(
        &mut self,
        nodes: &mut Vec<InternalNode>,
        parent: NodeId,
        name: OsString,
        kind: EntryKind,
        measured: MeasuredMetadata,
    ) -> NodeId {
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

        if let Some(identity) = measured.file_identity
            && !self.seen_files.insert(identity)
        {
            self.duplicate_hard_links += 1;
            logical_bytes = 0;
            allocated_bytes = 0;
        }

        self.observed_logical_bytes = self.observed_logical_bytes.saturating_add(logical_bytes);
        self.observed_allocated_bytes = self
            .observed_allocated_bytes
            .saturating_add(allocated_bytes);

        let node_id = nodes.len();
        nodes[parent].children.push(node_id);
        nodes.push(InternalNode {
            name,
            parent: Some(parent),
            children: Vec::new(),
            kind,
            logical_bytes,
            allocated_bytes,
            file_count,
            directory_count,
        });
        node_id
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

pub(crate) fn scan_path_with_backend<F>(
    root: &Path,
    cancel: Arc<AtomicBool>,
    backend: ScanBackend,
    mut on_progress: F,
) -> Result<ScanOutput, String>
where
    F: FnMut(ScanProgress),
{
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
            scan_path_jwalk(root, cancel, on_progress)
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
    }
}

fn scan_path_jwalk<F>(
    root: &Path,
    cancel: Arc<AtomicBool>,
    mut on_progress: F,
) -> Result<ScanOutput, String>
where
    F: FnMut(ScanProgress),
{
    let root = root
        .canonicalize()
        .map_err(|error| format!("Could not open {}: {error}", root.display()))?;
    let root_metadata = root
        .metadata()
        .map_err(|error| format!("Could not read {}: {error}", root.display()))?;

    if !root_metadata.is_dir() {
        return Err("Choose a directory to scan.".to_string());
    }

    if cancel.load(Ordering::Relaxed) {
        return Err("Scan cancelled.".to_string());
    }

    let started_at = Instant::now();
    let root_filesystem = filesystem_id(&root_metadata);
    let root_node_path: Arc<Path> = Arc::from(root.clone());
    let mut nodes = vec![InternalNode::root(&root)];
    // jwalk reads directories in parallel but its public iterator yields them
    // depth-first. The stack therefore maps entry depth to a compact parent ID
    // without retaining a full-path lookup table.
    let mut ancestor_stack = vec![0];

    let mut counters = ScanCounters::default();
    let mut entries_since_progress = 0_u64;
    let mut last_progress_at = Instant::now();

    let worker_cancel = cancel.clone();
    let threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .clamp(2, 32);

    let walker = WalkDirGeneric::<((), Option<MeasuredMetadata>)>::new(&root)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(Parallelism::RayonNewPool(threads))
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
                        let measured = measure_metadata(&metadata, child.file_type.is_file());
                        if child.file_type.is_dir()
                            && root_filesystem.is_some()
                            && measured.filesystem_id != root_filesystem
                        {
                            child.read_children_path = None;
                        }
                        child.client_state = Some(measured);
                    }
                    Err(_) if child.file_type.is_dir() => {
                        child.read_children_path = None;
                        child.client_state = Some(MeasuredMetadata {
                            metadata_error: true,
                            ..MeasuredMetadata::default()
                        });
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
            Err(_) => {
                counters.skipped_entries += 1;
                continue;
            }
        };

        if entry.depth == 0 {
            if entry.read_children_error.is_some() {
                counters.skipped_entries += 1;
            }
            continue;
        }

        let measured = match entry.client_state {
            Some(measured) => measured,
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
        entries_since_progress += 1;
        let should_report_progress = entries_since_progress >= PROGRESS_ENTRY_INTERVAL
            || last_progress_at.elapsed() >= PROGRESS_INTERVAL;
        let current_path =
            should_report_progress.then(|| entry.path().to_string_lossy().into_owned());

        let node_id = counters.push_node(&mut nodes, parent, entry.file_name, kind, measured);
        debug_assert_eq!(node_id, nodes.len() - 1);
        if matches!(kind, EntryKind::Directory) {
            ancestor_stack.push(node_id);
        }

        if let Some(current_path) = current_path {
            on_progress(ScanProgress {
                entries_scanned: counters.files_scanned + counters.directories_scanned,
                files_scanned: counters.files_scanned,
                directories_scanned: counters.directories_scanned,
                logical_bytes: counters.observed_logical_bytes,
                allocated_bytes: counters.observed_allocated_bytes,
                skipped_entries: counters.skipped_entries,
                current_path,
                elapsed_ms: elapsed_ms(started_at),
            });
            entries_since_progress = 0;
            last_progress_at = Instant::now();
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
        "jwalk",
        ScanSemantics {
            allocated_size_is_estimate: !cfg!(unix),
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

    for node_id in (1..nodes.len()).rev() {
        if node_id % PROGRESS_ENTRY_INTERVAL as usize == 0 && cancel.load(Ordering::Relaxed) {
            return Err("Scan cancelled.".to_string());
        }
        let parent = nodes[node_id]
            .parent
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

    let aggregation_completed_at = Instant::now();
    let aggregation_us =
        duration_us(aggregation_completed_at.duration_since(traversal_completed_at));

    let root_totals = &nodes[0];
    let logical_bytes = root_totals.logical_bytes;
    let allocated_bytes = root_totals.allocated_bytes;
    let file_count = root_totals.file_count;
    let directory_count = root_totals.directory_count.saturating_sub(1);

    let indexing_completed_at = Instant::now();
    let indexing_us = duration_us(indexing_completed_at.duration_since(aggregation_completed_at));
    let elapsed_ms = duration_ms(indexing_completed_at.duration_since(started_at));

    on_progress(ScanProgress {
        entries_scanned: file_count + directory_count,
        files_scanned: file_count,
        directories_scanned: directory_count,
        logical_bytes,
        allocated_bytes,
        skipped_entries: counters.skipped_entries,
        current_path: root.to_string_lossy().into_owned(),
        elapsed_ms,
    });

    let snapshot = ScanSnapshot {
        root: 0,
        root_path: root_node_path,
        nodes,
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

impl ScanSnapshot {
    pub(crate) fn directory_view(
        &self,
        scan_id: u64,
        requested: u64,
    ) -> Result<DirectoryView, String> {
        let node_id = usize::try_from(requested)
            .ok()
            .filter(|node_id| *node_id < self.nodes.len())
            .ok_or_else(|| "That item is not part of this scan.".to_string())?;
        let node = &self.nodes[node_id];

        if !matches!(node.kind, EntryKind::Directory) {
            return Err("Only folders can be opened in the scan map.".to_string());
        }

        let total_items = node.children.len();
        let ranked_ids = self.ranked_child_ids(node_id, MAX_LIST_ITEMS);
        let items = ranked_ids
            .iter()
            .map(|child| self.scan_item(*child))
            .collect();
        let chart_item_count = ranked_ids.len().min(MAX_CHART_ITEMS_PER_DIRECTORY);

        Ok(DirectoryView {
            scan_id,
            node_id: wire_id(node_id),
            root: self.root_path.to_string_lossy().into_owned(),
            path: self.node_path(node_id).to_string_lossy().into_owned(),
            display_name: node.name().to_string_lossy().into_owned(),
            logical_bytes: node.logical_bytes,
            allocated_bytes: node.allocated_bytes,
            total_items,
            items_truncated: total_items > MAX_LIST_ITEMS,
            breadcrumbs: self.breadcrumbs(node_id),
            items,
            chart_items: self.chart_items(node_id, 0, &ranked_ids[..chart_item_count], total_items),
        })
    }

    fn scan_item(&self, node_id: NodeId) -> ScanItem {
        let node = &self.nodes[node_id];
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
            current = node.parent;
        }

        breadcrumbs.reverse();
        breadcrumbs
    }

    fn chart_children(&self, parent: NodeId, depth: usize) -> Vec<ChartItem> {
        if depth >= MAX_CHART_DEPTH {
            return Vec::new();
        }

        let total_items = self.nodes[parent].children.len();
        let ranked_ids = self.ranked_child_ids(parent, MAX_CHART_ITEMS_PER_DIRECTORY);
        self.chart_items(parent, depth, &ranked_ids, total_items)
    }

    fn chart_items(
        &self,
        parent: NodeId,
        depth: usize,
        ranked_ids: &[NodeId],
        total_items: usize,
    ) -> Vec<ChartItem> {
        let mut items: Vec<_> = ranked_ids
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
                        .then(|| self.chart_children(*node_id, depth + 1))
                        .unwrap_or_default(),
                }
            })
            .collect();

        if total_items > ranked_ids.len() {
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
            let selected = ranked_ids.iter().fold((0_u64, 0_u64), |total, node_id| {
                let node = &self.nodes[*node_id];
                (
                    total.0.saturating_add(node.logical_bytes),
                    total.1.saturating_add(node.allocated_bytes),
                )
            });
            items.push(ChartItem {
                id: None,
                name: format!("{} more items", total_items - ranked_ids.len()),
                kind: EntryKind::Other,
                logical_bytes: total.0.saturating_sub(selected.0),
                allocated_bytes: total.1.saturating_sub(selected.1),
                children: Vec::new(),
            });
        }

        items
    }

    fn ranked_child_ids(&self, parent: NodeId, limit: usize) -> Vec<NodeId> {
        let mut ranked = self.nodes[parent].children.clone();

        if ranked.len() > limit {
            ranked.select_nth_unstable_by(limit, |left, right| {
                compare_node_ids(&self.nodes, *left, *right)
            });
            ranked.truncate(limit);
        }
        ranked.sort_unstable_by(|left, right| compare_node_ids(&self.nodes, *left, *right));
        ranked
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
                .parent
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

fn compare_node_ids(nodes: &[InternalNode], left: NodeId, right: NodeId) -> std::cmp::Ordering {
    let left_node = &nodes[left];
    let right_node = &nodes[right];
    right_node
        .allocated_bytes
        .cmp(&left_node.allocated_bytes)
        .then_with(|| right_node.logical_bytes.cmp(&left_node.logical_bytes))
        .then_with(|| left_node.name().cmp(right_node.name()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_and_aggregates_a_directory_tree() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).expect("create nested directory");
        fs::write(temp.path().join("root.bin"), vec![1_u8; 17]).expect("write root file");
        fs::write(nested.join("child.bin"), vec![2_u8; 31]).expect("write nested file");

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
            } else {
                "jwalk"
            }
        );
        assert_eq!(result.logical_bytes, 48);
        assert_eq!(result.file_count, 2);
        assert_eq!(result.directory_count, 1);
        assert_eq!(progress.last().expect("final progress").logical_bytes, 48);
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
    fn honors_cancellation_before_traversal() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let cancel = Arc::new(AtomicBool::new(true));

        let error = scan_path(temp.path(), cancel, |_| {}).expect_err("scan must cancel");

        assert_eq!(error, "Scan cancelled.");
    }

    #[cfg(unix)]
    #[test]
    fn counts_hard_linked_content_once() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let original = temp.path().join("original.bin");
        fs::write(&original, vec![7_u8; 64]).expect("write fixture file");
        fs::hard_link(&original, temp.path().join("copy.bin")).expect("create hard link");

        let output =
            scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {}).expect("scan fixture");
        let result = output.result;

        assert_eq!(result.logical_bytes, 64);
        assert_eq!(result.file_count, 2);
        assert_eq!(result.duplicate_hard_links, 1);
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

    fn assert_arena_invariants(snapshot: &ScanSnapshot) {
        assert_eq!(snapshot.root, 0);
        assert!(snapshot.root_path.is_absolute());

        for (node_id, node) in snapshot.nodes.iter().enumerate() {
            if let Some(parent_id) = node.parent {
                assert!(parent_id < node_id);
                assert!(snapshot.nodes[parent_id].children.contains(&node_id));
            } else {
                assert_eq!(node_id, snapshot.root);
            }

            for child_id in &node.children {
                assert_eq!(snapshot.nodes[*child_id].parent, Some(node_id));
            }

            assert!(!node.name.is_empty());
        }
    }
}
