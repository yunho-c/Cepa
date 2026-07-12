use serde::Serialize;
use std::path::Path;

use crate::scanner::EntryKind;

#[path = "compression/estimator.rs"]
mod estimator;

pub(crate) use estimator::SavingsEstimate;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CompressionCapabilityStatus {
    InspectOnly,
    Unsupported,
    Unavailable,
}

/// Read-only information about transparent compression on the scanned volume.
///
/// This deliberately separates filesystem capability from Cepa's ability to
/// mutate files. No writer backend exists yet, so `writer_available` is always
/// false regardless of the volume's native features.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompressionCapability {
    pub status: CompressionCapabilityStatus,
    pub filesystem: String,
    pub volume_supports_transparent_compression: bool,
    pub writer_available: bool,
    pub algorithms: Vec<String>,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
// Each native target constructs only the states its filesystem API can report.
#[allow(dead_code)]
pub(crate) enum CompressionStateKind {
    Compressed,
    NotCompressed,
    Enabled,
    Disabled,
    Inherited,
    NotApplicable,
    Unsupported,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum CompressionStateScope {
    ExistingData,
    FutureWrites,
    None,
}

/// Current read-only compression metadata for one scan-authorized item.
/// `scope` is explicit because Btrfs inode flags govern future writes while
/// Windows and macOS report the state of existing file data.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompressionState {
    pub state: CompressionStateKind,
    pub scope: CompressionStateScope,
    pub format: Option<String>,
    pub detail: String,
}

impl CompressionState {
    fn existing_data(compressed: bool, format: Option<&str>, detail: impl Into<String>) -> Self {
        Self {
            state: if compressed {
                CompressionStateKind::Compressed
            } else {
                CompressionStateKind::NotCompressed
            },
            scope: CompressionStateScope::ExistingData,
            format: format.map(str::to_owned),
            detail: detail.into(),
        }
    }

    #[allow(dead_code)]
    fn future_writes(state: CompressionStateKind, detail: impl Into<String>) -> Self {
        Self {
            state,
            scope: CompressionStateScope::FutureWrites,
            format: None,
            detail: detail.into(),
        }
    }

    fn not_applicable(detail: impl Into<String>) -> Self {
        Self {
            state: CompressionStateKind::NotApplicable,
            scope: CompressionStateScope::None,
            format: None,
            detail: detail.into(),
        }
    }

    #[allow(dead_code)]
    fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            state: CompressionStateKind::Unsupported,
            scope: CompressionStateScope::None,
            format: None,
            detail: detail.into(),
        }
    }

    fn unavailable(detail: impl Into<String>) -> Self {
        Self {
            state: CompressionStateKind::Unavailable,
            scope: CompressionStateScope::None,
            format: None,
            detail: detail.into(),
        }
    }
}

impl CompressionCapability {
    fn inspect_only(
        filesystem: impl Into<String>,
        algorithms: impl IntoIterator<Item = &'static str>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            status: CompressionCapabilityStatus::InspectOnly,
            filesystem: filesystem.into(),
            volume_supports_transparent_compression: true,
            writer_available: false,
            algorithms: algorithms.into_iter().map(str::to_owned).collect(),
            detail: detail.into(),
        }
    }

    fn unsupported(filesystem: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: CompressionCapabilityStatus::Unsupported,
            filesystem: filesystem.into(),
            volume_supports_transparent_compression: false,
            writer_available: false,
            algorithms: Vec::new(),
            detail: detail.into(),
        }
    }

    fn unavailable(filesystem: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: CompressionCapabilityStatus::Unavailable,
            filesystem: filesystem.into(),
            volume_supports_transparent_compression: false,
            writer_available: false,
            algorithms: Vec::new(),
            detail: detail.into(),
        }
    }
}

pub(crate) fn probe(path: &Path) -> CompressionCapability {
    platform::probe(path)
}

pub(crate) fn inspect(path: &Path, kind: EntryKind) -> CompressionState {
    match kind {
        EntryKind::File => platform::inspect(path),
        EntryKind::Directory => CompressionState::not_applicable(
            "Folder compression defaults are not part of the current read-only file inspector.",
        ),
        EntryKind::Symlink => CompressionState::not_applicable(
            "Symbolic links and reparse points are never followed for compression inspection.",
        ),
        EntryKind::Other => {
            CompressionState::not_applicable("This filesystem entry is not a regular file.")
        }
    }
}

