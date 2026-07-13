use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use sysinfo::Disks;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScanRoot {
    name: String,
    path: String,
    display_path: String,
    total_bytes: u64,
    available_bytes: u64,
    is_removable: bool,
    is_read_only: bool,
}

impl ScanRoot {
    pub(crate) fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug)]
struct RawScanRoot {
    name: String,
    path: PathBuf,
    display_path: Option<PathBuf>,
    total_bytes: u64,
    available_bytes: u64,
    is_removable: bool,
    is_read_only: bool,
}

pub(crate) fn discover_scan_roots() -> Vec<ScanRoot> {
    let disks = Disks::new_with_refreshed_list();
    let roots = disks
        .list()
        .iter()
        .map(|disk| RawScanRoot {
            name: disk.name().to_string_lossy().into_owned(),
            path: disk.mount_point().to_path_buf(),
            display_path: None,
            total_bytes: disk.total_space(),
            available_bytes: disk.available_space(),
            is_removable: disk.is_removable(),
            is_read_only: disk.is_read_only(),
        })
        .collect::<Vec<_>>();
    #[cfg(target_os = "macos")]
    let roots = {
        let mut roots = roots;
        collapse_macos_data_volume(&mut roots);
        roots
    };
    normalize_scan_roots(roots, std::env::current_exe().ok().as_deref())
}

