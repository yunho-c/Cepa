use std::io;
use std::num::NonZeroU64;
use std::path::Path;

#[cfg(unix)]
const UNIX_NANOSECOND_BITS: u32 = 30;
#[cfg(unix)]
const UNIX_MIN_SECONDS: i64 = -(1_i64 << 33);
#[cfg(unix)]
const UNIX_MAX_SECONDS: i64 = (1_i64 << 33) - 1;
#[cfg(unix)]
const UNIX_NANOSECONDS_PER_SECOND: i64 = 1_000_000_000;

/// Compact, exact metadata retained from the scan for mutation planning.
///
/// A nonzero file ID gives `Option<ScannedFileRevision>` a niche, so unavailable
/// revisions do not add a separate tag to every retained scanner node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScannedFileRevision {
    filesystem_id: u64,
    file_id: NonZeroU64,
    modified: u64,
    changed: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FileSnapshot {
    pub scanned: ScannedFileRevision,
    pub logical_bytes: u64,
    pub allocated_bytes: Option<u64>,
    pub link_count: u64,
}

impl ScannedFileRevision {
    pub(crate) fn from_raw_parts(
        filesystem_id: u64,
        file_id: u64,
        modified: u64,
        changed: u64,
    ) -> Option<Self> {
        Some(Self {
            filesystem_id,
            file_id: NonZeroU64::new(file_id)?,
            modified,
            changed,
        })
    }

    #[cfg(unix)]
    pub(crate) fn from_unix_parts(
        filesystem_id: u64,
        file_id: u64,
        modified_seconds: i64,
        modified_nanoseconds: i64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
    ) -> Option<Self> {
        Self::from_raw_parts(
            filesystem_id,
            file_id,
            pack_unix_timestamp(modified_seconds, modified_nanoseconds)?,
            pack_unix_timestamp(changed_seconds, changed_nanoseconds)?,
        )
    }

    #[cfg(unix)]
    pub(crate) fn from_unix_metadata(metadata: &std::fs::Metadata) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;

        Self::from_unix_parts(
            metadata.dev(),
            metadata.ino(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        )
    }
}

#[cfg(unix)]
fn pack_unix_timestamp(seconds: i64, nanoseconds: i64) -> Option<u64> {
    if !(UNIX_MIN_SECONDS..=UNIX_MAX_SECONDS).contains(&seconds)
        || !(0..UNIX_NANOSECONDS_PER_SECOND).contains(&nanoseconds)
    {
        return None;
    }
    let biased_seconds = u64::try_from(i128::from(seconds) - i128::from(UNIX_MIN_SECONDS)).ok()?;
    Some((biased_seconds << UNIX_NANOSECOND_BITS) | nanoseconds as u64)
}

#[cfg(unix)]
pub(crate) fn snapshot_no_follow(path: &Path) -> io::Result<FileSnapshot> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is no longer a regular file",
        ));
    }
    let scanned = ScannedFileRevision::from_unix_metadata(&metadata).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "the filesystem returned an unusable file identity or timestamp",
        )
    })?;
    Ok(FileSnapshot {
        scanned,
        logical_bytes: metadata.len(),
        allocated_bytes: Some(metadata.blocks().saturating_mul(512)),
        link_count: metadata.nlink(),
    })
}

#[cfg(windows)]
pub(crate) fn snapshot_no_follow(path: &Path) -> io::Result<FileSnapshot> {
    use std::fs::File;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileBasicInfo, FileStandardInfo,
        GetFileInformationByHandle, GetFileInformationByHandleEx, OPEN_EXISTING,
    };

    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide.push(0);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is a reparse point",
        ));
    }
    let mut standard = FILE_STANDARD_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if standard.Directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is no longer a regular file",
        ));
    }
    let mut basic = FILE_BASIC_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let file_id =
        u64::from(information.nFileIndexHigh) << 32 | u64::from(information.nFileIndexLow);
    let scanned = ScannedFileRevision::from_raw_parts(
        u64::from(information.dwVolumeSerialNumber),
        file_id,
        basic.LastWriteTime as u64,
        basic.ChangeTime as u64,
    )
    .ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "the filesystem returned an unusable file identity",
        )
    })?;

    Ok(FileSnapshot {
        scanned,
        logical_bytes: u64::try_from(standard.EndOfFile).unwrap_or(0),
        allocated_bytes: Some(u64::try_from(standard.AllocationSize).unwrap_or(0)),
        link_count: u64::from(standard.NumberOfLinks),
    })
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn snapshot_no_follow(_path: &Path) -> io::Result<FileSnapshot> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "stable file identity is unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn optional_revision_stays_compact() {
        assert_eq!(size_of::<ScannedFileRevision>(), 32);
        assert_eq!(size_of::<Option<ScannedFileRevision>>(), 32);
    }

    #[cfg(unix)]
    #[test]
    fn unix_timestamp_packing_is_exact_and_bounded() {
        assert_ne!(
            pack_unix_timestamp(-1, 999_999_999),
            pack_unix_timestamp(0, 0)
        );
        assert!(pack_unix_timestamp(UNIX_MIN_SECONDS, 0).is_some());
        assert!(pack_unix_timestamp(UNIX_MAX_SECONDS, 999_999_999).is_some());
        assert!(pack_unix_timestamp(UNIX_MIN_SECONDS - 1, 0).is_none());
        assert!(pack_unix_timestamp(UNIX_MAX_SECONDS + 1, 0).is_none());
        assert!(pack_unix_timestamp(0, -1).is_none());
        assert!(pack_unix_timestamp(0, 1_000_000_000).is_none());
    }
}
