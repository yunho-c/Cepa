use serde::Serialize;
use std::path::Path;

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

#[cfg(target_os = "macos")]
mod platform {
    use super::CompressionCapability;
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
    use super::CompressionCapability;
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;
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
}

#[cfg(windows)]
mod platform {
    use super::CompressionCapability;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
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
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod platform {
    use super::CompressionCapability;
    use std::path::Path;

    pub(super) fn probe(_path: &Path) -> CompressionCapability {
        CompressionCapability::unsupported(
            std::env::consts::OS,
            "Cepa does not yet have a compression capability probe for this platform.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{CompressionCapability, CompressionCapabilityStatus, probe};

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

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_decmpfs_capability_from_the_local_temporary_volume() {
        let temp = tempfile::tempdir().expect("create temporary volume fixture");
        let capability = probe(temp.path());

        assert_eq!(capability.status, CompressionCapabilityStatus::InspectOnly);
        assert!(capability.volume_supports_transparent_compression);
        assert!(!capability.writer_available);
    }
}
