//! macOS scanner policy: never materialize APFS/File Provider placeholders.
//!
//! See Apple TN3150. Checking flags alone is racy, and even canonicalization can
//! materialize an intermediate directory. Install the policy before any path IO,
//! on the caller AND every worker; restore it before returning a borrowed thread.
use std::io;
use std::marker::PhantomData;
use std::os::macos::fs::MetadataExt;
use std::rc::Rc;
use std::sync::{Arc, mpsc};

// Public Darwin sys/resource.h and sys/stat.h constants absent from libc.
const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: i32 = 3;
const IOPOL_SCOPE_THREAD: i32 = 1;
const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: i32 = 1;
pub(super) const SF_DATALESS: u32 = 0x4000_0000;

unsafe extern "C" {
    fn getiopolicy_np(iotype: i32, scope: i32) -> i32;
    fn setiopolicy_np(iotype: i32, scope: i32, policy: i32) -> i32;
}

pub(super) struct NoMaterialization {
    previous: i32,
    // A thread-local policy must never be restored on a different thread.
    _thread_bound: PhantomData<Rc<()>>,
}

fn policy() -> io::Result<i32> {
    // SAFETY: these constants select the calling thread's public IO policy.
    let value = unsafe {
        getiopolicy_np(
            IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
            IOPOL_SCOPE_THREAD,
        )
    };
    if value < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value)
    }
}

fn set_policy(value: i32) -> io::Result<()> {
    // SAFETY: no pointers; only the calling thread is affected.
    if unsafe {
        setiopolicy_np(
            IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
            IOPOL_SCOPE_THREAD,
            value,
        )
    } < 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl NoMaterialization {
    pub(super) fn new() -> io::Result<Self> {
        let previous = policy()?;
        set_policy(IOPOL_MATERIALIZE_DATALESS_FILES_OFF)?;
        Ok(Self {
            previous,
            _thread_bound: PhantomData,
        })
    }
}

impl Drop for NoMaterialization {
    fn drop(&mut self) {
        // A failed restoration leaves the more restrictive policy in place.
        let _ = set_policy(self.previous);
    }
}

pub(super) fn is_dataless(metadata: &std::fs::Metadata) -> bool {
    metadata.st_flags() & SF_DATALESS != 0
}

pub(super) fn materialization_blocked(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::EDEADLK)
}

pub(super) fn protection_error(error: impl std::fmt::Display) -> String {
    format!("Could not prevent cloud downloads while scanning: {error}")
}

