use super::*;
use crate::scanner::{ScanBackend, scan_path_with_backend, windows, windows_walk};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use windows_sys::Win32::Storage::CloudFilters::*;

#[test]
fn distinguishes_recall_from_pin_intent_and_extended_attributes() {
    for flag in [FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS] {
        assert!(needs_recall(flag));
        assert!(enumeration_needs_recall(flag | FILE_ATTRIBUTE_DIRECTORY));
    }
    for flag in [
        FILE_ATTRIBUTE_PINNED,
        FILE_ATTRIBUTE_UNPINNED,
        FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ATTRIBUTE_EA,
    ] {
        assert!(!needs_recall(flag));
    }
    assert!(enumeration_needs_recall(FILE_ATTRIBUTE_RECALL_ON_OPEN));
    for variant in 0..=15 {
        let tag = IO_REPARSE_TAG_CLOUD | (variant << 12);
        assert_eq!(classify(FILE_ATTRIBUTE_REPARSE_POINT, tag), EntryKind::File);
        assert_eq!(
            classify(FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY, tag),
            EntryKind::Directory
        );
    }
    assert_eq!(
        classify(
            FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY,
            0xA0000003
        ),
        EntryKind::Symlink
    );
}

#[test]
fn placeholder_mode_is_restored_after_nested_scopes_and_unwinding() {
    let original = unsafe { RtlSetThreadPlaceholderCompatibilityMode(1) };
    assert!(original >= 0);
    let _ = std::panic::catch_unwind(|| {
        let _outer = PlaceholderMode::new().unwrap();
        {
            let _inner = PlaceholderMode::new().unwrap();
        }
        assert_eq!(unsafe { RtlSetThreadPlaceholderCompatibilityMode(2) }, 2);
        panic!("exercise restoration");
    });
    assert_eq!(
        unsafe { RtlSetThreadPlaceholderCompatibilityMode(original) },
        1
    );
}

#[test]
fn relative_open_rejects_paths_and_keeps_the_original_parent() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    std::fs::create_dir(&original).unwrap();
    std::fs::write(original.join("child"), b"first").unwrap();
    let (_, parent) = open_root(&original).unwrap();
    for name in ["..", ".", "../child", "child\\nested", "file:stream"] {
        assert!(open_child(&parent, OsStr::new(name), false).is_err());
    }
    std::fs::rename(&original, temp.path().join("moved")).unwrap();
    std::fs::create_dir(&original).unwrap();
    std::fs::write(original.join("child"), b"replacement").unwrap();
    let child = open_child(&parent, OsStr::new("child"), false).unwrap();
    assert_eq!(child.metadata().unwrap().len(), 5);
}

#[test]
fn downloaded_provider_named_files_and_hard_links_are_counted() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["OneDrive", "Google Drive"] {
        let folder = temp.path().join(name);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("z-file"), [7; 31]).unwrap();
        std::fs::hard_link(folder.join("z-file"), folder.join("a-file")).unwrap();
    }
    let output = scan_path_with_backend(
        temp.path(),
        Arc::new(AtomicBool::new(false)),
        ScanBackend::Win32,
        |_| {},
    )
    .unwrap();
    assert_eq!(output.result.logical_bytes, 62);
    assert_eq!(output.result.file_count, 4);
    assert_eq!(output.result.duplicate_hard_links, 2);
    assert_eq!(output.result.skipped_cloud_entries, 0);
    assert_eq!(output.result.skipped_entries, 0);
}