fn normalize_scan_roots(
    roots: impl IntoIterator<Item = RawScanRoot>,
    current_executable: Option<&Path>,
) -> Vec<ScanRoot> {
    let mut unique = BTreeMap::<PathBuf, RawScanRoot>::new();
    for root in roots {
        if root.total_bytes == 0 || !root.path.is_absolute() {
            continue;
        }
        unique
            .entry(root.path.clone())
            .and_modify(|existing| {
                if root.total_bytes > existing.total_bytes {
                    *existing = root.clone();
                }
            })
            .or_insert(root);
    }

    let roots = unique.into_values().collect::<Vec<_>>();
    let system_path = current_executable
        .and_then(|executable| {
            roots
                .iter()
                .filter(|root| {
                    executable.starts_with(root.display_path.as_deref().unwrap_or(&root.path))
                })
                .max_by_key(|root| root.path.components().count())
                .map(|root| root.path.clone())
        })
        .or_else(|| {
            roots
                .iter()
                .find(|root| root.path == Path::new("/"))
                .map(|root| root.path.clone())
        });

    let mut normalized = roots
        .into_iter()
        .map(|root| {
            let is_system = system_path.as_deref() == Some(root.path.as_path());
            ScanRoot {
                name: display_name(
                    &root.name,
                    root.display_path.as_deref().unwrap_or(&root.path),
                    is_system,
                ),
                path: root.path.to_string_lossy().into_owned(),
                display_path: root
                    .display_path
                    .as_deref()
                    .unwrap_or(&root.path)
                    .to_string_lossy()
                    .into_owned(),
                total_bytes: root.total_bytes,
                available_bytes: root.available_bytes.min(root.total_bytes),
                is_removable: root.is_removable,
                is_read_only: root.is_read_only,
            }
        })
        .collect::<Vec<_>>();

    normalized.sort_by(|left, right| {
        let left_is_system = system_path
            .as_ref()
            .is_some_and(|path| left.path == path.to_string_lossy());
        let right_is_system = system_path
            .as_ref()
            .is_some_and(|path| right.path == path.to_string_lossy());
        right_is_system
            .cmp(&left_is_system)
            .then_with(|| left.is_removable.cmp(&right.is_removable))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    normalized
}

fn collapse_macos_data_volume(roots: &mut Vec<RawScanRoot>) {
    let system_index = roots.iter().position(|root| root.path == Path::new("/"));
    let data_index = roots
        .iter()
        .position(|root| root.path == Path::new("/System/Volumes/Data"));
    let (Some(system_index), Some(data_index)) = (system_index, data_index) else {
        return;
    };
    let system = &roots[system_index];
    let data = &roots[data_index];
    if system.name != data.name || system.total_bytes != data.total_bytes {
        return;
    }

    roots[data_index].display_path = Some(PathBuf::from("/"));
    roots.remove(system_index);
}

fn display_name(device_name: &str, mount_point: &Path, is_system: bool) -> String {
    let device_name = device_name.trim();
    let mount_display = mount_point.to_string_lossy();
    let is_native_device_name = device_name.starts_with("/dev/")
        || device_name.starts_with(r"\\?\Volume{")
        || device_name.eq_ignore_ascii_case(&mount_display);

    if !device_name.is_empty() && !is_native_device_name {
        return device_name.to_string();
    }
    if let Some(name) = mount_point.file_name().filter(|name| !name.is_empty()) {
        return name.to_string_lossy().into_owned();
    }
    if is_system {
        return "System".to_string();
    }

    mount_display.trim_end_matches(['/', '\\']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(path: &str) -> PathBuf {
        #[cfg(windows)]
        {
            if path.starts_with('/') {
                let path = path.trim_start_matches('/').replace('/', r"\");
                if path.is_empty() {
                    PathBuf::from(r"C:\")
                } else {
                    PathBuf::from(format!(r"C:\{path}"))
                }
            } else {
                PathBuf::from(path)
            }
        }
        #[cfg(not(windows))]
        PathBuf::from(path)
    }

    fn raw(name: &str, path: &str, total_bytes: u64) -> RawScanRoot {
        RawScanRoot {
            name: name.to_string(),
            path: test_path(path),
            display_path: None,
            total_bytes,
            available_bytes: total_bytes / 4,
            is_removable: false,
            is_read_only: false,
        }
    }

    #[test]
    fn filters_unusable_roots_and_deduplicates_mount_points() {
        let roots = normalize_scan_roots(
            [
                raw("System", "/", 1_000),
                raw("Duplicate", "/", 900),
                raw("Empty", "/empty", 0),
                raw("Relative", "relative", 500),
            ],
            Some(&test_path("/Applications/Cepa.app/Contents/MacOS/cepa")),
        );

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].name, "System");
        assert_eq!(roots[0].path, test_path("/").to_string_lossy());
    }

    #[test]
    fn keeps_the_system_root_first_and_external_roots_after_internal_storage() {
        let mut archive = raw("Archive", "/Volumes/Archive", 800);
        archive.is_removable = true;
        let roots = normalize_scan_roots(
            [
                archive,
                raw("Projects", "/Volumes/Projects", 900),
                raw("/dev/disk3s1", "/", 1_000),
            ],
            Some(&test_path("/Applications/Cepa.app/Contents/MacOS/cepa")),
        );

        assert_eq!(
            roots
                .iter()
                .map(|root| root.name.as_str())
                .collect::<Vec<_>>(),
            ["System", "Projects", "Archive"]
        );
    }

    #[test]
    fn uses_a_human_volume_name_then_falls_back_to_the_mount_name() {
        assert_eq!(
            display_name("Macintosh HD", Path::new("/"), true),
            "Macintosh HD"
        );
        assert_eq!(
            display_name("/dev/disk4s2", Path::new("/Volumes/Camera"), false),
            "Camera"
        );
    }

    #[test]
    fn clamps_inconsistent_available_space_and_preserves_read_only_state() {
        let mut root = raw("Archive", "/Volumes/Archive", 100);
        root.available_bytes = 120;
        root.is_read_only = true;
        let roots = normalize_scan_roots([root], None);

        assert_eq!(roots[0].available_bytes, 100);
        assert!(roots[0].is_read_only);
    }

    #[test]
    fn wire_contract_uses_compact_camel_case_fields() {
        let root = ScanRoot {
            name: "System".into(),
            path: "/".into(),
            display_path: "/".into(),
            total_bytes: 1_000,
            available_bytes: 250,
            is_removable: false,
            is_read_only: false,
        };
        let wire = serde_json::to_value(root).expect("serialize scan root");

        assert_eq!(wire["totalBytes"], 1_000);
        assert_eq!(wire["availableBytes"], 250);
        assert_eq!(wire["displayPath"], "/");
        assert_eq!(wire["isRemovable"], false);
        assert!(wire.get("filesystem").is_none());
    }

    #[test]
    fn discovers_at_least_one_scan_root_on_a_supported_desktop_host() {
        let roots = discover_scan_roots();

        assert!(
            !roots.is_empty(),
            "the host did not report any storage roots"
        );
        assert!(roots.iter().all(|root| root.total_bytes > 0));
        assert!(roots.iter().all(|root| Path::new(&root.path).is_absolute()));
    }

    #[test]
    fn collapses_the_macos_system_and_data_pair_without_losing_the_scan_path() {
        let mut system = raw("Macintosh HD", "/", 1_000);
        system.path = PathBuf::from("/");
        system.available_bytes = 251;
        system.is_read_only = true;
        let mut data = raw("Macintosh HD", "/System/Volumes/Data", 1_000);
        data.path = PathBuf::from("/System/Volumes/Data");
        data.available_bytes = 250;
        let mut roots = vec![system, data];

        collapse_macos_data_volume(&mut roots);

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, Path::new("/System/Volumes/Data"));
        assert_eq!(roots[0].display_path.as_deref(), Some(Path::new("/")));
    }
}