pub(super) fn worker_pool(threads: usize) -> Result<Arc<rayon::ThreadPool>, String> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .spawn_handler(|worker| {
            let (sender, receiver) = mpsc::sync_channel(1);
            std::thread::Builder::new().spawn(move || {
                let guard = NoMaterialization::new();
                match guard {
                    Ok(_guard) => {
                        if sender.send(Ok(())).is_ok() {
                            worker.run();
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                    }
                }
            })?;
            // Fail pool construction before any traversal if protection failed.
            receiver.recv().map_err(io::Error::other)?
        })
        .build()
        .map(Arc::new)
        .map_err(protection_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{ScanBackend, scan_path_with_backend, validate_scan_root};
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn restores_thread_policy_including_nested_and_unwinding_scopes() {
        let before = policy().unwrap();
        let _ = std::panic::catch_unwind(|| {
            let _outer = NoMaterialization::new().unwrap();
            {
                let _inner = NoMaterialization::new().unwrap();
            }
            assert_eq!(policy().unwrap(), IOPOL_MATERIALIZE_DATALESS_FILES_OFF);
            panic!("exercise unwinding");
        });
        assert_eq!(policy().unwrap(), before);
    }

    #[test]
    fn every_fallback_worker_has_no_materialization_policy() {
        let before = policy().unwrap();
        let pool = worker_pool(4).unwrap();
        let policies = pool.broadcast(|_| policy().unwrap());
        assert_eq!(policies, vec![IOPOL_MATERIALIZE_DATALESS_FILES_OFF; 4]);
        assert_eq!(policy().unwrap(), before);
    }

    #[test]
    fn only_materialization_refusal_is_a_cloud_exclusion() {
        assert!(materialization_blocked(&io::Error::from_raw_os_error(
            libc::EDEADLK
        )));
        assert!(!materialization_blocked(&io::Error::from_raw_os_error(
            libc::EACCES
        )));
        assert!(!materialization_blocked(&io::Error::from_raw_os_error(
            libc::ENOENT
        )));
    }

    #[test]
    fn local_files_inside_provider_named_folders_are_counted_by_both_backends() {
        let temp = tempfile::tempdir().unwrap();
        for folder in [
            "Library/Mobile Documents/iCloud~books",
            "Library/CloudStorage/GoogleDrive-account",
        ] {
            let path = temp.path().join(folder);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("downloaded.txt"), b"locally downloaded").unwrap();
        }
        let before = policy().unwrap();
        let native = scan_path_with_backend(
            temp.path(),
            Arc::new(AtomicBool::new(false)),
            ScanBackend::Getattrlistbulk,
            |_| {},
        )
        .unwrap();
        let fallback = scan_path_with_backend(
            temp.path(),
            Arc::new(AtomicBool::new(false)),
            ScanBackend::Jwalk,
            |_| {},
        )
        .unwrap();
        assert!(
            native
                .result
                .accounting_mismatches(&fallback.result)
                .is_empty()
        );
        assert_eq!(native.result.file_count, 2);
        assert_eq!(native.result.logical_bytes, 36);
        assert_eq!(native.result.skipped_cloud_entries, 0);
        assert!(native.result.allocated_bytes > 0);
        assert_eq!(policy().unwrap(), before);
    }

    /// Opt in with a small, already-synced directory containing evicted items.
    /// Does not create, evict, download, or modify any provider files.
    #[test]
    #[ignore = "requires CEPA_CLOUD_FIXTURE_ROOT containing real dataless entries"]
    fn real_cloud_placeholders_remain_evicted_on_both_backends() {
        let before = policy().unwrap();
        let root = std::env::var_os("CEPA_CLOUD_FIXTURE_ROOT").expect("cloud fixture root");
        let root = Path::new(&root);
        let placeholders: Vec<_> = {
            let _guard = NoMaterialization::new().unwrap();
            std::fs::read_dir(root)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter_map(|path| {
                    let metadata = std::fs::symlink_metadata(&path).unwrap();
                    is_dataless(&metadata).then_some((path, metadata))
                })
                .collect()
        };
        assert!(
            !placeholders.is_empty(),
            "fixture must contain evicted entries"
        );
        if let Some((directory, _)) = placeholders.iter().find(|(_, metadata)| metadata.is_dir()) {
            super::super::macos::assert_worker_blocks_evicted_directory(directory);
        }
        for (path, metadata) in &placeholders {
            if metadata.is_dir() {
                assert!(validate_scan_root(path).is_err());
                // Even resolving a descendant must fail without materialization.
                assert!(validate_scan_root(&path.join("unavailable-child")).is_err());
            }
        }
        for backend in [ScanBackend::Getattrlistbulk, ScanBackend::Jwalk] {
            for (path, metadata) in &placeholders {
                if metadata.is_dir() {
                    assert!(
                        scan_path_with_backend(
                            path,
                            Arc::new(AtomicBool::new(false)),
                            backend,
                            |_| {}
                        )
                        .is_err()
                    );
                }
            }
            let started = std::time::Instant::now();
            let scan =
                scan_path_with_backend(root, Arc::new(AtomicBool::new(false)), backend, |_| {})
                    .unwrap();
            assert!(scan.result.skipped_cloud_entries >= placeholders.len() as u64);
            assert_eq!(policy().unwrap(), before);
            for (path, original) in &placeholders {
                let _guard = NoMaterialization::new().unwrap();
                let after = std::fs::symlink_metadata(path).unwrap();
                assert!(is_dataless(&after), "materialized {}", path.display());
                assert_eq!(after.st_dev(), original.st_dev());
                assert_eq!(after.st_ino(), original.st_ino());
                // APFS can refresh a dataless directory's synthetic st_size
                // after a blocked enumeration. Directory length is not payload
                // size; only regular-file lengths are comparable here.
                if original.is_file() {
                    assert_eq!(after.st_size(), original.st_size());
                }
                assert_eq!(after.st_blocks(), original.st_blocks());
                assert!(
                    !scan
                        .snapshot
                        .nodes
                        .iter()
                        .any(|node| node.name == path.file_name().unwrap())
                );
            }
            eprintln!(
                "{backend}: {} cloud exclusions, {} local files, {} allocated bytes, {:?}",
                scan.result.skipped_cloud_entries,
                scan.result.file_count,
                scan.result.allocated_bytes,
                started.elapsed()
            );
        }
    }
}