#[test]
#[ignore = "read-only guard check of CEPA_GOOGLE_DRIVE_ROOT on an existing streaming volume"]
fn google_drive_stream_volume_is_rejected_before_traversal() {
    let root = std::env::var_os("CEPA_GOOGLE_DRIVE_ROOT").expect("set CEPA_GOOGLE_DRIVE_ROOT");
    let root = Path::new(&root);
    assert!(
        is_google_drive_root(root),
        "fixture must be a Google streaming volume"
    );
    for path in [
        root.to_path_buf(),
        root.join("My Drive"),
        root.join("nonexistent-test-child"),
    ] {
        let error = crate::scanner::validate_scan_root(&path).unwrap_err();
        assert!(error.contains("streaming drive does not expose"), "{error}");
        for backend in [
            ScanBackend::Auto,
            ScanBackend::Win32,
            ScanBackend::Jwalk,
            ScanBackend::Mft,
        ] {
            let mut progress = 0;
            let result =
                scan_path_with_backend(&path, Arc::new(AtomicBool::new(false)), backend, |_| {
                    progress += 1
                });
            let error = match result {
                Ok(_) => panic!("streaming volume was scanned"),
                Err(e) => e,
            };
            assert!(
                error.contains("streaming drive does not expose"),
                "{backend}: {error}"
            );
            assert_eq!(progress, 0, "{backend} traversed a virtual stream");
        }
    }
    assert!(
        crate::scan_roots::discover_scan_roots()
            .iter()
            .all(|item| !is_google_drive_root(item.path()))
    );
    eprintln!(
        "Google Drive volume and descendants rejected by validation and all 4 Windows selectors with zero progress; streaming volume absent from discovered roots"
    );
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn hr(value: i32) {
    assert!(value >= 0, "Cloud Files HRESULT {value:#x}");
}
struct Fixture {
    directory: tempfile::TempDir,
    path: Vec<u16>,
    connection: CF_CONNECTION_KEY,
    callbacks: Box<AtomicUsize>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            CfDisconnectSyncRoot(self.connection);
            CfUnregisterSyncRoot(self.path.as_ptr());
        }
    }
}

unsafe extern "system" fn fetch_data(
    info: *const CF_CALLBACK_INFO,
    parameters: *const CF_CALLBACK_PARAMETERS,
) {
    // Keep the fixture provider connected so any accidental recall is observed,
    // then fail immediately instead of downloading data or hanging the test.
    let (info, parameters) = unsafe { (&*info, &*parameters) };
    let count = unsafe { &*info.CallbackContext.cast::<AtomicUsize>() };
    count.fetch_add(1, Ordering::SeqCst);
    let op = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        RequestKey: info.RequestKey,
        ..Default::default()
    };
    let fetch = unsafe { parameters.Anonymous.FetchData };
    let mut params = CF_OPERATION_PARAMETERS {
        ParamSize: (std::mem::offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_0>()) as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferData: CF_OPERATION_PARAMETERS_0_0 {
                CompletionStatus: 0xC000CF11_u32 as i32,
                Offset: fetch.RequiredFileOffset,
                Length: fetch.RequiredLength,
                ..Default::default()
            },
        },
    };
    unsafe {
        CfExecute(&op, &mut params);
    }
}
unsafe extern "system" fn fetch_placeholders(
    info: *const CF_CALLBACK_INFO,
    _: *const CF_CALLBACK_PARAMETERS,
) {
    let info = unsafe { &*info };
    let count = unsafe { &*info.CallbackContext.cast::<AtomicUsize>() };
    count.fetch_add(1, Ordering::SeqCst);
    let op = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_PLACEHOLDERS,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        RequestKey: info.RequestKey,
        ..Default::default()
    };
    let mut params = CF_OPERATION_PARAMETERS {
        ParamSize: (std::mem::offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_4>()) as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferPlaceholders: CF_OPERATION_PARAMETERS_0_4 {
                CompletionStatus: 0xC000CF11_u32 as i32,
                ..Default::default()
            },
        },
    };
    unsafe {
        CfExecute(&op, &mut params);
    }
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = wide(directory.path().as_os_str());
        let name = wide(OsStr::new("Cepa disposable scanner test"));
        let version = wide(OsStr::new("1.0"));
        let registration = CF_SYNC_REGISTRATION {
            StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
            ProviderName: name.as_ptr(),
            ProviderVersion: version.as_ptr(),
            ..Default::default()
        };
        let policies = CF_SYNC_POLICIES {
            StructSize: size_of::<CF_SYNC_POLICIES>() as u32,
            Hydration: CF_HYDRATION_POLICY {
                Primary: CF_HYDRATION_POLICY_FULL,
                Modifier: 0,
            },
            Population: CF_POPULATION_POLICY {
                Primary: CF_POPULATION_POLICY_PARTIAL,
                Modifier: 0,
            },
            ..Default::default()
        };
        hr(unsafe {
            CfRegisterSyncRoot(
                path.as_ptr(),
                &registration,
                &policies,
                CF_REGISTER_FLAG_DISABLE_ON_DEMAND_POPULATION_ON_ROOT,
            )
        });
        // Construct the cleanup owner before any subsequent fallible operation.
        let mut fixture = Self {
            directory,
            path,
            connection: CF_CONNECTION_KEY::default(),
            callbacks: Box::new(AtomicUsize::new(0)),
        };
        let callbacks = [
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_DATA,
                Callback: Some(fetch_data),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_PLACEHOLDERS,
                Callback: Some(fetch_placeholders),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NONE,
                Callback: None,
            },
        ];
        hr(unsafe {
            CfConnectSyncRoot(
                fixture.path.as_ptr(),
                callbacks.as_ptr(),
                (&*fixture.callbacks as *const AtomicUsize).cast(),
                0,
                &mut fixture.connection,
            )
        });
        fixture
    }
    fn placeholder(&self, name: &str, directory: bool) {
        let name = wide(OsStr::new(name));
        let identity = b"cepa-test";
        let mut info = CF_PLACEHOLDER_CREATE_INFO {
            RelativeFileName: name.as_ptr(),
            FileIdentity: identity.as_ptr().cast(),
            FileIdentityLength: identity.len() as u32,
            FsMetadata: CF_FS_METADATA {
                BasicInfo: FILE_BASIC_INFO {
                    FileAttributes: if directory {
                        FILE_ATTRIBUTE_DIRECTORY
                    } else {
                        FILE_ATTRIBUTE_NORMAL
                    },
                    ..Default::default()
                },
                FileSize: if directory { 0 } else { 1024 * 1024 },
            },
            Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
            ..Default::default()
        };
        let mut processed = 0;
        hr(unsafe { CfCreatePlaceholders(self.path.as_ptr(), &mut info, 1, 0, &mut processed) });
        hr(info.Result);
        assert_eq!(processed, 1);
    }
    fn downloaded(&self) {
        let path = self.directory.path().join("downloaded.bin");
        std::fs::write(&path, [9; 4096]).unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        let identity = b"downloaded";
        hr(unsafe {
            CfConvertToPlaceholder(
                file.as_raw_handle(),
                identity.as_ptr().cast(),
                identity.len() as u32,
                CF_CONVERT_FLAG_MARK_IN_SYNC,
                null_mut(),
                null_mut(),
            )
        });
        let directory_path = self.directory.path().join("downloaded-folder");
        std::fs::create_dir(&directory_path).unwrap();
        std::fs::write(directory_path.join("local-child.bin"), [3; 37]).unwrap();
        let name = wide(directory_path.as_os_str());
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Foundation::GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        let file = unsafe { File::from_raw_handle(handle) };
        hr(unsafe {
            CfConvertToPlaceholder(
                file.as_raw_handle(),
                identity.as_ptr().cast(),
                identity.len() as u32,
                CF_CONVERT_FLAG_MARK_IN_SYNC,
                null_mut(),
                null_mut(),
            )
        });
    }
}