pub(crate) fn estimate(
    target: &crate::scanner::CompressionTarget,
    cancel: &std::sync::atomic::AtomicBool,
) -> SavingsEstimate {
    estimator::estimate(target, cancel)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{CompressionCapability, CompressionState};
    use std::ffi::{CStr, CString};
    use std::mem::{self, MaybeUninit};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    #[repr(C)]
    struct CapabilityBuffer {
        length: u32,
        capabilities: libc::vol_capabilities_attr_t,
    }

    pub(super) fn probe(path: &Path) -> CompressionCapability {
        let path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(path) => path,
            Err(_) => {
                return CompressionCapability::unavailable(
                    "unknown",
                    "The scanned path contains a null byte and could not be inspected.",
                );
            }
        };
        let filesystem = filesystem_name(&path).unwrap_or_else(|_| "unknown".to_string());
        match decmpfs_supported(&path) {
            Ok(true) => CompressionCapability::inspect_only(
                filesystem,
                [],
                "This volume supports transparent decmpfs decompression. Cepa can only report the capability; compression changes are not implemented.",
            ),
            Ok(false) => CompressionCapability::unsupported(
                filesystem,
                "This volume does not report transparent decmpfs compression support.",
            ),
            Err(error) => CompressionCapability::unavailable(
                filesystem,
                format!("The volume capability query failed: {error}."),
            ),
        }
    }

    pub(super) fn inspect(path: &Path) -> CompressionState {
        let path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(path) => path,
            Err(_) => {
                return CompressionState::unavailable(
                    "The scanned path contains a null byte and could not be inspected.",
                );
            }
        };
        let mut info = MaybeUninit::<libc::stat>::zeroed();
        // SAFETY: path is null-terminated and lstat writes to valid storage.
        if unsafe { libc::lstat(path.as_ptr(), info.as_mut_ptr()) } != 0 {
            return CompressionState::unavailable(format!(
                "The file metadata query failed: {}.",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: lstat succeeded and initialized the output structure.
        let info = unsafe { info.assume_init() };
        if info.st_mode & libc::S_IFMT != libc::S_IFREG {
            return CompressionState::unavailable(
                "The scanned item is no longer a regular file and was not followed.",
            );
        }
        if info.st_flags & libc::UF_COMPRESSED != 0 {
            CompressionState::existing_data(
                true,
                Some("decmpfs"),
                "macOS reports UF_COMPRESSED for this file. Cepa reads this metadata but does not modify it.",
            )
        } else {
            CompressionState::existing_data(
                false,
                None,
                "macOS does not report UF_COMPRESSED for this file.",
            )
        }
    }

    fn decmpfs_supported(path: &CStr) -> std::io::Result<bool> {
        let mut attributes = libc::attrlist {
            bitmapcount: libc::ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: 0,
            volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_CAPABILITIES,
            dirattr: 0,
            fileattr: 0,
            forkattr: 0,
        };
        let mut buffer = MaybeUninit::<CapabilityBuffer>::zeroed();
        // SAFETY: both pointers remain valid for the call, the buffer matches
        // the fixed ATTR_VOL_CAPABILITIES response, and getattrlist writes at
        // most the supplied buffer size.
        let result = unsafe {
            libc::getattrlist(
                path.as_ptr(),
                (&mut attributes as *mut libc::attrlist).cast(),
                buffer.as_mut_ptr().cast(),
                mem::size_of::<CapabilityBuffer>(),
                0,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: getattrlist succeeded and initialized the complete fixed-size
        // response requested above.
        let buffer = unsafe { buffer.assume_init() };
        if usize::try_from(buffer.length).unwrap_or(0) < mem::size_of::<CapabilityBuffer>() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the capability response was shorter than expected",
            ));
        }
        let index = libc::VOL_CAPABILITIES_FORMAT;
        let bit = libc::VOL_CAP_FMT_DECMPFS_COMPRESSION;
        Ok(buffer.capabilities.valid[index] & bit != 0
            && buffer.capabilities.capabilities[index] & bit != 0)
    }

    fn filesystem_name(path: &CStr) -> std::io::Result<String> {
        let mut info = MaybeUninit::<libc::statfs>::zeroed();
        // SAFETY: path is null-terminated and info points to writable storage.
        if unsafe { libc::statfs(path.as_ptr(), info.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: statfs succeeded and initialized the output structure.
        let info = unsafe { info.assume_init() };
        let bytes = info.f_fstypename.map(|character| character as u8);
        let length = bytes
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(bytes.len());
        Ok(String::from_utf8_lossy(&bytes[..length]).into_owned())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{CompressionCapability, CompressionState, CompressionStateKind};
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
    use std::path::Path;

    pub(super) fn probe(path: &Path) -> CompressionCapability {
        let path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(path) => path,
            Err(_) => {
                return CompressionCapability::unavailable(
                    "unknown",
                    "The scanned path contains a null byte and could not be inspected.",
                );
            }
        };
        let mut info = MaybeUninit::<libc::statfs>::zeroed();
        // SAFETY: path is null-terminated and info points to writable storage.
        if unsafe { libc::statfs(path.as_ptr(), info.as_mut_ptr()) } != 0 {
            return CompressionCapability::unavailable(
                "unknown",
                format!(
                    "The filesystem type query failed: {}.",
                    std::io::Error::last_os_error()
                ),
            );
        }
        // SAFETY: statfs succeeded and initialized the output structure.
        let filesystem_type = unsafe { info.assume_init() }.f_type;
        if filesystem_type == libc::BTRFS_SUPER_MAGIC {
            CompressionCapability::inspect_only(
                "btrfs",
                ["zlib", "lzo", "zstd"],
                "Btrfs supports transparent compression. Cepa can only report the volume capability; per-file inspection and compression changes are not implemented.",
            )
        } else {
            CompressionCapability::unsupported(
                format!("0x{filesystem_type:x}"),
                "Cepa's read-only compression protocol currently recognizes Btrfs on Linux.",
            )
        }
    }

    pub(super) fn inspect(path: &Path) -> CompressionState {
        let path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(path) => path,
            Err(_) => {
                return CompressionState::unavailable(
                    "The scanned path contains a null byte and could not be inspected.",
                );
            }
        };
        // SAFETY: path is null-terminated. O_NOFOLLOW prevents resolving a
        // replacement symlink, and OwnedFd closes the successful descriptor.
        let raw_fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if raw_fd < 0 {
            return CompressionState::unavailable(format!(
                "The file could not be opened without following links: {}.",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: raw_fd is a newly owned successful open result.
        let file = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        let mut metadata = MaybeUninit::<libc::stat>::zeroed();
        // SAFETY: the descriptor is open and metadata points to writable storage.
        if unsafe { libc::fstat(file.as_raw_fd(), metadata.as_mut_ptr()) } != 0 {
            return CompressionState::unavailable(format!(
                "The opened file metadata query failed: {}.",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: fstat succeeded and initialized the output structure.
        if unsafe { metadata.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFREG {
            return CompressionState::unavailable(
                "The scanned item is no longer a regular file and was not followed.",
            );
        }
        let mut filesystem = MaybeUninit::<libc::statfs>::zeroed();
        // SAFETY: the descriptor is open and filesystem points to writable storage.
        if unsafe { libc::fstatfs(file.as_raw_fd(), filesystem.as_mut_ptr()) } != 0 {
            return CompressionState::unavailable(format!(
                "The opened file's filesystem query failed: {}.",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: fstatfs succeeded and initialized the output structure.
        if unsafe { filesystem.assume_init() }.f_type != libc::BTRFS_SUPER_MAGIC {
            return CompressionState::unsupported(
                "Per-file compression-state inspection is currently implemented only for Btrfs on Linux.",
            );
        }

        let mut flags: libc::c_int = 0;
        // SAFETY: FS_IOC_GETFLAGS expects an int pointer for an open inode.
        if unsafe { libc::ioctl(file.as_raw_fd(), libc::FS_IOC_GETFLAGS, &mut flags) } != 0 {
            return CompressionState::unavailable(format!(
                "The Btrfs inode-flag query failed: {}.",
                std::io::Error::last_os_error()
            ));
        }
        const FS_COMPR_FL: libc::c_int = 0x0000_0004;
        const FS_NOCOMP_FL: libc::c_int = 0x0000_0400;
        if flags & FS_COMPR_FL != 0 {
            CompressionState::future_writes(
                CompressionStateKind::Enabled,
                "Btrfs has the compression inode flag set. This governs newly written data and does not prove that existing extents are compressed.",
            )
        } else if flags & FS_NOCOMP_FL != 0 {
            CompressionState::future_writes(
                CompressionStateKind::Disabled,
                "Btrfs has the no-compression inode flag set for future writes.",
            )
        } else {
            CompressionState::future_writes(
                CompressionStateKind::Inherited,
                "This file has no explicit Btrfs compression flag and follows the applicable mount or parent policy for future writes.",
            )
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::{CompressionCapability, CompressionState};
    use std::fs::File;
    use std::mem;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, COMPRESSION_FORMAT_LZNT1, COMPRESSION_FORMAT_NONE, CreateFileW,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_GET_COMPRESSION;
    use windows_sys::Win32::System::SystemServices::FILE_FILE_COMPRESSION;

    pub(super) fn probe(path: &Path) -> CompressionCapability {
        let mut path_wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        path_wide.push(0);
        let mut volume_path = vec![0_u16; 512];
        // SAFETY: both buffers are valid and null-terminated/output-sized as
        // required by GetVolumePathNameW.
        if unsafe {
            GetVolumePathNameW(
                path_wide.as_ptr(),
                volume_path.as_mut_ptr(),
                volume_path.len() as u32,
            )
        } == 0
        {
            return CompressionCapability::unavailable(
                "unknown",
                format!(
                    "The volume path query failed: {}.",
                    std::io::Error::last_os_error()
                ),
            );
        }

        let mut filesystem_name = vec![0_u16; 64];
        let mut flags = 0_u32;
        // SAFETY: volume_path was initialized by the successful call above;
        // optional outputs are null and the supplied name buffer is writable.
        if unsafe {
            GetVolumeInformationW(
                volume_path.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut flags,
                filesystem_name.as_mut_ptr(),
                filesystem_name.len() as u32,
            )
        } == 0
        {
            return CompressionCapability::unavailable(
                "unknown",
                format!(
                    "The volume information query failed: {}.",
                    std::io::Error::last_os_error()
                ),
            );
        }
        let length = filesystem_name
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(filesystem_name.len());
        let filesystem = match String::from_utf16_lossy(&filesystem_name[..length]) {
            name if name.is_empty() => "unknown".to_string(),
            name => name,
        };
        if flags & FILE_FILE_COMPRESSION != 0 {
            CompressionCapability::inspect_only(
                filesystem,
                ["lznt1"],
                "This volume supports per-file transparent compression. Cepa can only report the capability; compression changes are not implemented.",
            )
        } else {
            CompressionCapability::unsupported(
                filesystem,
                "This volume does not report per-file transparent compression support.",
            )
        }
    }

    pub(super) fn inspect(path: &Path) -> CompressionState {
        let mut path_wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        path_wide.push(0);
        // SAFETY: path_wide is null-terminated. OPEN_REPARSE_POINT prevents
        // following a replacement reparse point, and File owns the handle.
        let handle = unsafe {
            CreateFileW(
                path_wide.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return CompressionState::unavailable(format!(
                "The file could not be opened without following reparse points: {}.",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: handle is a newly owned successful CreateFileW result.
        let file = unsafe { File::from_raw_handle(handle) };
        let mut metadata = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is open and metadata points to writable storage.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut metadata) } == 0 {
            return CompressionState::unavailable(format!(
                "The opened file metadata query failed: {}.",
                std::io::Error::last_os_error()
            ));
        }
        if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return CompressionState::not_applicable(
                "Reparse points are never followed for compression inspection.",
            );
        }

        let mut format = COMPRESSION_FORMAT_NONE;
        let mut returned = 0_u32;
        // SAFETY: the handle is open, no input buffer is supplied, and the
        // output buffer is a writable 16-bit compression format as documented.
        if unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                FSCTL_GET_COMPRESSION,
                std::ptr::null(),
                0,
                (&mut format as *mut u16).cast(),
                mem::size_of::<u16>() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return CompressionState::unavailable(format!(
                "The Windows compression-state query failed: {}.",
                std::io::Error::last_os_error()
            ));
        }
        if returned < mem::size_of::<u16>() as u32 {
            return CompressionState::unavailable(
                "The Windows compression-state response was shorter than expected.",
            );
        }
        if format == COMPRESSION_FORMAT_NONE {
            CompressionState::existing_data(
                false,
                None,
                "Windows reports that this file is not compressed.",
            )
        } else {
            let format_name = if format == COMPRESSION_FORMAT_LZNT1 {
                "lznt1".to_string()
            } else {
                format!("format-0x{format:04x}")
            };
            CompressionState::existing_data(
                true,
                Some(&format_name),
                "Windows reports the current per-stream compression format for this file.",
            )
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod platform {
    use super::{CompressionCapability, CompressionState};
    use std::path::Path;

    pub(super) fn probe(_path: &Path) -> CompressionCapability {
        CompressionCapability::unsupported(
            std::env::consts::OS,
            "Cepa does not yet have a compression capability probe for this platform.",
        )
    }

    pub(super) fn inspect(_path: &Path) -> CompressionState {
        CompressionState::unsupported(
            "Cepa does not yet have a compression-state inspector for this platform.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompressionCapability, CompressionCapabilityStatus, CompressionState, CompressionStateKind,
        CompressionStateScope, inspect, probe,
    };
    use crate::scanner::EntryKind;
    use std::path::Path;

    #[test]
    fn wire_contract_separates_volume_support_from_writer_availability() {
        let capability = CompressionCapability::inspect_only(
            "testfs",
            ["test-algorithm"],
            "read-only test capability",
        );
        let wire = serde_json::to_value(&capability).expect("serialize capability");

        assert_eq!(wire["status"], "inspectOnly");
        assert_eq!(wire["filesystem"], "testfs");
        assert_eq!(wire["volumeSupportsTransparentCompression"], true);
        assert_eq!(wire["writerAvailable"], false);
        assert_eq!(wire["algorithms"][0], "test-algorithm");
    }

    #[test]
    fn state_wire_contract_keeps_state_and_scope_orthogonal() {
        let state =
            CompressionState::future_writes(CompressionStateKind::Enabled, "future writes only");
        let wire = serde_json::to_value(&state).expect("serialize state");

        assert_eq!(wire["state"], "enabled");
        assert_eq!(wire["scope"], "futureWrites");
        assert!(wire["format"].is_null());
    }

    #[test]
    fn non_files_are_rejected_before_platform_inspection() {
        let state = inspect(Path::new("not-opened"), EntryKind::Symlink);

        assert_eq!(state.state, CompressionStateKind::NotApplicable);
        assert_eq!(state.scope, CompressionStateScope::None);
    }

    #[test]
    fn probing_a_real_temporary_volume_never_claims_a_writer() {
        let temp = tempfile::tempdir().expect("create temporary volume fixture");
        let capability = probe(temp.path());

        assert!(!capability.filesystem.is_empty());
        assert!(!capability.writer_available);
        assert_eq!(
            capability.volume_supports_transparent_compression,
            capability.status == CompressionCapabilityStatus::InspectOnly
        );
        #[cfg(any(target_os = "macos", target_os = "linux", windows))]
        assert_ne!(
            capability.status,
            CompressionCapabilityStatus::Unavailable,
            "a capability query on the local temporary volume should succeed"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn inspects_a_real_regular_file_without_mutating_it() {
        let temp = tempfile::tempdir().expect("create state fixture");
        let file = temp.path().join("state.bin");
        std::fs::write(&file, b"read-only state fixture").expect("write state fixture");

        let state = inspect(&file, EntryKind::File);

        assert_ne!(state.state, CompressionStateKind::Unavailable);
        assert!(matches!(
            state.state,
            CompressionStateKind::Compressed
                | CompressionStateKind::NotCompressed
                | CompressionStateKind::Enabled
                | CompressionStateKind::Disabled
                | CompressionStateKind::Inherited
                | CompressionStateKind::Unsupported
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_decmpfs_capability_from_the_local_temporary_volume() {
        let temp = tempfile::tempdir().expect("create temporary volume fixture");
        let capability = probe(temp.path());

        assert_eq!(capability.status, CompressionCapabilityStatus::InspectOnly);
        assert!(capability.volume_supports_transparent_compression);
        assert!(!capability.writer_available);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_uncompressed_and_compressed_decmpfs_file_state() {
        let temp = tempfile::tempdir().expect("create compression fixture");
        let source = temp.path().join("source.bin");
        let compressed = temp.path().join("compressed.bin");
        std::fs::write(&source, vec![0_u8; 1024 * 1024]).expect("write compressible fixture");

        let uncompressed = inspect(&source, EntryKind::File);
        assert_eq!(uncompressed.state, CompressionStateKind::NotCompressed);
        assert_eq!(uncompressed.scope, CompressionStateScope::ExistingData);

        let status = std::process::Command::new("/usr/bin/ditto")
            .arg("--hfsCompression")
            .arg(&source)
            .arg(&compressed)
            .status()
            .expect("run ditto fixture compressor");
        assert!(status.success(), "ditto should create compressed fixture");

        let inspected = inspect(&compressed, EntryKind::File);
        assert_eq!(inspected.state, CompressionStateKind::Compressed);
        assert_eq!(inspected.scope, CompressionStateScope::ExistingData);
        assert_eq!(inspected.format.as_deref(), Some("decmpfs"));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlink_that_replaces_a_scanned_file() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("create replacement fixture");
        let target = temp.path().join("target.bin");
        let replacement = temp.path().join("replacement.bin");
        std::fs::write(&target, b"original").expect("write original");
        std::fs::write(&replacement, b"replacement").expect("write replacement");
        std::fs::remove_file(&target).expect("remove original");
        symlink(&replacement, &target).expect("replace with symlink");

        let state = inspect(&target, EntryKind::File);
        assert_eq!(state.state, CompressionStateKind::Unavailable);
    }
}
