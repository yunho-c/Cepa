use jwalk::{Parallelism, WalkDirGeneric};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub file_count: u64,
    pub directory_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breadcrumb {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartItem {
    pub name: String,
    pub path: Option<String>,
    pub kind: EntryKind,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub children: Vec<ChartItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryView {
    pub scan_id: u64,
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
    pub elapsed_ms: u64,
    pub allocated_size_is_estimate: bool,
    pub hard_link_deduplication_supported: bool,
    pub same_filesystem_enforced: bool,
}

#[derive(Debug)]
pub(crate) struct ScanOutput {
    pub result: ScanResult,
    pub snapshot: ScanSnapshot,
}

#[derive(Debug)]
pub(crate) struct ScanSnapshot {
    root: PathBuf,
    nodes: HashMap<PathBuf, InternalNode>,
    children: HashMap<PathBuf, Vec<PathBuf>>,
}

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

#[derive(Debug)]
struct InternalNode {
    name: OsString,
    parent: Option<PathBuf>,
    depth: usize,
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
            depth: 0,
            kind: EntryKind::Directory,
            logical_bytes: 0,
            allocated_bytes: 0,
            file_count: 0,
            directory_count: 1,
        }
    }
}

pub(crate) fn scan_path<F>(
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
    let mut nodes = HashMap::new();
    nodes.insert(root.clone(), InternalNode::root(&root));

    let mut seen_files = HashSet::new();
    let mut files_scanned = 0_u64;
    let mut directories_scanned = 0_u64;
    let mut skipped_entries = 0_u64;
    let mut skipped_filesystems = 0_u64;
    let mut duplicate_hard_links = 0_u64;
    let mut observed_logical_bytes = 0_u64;
    let mut observed_allocated_bytes = 0_u64;
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
                skipped_entries += 1;
                continue;
            }
        };

        if entry.depth == 0 {
            if entry.read_children_error.is_some() {
                skipped_entries += 1;
            }
            continue;
        }

        let path = entry.path();
        let measured = match entry.client_state {
            Some(measured) => measured,
            None => {
                skipped_entries += 1;
                continue;
            }
        };

        if measured.metadata_error {
            skipped_entries += 1;
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

        let (mut logical_bytes, mut allocated_bytes, file_count, directory_count) = match kind {
            EntryKind::Directory => {
                directories_scanned += 1;
                (0, 0, 0, 1)
            }
            EntryKind::File => {
                files_scanned += 1;
                (measured.logical_bytes, measured.allocated_bytes, 1, 0)
            }
            EntryKind::Symlink | EntryKind::Other => (0, 0, 0, 0),
        };

        if is_other_filesystem {
            skipped_filesystems += 1;
        }

        if let Some(identity) = measured.file_identity
            && !seen_files.insert(identity)
        {
            duplicate_hard_links += 1;
            logical_bytes = 0;
            allocated_bytes = 0;
        }

        observed_logical_bytes = observed_logical_bytes.saturating_add(logical_bytes);
        observed_allocated_bytes = observed_allocated_bytes.saturating_add(allocated_bytes);

        if entry.read_children_error.is_some() {
            skipped_entries += 1;
        }

        nodes.insert(
            path.clone(),
            InternalNode {
                name: entry.file_name,
                parent: Some(entry.parent_path.to_path_buf()),
                depth: entry.depth,
                kind,
                logical_bytes,
                allocated_bytes,
                file_count,
                directory_count,
            },
        );

        entries_since_progress += 1;
        if entries_since_progress >= PROGRESS_ENTRY_INTERVAL
            || last_progress_at.elapsed() >= PROGRESS_INTERVAL
        {
            on_progress(ScanProgress {
                entries_scanned: files_scanned + directories_scanned,
                files_scanned,
                directories_scanned,
                logical_bytes: observed_logical_bytes,
                allocated_bytes: observed_allocated_bytes,
                skipped_entries,
                current_path: path.to_string_lossy().into_owned(),
                elapsed_ms: elapsed_ms(started_at),
            });
            entries_since_progress = 0;
            last_progress_at = Instant::now();
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return Err("Scan cancelled.".to_string());
    }

    let mut paths_by_depth: Vec<_> = nodes
        .iter()
        .map(|(path, node)| (node.depth, path.clone()))
        .collect();
    paths_by_depth.sort_unstable_by(|left, right| right.0.cmp(&left.0));

    for (_, path) in paths_by_depth {
        let Some(node) = nodes.get(&path) else {
            continue;
        };
        let Some(parent_path) = node.parent.clone() else {
            continue;
        };
        let totals = (
            node.logical_bytes,
            node.allocated_bytes,
            node.file_count,
            node.directory_count,
        );

        if let Some(parent) = nodes.get_mut(&parent_path) {
            parent.logical_bytes = parent.logical_bytes.saturating_add(totals.0);
            parent.allocated_bytes = parent.allocated_bytes.saturating_add(totals.1);
            parent.file_count = parent.file_count.saturating_add(totals.2);
            parent.directory_count = parent.directory_count.saturating_add(totals.3);
        }
    }

    let root_totals = nodes
        .get(&root)
        .ok_or_else(|| "The scan did not produce a root result.".to_string())?;
    let logical_bytes = root_totals.logical_bytes;
    let allocated_bytes = root_totals.allocated_bytes;
    let file_count = root_totals.file_count;
    let directory_count = root_totals.directory_count.saturating_sub(1);

    let elapsed_ms = elapsed_ms(started_at);
    on_progress(ScanProgress {
        entries_scanned: file_count + directory_count,
        files_scanned: file_count,
        directories_scanned: directory_count,
        logical_bytes,
        allocated_bytes,
        skipped_entries,
        current_path: root.to_string_lossy().into_owned(),
        elapsed_ms,
    });

    let mut children: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for (path, node) in &nodes {
        if let Some(parent) = &node.parent {
            children
                .entry(parent.clone())
                .or_default()
                .push(path.clone());
        }
    }

    for child_paths in children.values_mut() {
        child_paths.sort_unstable_by(|left, right| compare_node_paths(&nodes, left, right));
    }

    let snapshot = ScanSnapshot {
        root: root.clone(),
        nodes,
        children,
    };

    Ok(ScanOutput {
        result: ScanResult {
            root: root.to_string_lossy().into_owned(),
            display_name: root
                .file_name()
                .unwrap_or(root.as_os_str())
                .to_string_lossy()
                .into_owned(),
            backend: "jwalk",
            logical_bytes,
            allocated_bytes,
            file_count,
            directory_count,
            skipped_entries,
            skipped_filesystems,
            duplicate_hard_links,
            elapsed_ms,
            allocated_size_is_estimate: !cfg!(unix),
            hard_link_deduplication_supported: cfg!(unix),
            same_filesystem_enforced: root_filesystem.is_some(),
        },
        snapshot,
    })
}