#[test]
#[ignore = "registers a disposable Cloud Files sync root on Windows NTFS"]
fn cloud_files_fixture_skips_without_fetching_on_both_backends() {
    let _mode = PlaceholderMode::new().unwrap();
    let fixture = Fixture::new();
    fixture.placeholder("remote.bin", false);
    fixture.placeholder("remote-folder", true);
    fixture.downloaded();
    std::fs::write(fixture.directory.path().join("ordinary.bin"), [1; 17]).unwrap();
    let (_, root) = open_root(fixture.directory.path()).unwrap();
    let before = attributes(&open_child(&root, OsStr::new("remote.bin"), false).unwrap()).unwrap();
    assert!(needs_recall(before.FileAttributes));
    let downloaded =
        attributes(&open_child(&root, OsStr::new("downloaded.bin"), false).unwrap()).unwrap();
    assert!(is_cloud_tag(downloaded.ReparseTag));
    assert!(!needs_recall(downloaded.FileAttributes));
    assert!(validate_root(&fixture.directory.path().join("remote-folder")).is_err());
    assert!(validate_root(&fixture.directory.path().join("remote-folder/missing-child")).is_err());
    let mut outputs = Vec::new();
    outputs.push(
        windows_walk::scan_path(
            fixture.directory.path(),
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .unwrap(),
    );
    outputs.push(windows::scan_fixture(fixture.directory.path(), &mut |_| {}).unwrap());
    for output in &outputs {
        assert_eq!(output.result.logical_bytes, 4150);
        assert_eq!(output.result.file_count, 3);
        assert_eq!(output.result.directory_count, 1);
        assert_eq!(output.result.skipped_cloud_entries, 2);
        assert_eq!(output.result.skipped_entries, 0);
        assert!(
            !output
                .snapshot
                .nodes
                .iter()
                .any(|node| node.name == "remote.bin" || node.name == "remote-folder")
        );
        eprintln!(
            "{}: {} local files, {} logical, {} allocated, {} cloud exclusions",
            output.result.backend,
            output.result.file_count,
            output.result.logical_bytes,
            output.result.allocated_bytes,
            output.result.skipped_cloud_entries
        );
    }
    assert!(
        outputs[0]
            .result
            .accounting_mismatches(&outputs[1].result)
            .is_empty()
    );
    let after = attributes(&open_child(&root, OsStr::new("remote.bin"), false).unwrap()).unwrap();
    assert_eq!(before.FileAttributes, after.FileAttributes);
    assert_eq!(
        fixture.callbacks.load(Ordering::SeqCst),
        0,
        "scanner requested provider data or directory population"
    );
    // Exercise the last line of defense after a directory becomes a cloud
    // placeholder between its admission check and the actual enumeration.
    let remote_dir = open_child(&root, OsStr::new("remote-folder"), false).unwrap();
    let remote_dir = open_child(&remote_dir, OsStr::new(""), true).unwrap();
    let mut buffer = vec![0_u64; 2048];
    let _ = read_directory(&remote_dir, &mut buffer).unwrap();
    assert_eq!(fixture.callbacks.load(Ordering::SeqCst), 0);
    eprintln!(
        "provider fetch callbacks: 0; cloud-only roots and descendants rejected; on-disk enumeration did not populate a cloud directory"
    );
    // Positive control: ordinary content IO must reach the connected provider.
    assert!(std::fs::read(fixture.directory.path().join("remote.bin")).is_err());
    assert!(
        fixture.callbacks.load(Ordering::SeqCst) > 0,
        "provider callback instrumentation did not activate"
    );
    eprintln!(
        "positive-control content read reached provider: {} fetch callbacks",
        fixture.callbacks.load(Ordering::SeqCst)
    );
}

#[test]
#[ignore = "read-only scan of CEPA_CLOUD_FIXTURE_ROOT with existing Windows placeholders"]
fn existing_cloud_placeholders_stay_offline() {
    let _mode = PlaceholderMode::new().unwrap();
    let root = std::env::var_os("CEPA_CLOUD_FIXTURE_ROOT").expect("set CEPA_CLOUD_FIXTURE_ROOT");
    let root = Path::new(&root);
    let (_, handle) = open_root(root).unwrap();
    let directory = directory_handle(&handle).unwrap();
    let mut buffer = vec![0_u64; 16 * 1024 / 8];
    let mut placeholders = Vec::new();
    loop {
        let length = read_directory(&directory, &mut buffer).unwrap();
        if length == 0 {
            break;
        }
        let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), length) };
        let mut offset = 0;
        loop {
            let record = &bytes[offset..];
            let next = u32::from_le_bytes(record[..4].try_into().unwrap()) as usize;
            let attrs = u32::from_le_bytes(record[56..60].try_into().unwrap());
            let length = u32::from_le_bytes(record[60..64].try_into().unwrap()) as usize;
            if enumeration_needs_recall(attrs) {
                use std::os::windows::ffi::OsStringExt;
                let name = std::ffi::OsString::from_wide(
                    &record[64..64 + length]
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect::<Vec<_>>(),
                );
                let file = open_child(&handle, &name, false).unwrap();
                let meta = file.metadata().unwrap();
                placeholders.push((name, attributes(&file).unwrap().FileAttributes, meta.len()));
            }
            if next == 0 {
                break;
            }
            offset += next;
        }
    }
    assert!(
        !placeholders.is_empty(),
        "fixture must contain unavailable entries"
    );
    let output = windows_walk::scan_path(root, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
    assert!(output.result.skipped_cloud_entries >= placeholders.len() as u64);
    for (name, before, length) in &placeholders {
        let file = open_child(&handle, name, false).unwrap();
        assert_eq!(attributes(&file).unwrap().FileAttributes, *before);
        assert_eq!(file.metadata().unwrap().len(), *length);
    }
    eprintln!(
        "Existing cloud fixture: {} directly observed placeholders unchanged; {} local files, {} directories, {} logical bytes, {} allocated bytes, {} cloud exclusions, {} unavailable items",
        placeholders.len(),
        output.result.file_count,
        output.result.directory_count,
        output.result.logical_bytes,
        output.result.allocated_bytes,
        output.result.skipped_cloud_entries,
        output.result.skipped_entries
    );
}