impl ScanSnapshot {
    pub(crate) fn root_path(&self) -> &Path {
        &self.root
    }

    pub(crate) fn directory_view(
        &self,
        scan_id: u64,
        requested: &str,
    ) -> Result<DirectoryView, String> {
        let path = PathBuf::from(requested);
        let node = self
            .nodes
            .get(&path)
            .ok_or_else(|| "That folder is not part of this scan.".to_string())?;

        if !matches!(node.kind, EntryKind::Directory) {
            return Err("Only folders can be opened in the scan map.".to_string());
        }

        let child_paths = self
            .children
            .get(&path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let total_items = child_paths.len();
        let items = child_paths
            .iter()
            .take(MAX_LIST_ITEMS)
            .filter_map(|child| self.scan_item(child))
            .collect();

        Ok(DirectoryView {
            scan_id,
            root: self.root.to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            display_name: node.name.to_string_lossy().into_owned(),
            logical_bytes: node.logical_bytes,
            allocated_bytes: node.allocated_bytes,
            total_items,
            items_truncated: total_items > MAX_LIST_ITEMS,
            breadcrumbs: self.breadcrumbs(&path),
            items,
            chart_items: self.chart_children(&path, 0),
        })
    }

    fn scan_item(&self, path: &Path) -> Option<ScanItem> {
        let node = self.nodes.get(path)?;
        Some(ScanItem {
            name: node.name.to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            kind: node.kind,
            logical_bytes: node.logical_bytes,
            allocated_bytes: node.allocated_bytes,
            file_count: node.file_count,
            directory_count: node.directory_count,
        })
    }

    fn breadcrumbs(&self, path: &Path) -> Vec<Breadcrumb> {
        let mut breadcrumbs = Vec::new();
        let mut current = Some(path);

        while let Some(current_path) = current {
            let Some(node) = self.nodes.get(current_path) else {
                break;
            };
            breadcrumbs.push(Breadcrumb {
                name: node.name.to_string_lossy().into_owned(),
                path: current_path.to_string_lossy().into_owned(),
            });
            if current_path == self.root {
                break;
            }
            current = node.parent.as_deref();
        }

        breadcrumbs.reverse();
        breadcrumbs
    }

    fn chart_children(&self, parent: &Path, depth: usize) -> Vec<ChartItem> {
        if depth >= MAX_CHART_DEPTH {
            return Vec::new();
        }

        let child_paths = self
            .children
            .get(parent)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut items: Vec<_> = child_paths
            .iter()
            .take(MAX_CHART_ITEMS_PER_DIRECTORY)
            .filter_map(|path| {
                let node = self.nodes.get(path)?;
                Some(ChartItem {
                    name: node.name.to_string_lossy().into_owned(),
                    path: Some(path.to_string_lossy().into_owned()),
                    kind: node.kind,
                    logical_bytes: node.logical_bytes,
                    allocated_bytes: node.allocated_bytes,
                    children: matches!(node.kind, EntryKind::Directory)
                        .then(|| self.chart_children(path, depth + 1))
                        .unwrap_or_default(),
                })
            })
            .collect();

        if child_paths.len() > MAX_CHART_ITEMS_PER_DIRECTORY {
            let omitted = child_paths
                .iter()
                .skip(MAX_CHART_ITEMS_PER_DIRECTORY)
                .filter_map(|path| self.nodes.get(path))
                .fold((0_u64, 0_u64), |total, node| {
                    (
                        total.0.saturating_add(node.logical_bytes),
                        total.1.saturating_add(node.allocated_bytes),
                    )
                });
            items.push(ChartItem {
                name: format!(
                    "{} more items",
                    child_paths.len() - MAX_CHART_ITEMS_PER_DIRECTORY
                ),
                path: None,
                kind: EntryKind::Other,
                logical_bytes: omitted.0,
                allocated_bytes: omitted.1,
                children: Vec::new(),
            });
        }

        items
    }
}

fn compare_node_paths(
    nodes: &HashMap<PathBuf, InternalNode>,
    left: &PathBuf,
    right: &PathBuf,
) -> std::cmp::Ordering {
    let left_node = &nodes[left];
    let right_node = &nodes[right];
    right_node
        .allocated_bytes
        .cmp(&left_node.allocated_bytes)
        .then_with(|| right_node.logical_bytes.cmp(&left_node.logical_bytes))
        .then_with(|| left_node.name.cmp(&right_node.name))
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
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

        assert_eq!(result.backend, "jwalk");
        assert_eq!(result.logical_bytes, 48);
        assert_eq!(result.file_count, 2);
        assert_eq!(result.directory_count, 1);
        assert_eq!(progress.last().expect("final progress").logical_bytes, 48);

        let view = output
            .snapshot
            .directory_view(7, &result.root)
            .expect("build root view");
        assert_eq!(view.scan_id, 7);
        assert_eq!(view.items.len(), 2);
        assert_eq!(view.items[0].logical_bytes, 31);
        assert_eq!(view.items[0].file_count, 1);
        assert_eq!(view.chart_items.len(), 2);

        let nested_path = view
            .items
            .iter()
            .find(|item| item.name == "nested")
            .expect("nested directory item")
            .path
            .clone();
        let nested_view = output
            .snapshot
            .directory_view(7, &nested_path)
            .expect("open nested view");
        assert_eq!(nested_view.items.len(), 1);
        assert_eq!(nested_view.items[0].name, "child.bin");
        assert_eq!(nested_view.breadcrumbs.len(), 2);
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
            .directory_view(1, &output.result.root)
            .expect("build root view");

        assert_eq!(output.result.logical_bytes, 0);
        assert_eq!(output.result.file_count, 0);
        assert_eq!(view.items.len(), 1);
        assert!(matches!(view.items[0].kind, EntryKind::Symlink));
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
            .directory_view(1, &output.result.root)
            .expect("build root view");

        assert_eq!(view.total_items, MAX_LIST_ITEMS + 1);
        assert_eq!(view.items.len(), MAX_LIST_ITEMS);
        assert!(view.items_truncated);
        assert_eq!(view.chart_items.len(), MAX_CHART_ITEMS_PER_DIRECTORY + 1);
        assert!(
            view.chart_items
                .last()
                .expect("aggregate item")
                .path
                .is_none()
        );
    }
}
