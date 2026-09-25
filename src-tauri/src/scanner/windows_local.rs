//! Windows local-only IO. All child opens are relative to an already-open parent;
//! no path lookup can follow a replaced intermediate directory. Only recognized
//! Cloud Files reparse tags are treated as ordinary files/directories.
//!
//! Placeholder exposure is NOT a no-hydration policy. Protection also requires
//! metadata-only, no-reparse/no-recall opens and on-disk-only directory queries.
use super::EntryKind;
use std::ffi::{OsStr, c_void};
use std::fs::{File, Metadata};
use std::io;
use std::marker::PhantomData;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Component, Path, PathBuf};
use std::ptr::{null, null_mut};
use std::rc::Rc;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::SystemServices::{IO_REPARSE_TAG_CLOUD, IO_REPARSE_TAG_CLOUD_MASK};

#[path = "windows_streams.rs"]
mod streams;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: *mut c_void,
    name: *const UnicodeString,
    attributes: u32,
    security_descriptor: *const c_void,
    security_qos: *const c_void,
}
#[repr(C)]
#[derive(Default)]
struct IoStatus {
    status: usize,
    information: usize,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn RtlSetThreadPlaceholderCompatibilityMode(mode: i8) -> i8;
    fn RtlNtStatusToDosError(status: i32) -> u32;
    fn NtCreateFile(
        handle: *mut *mut c_void,
        access: u32,
        attributes: *const ObjectAttributes,
        status: *mut IoStatus,
        allocation: *const i64,
        file_attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *const c_void,
        ea_length: u32,
    ) -> i32;
    fn NtQueryDirectoryFileEx(
        handle: *mut c_void,
        event: *mut c_void,
        apc: *const c_void,
        context: *const c_void,
        status: *mut IoStatus,
        buffer: *mut c_void,
        length: u32,
        class: u32,
        flags: u32,
        name: *const UnicodeString,
    ) -> i32;
}

pub(super) struct PlaceholderMode(i8, PhantomData<Rc<()>>);
impl PlaceholderMode {
    pub(super) fn new() -> io::Result<Self> {
        // PHCM_EXPOSE_PLACEHOLDERS; applies only to this thread.
        let previous = unsafe { RtlSetThreadPlaceholderCompatibilityMode(2) };
        if previous < 0 {
            return Err(io::Error::other("Could not expose cloud placeholders"));
        }
        Ok(Self(previous, PhantomData))
    }
}
impl Drop for PlaceholderMode {
    fn drop(&mut self) {
        unsafe {
            RtlSetThreadPlaceholderCompatibilityMode(self.0);
        }
    }
}

pub(super) fn needs_recall(attributes: u32) -> bool {
    attributes & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0
}
pub(super) fn enumeration_needs_recall(attributes: u32) -> bool {
    // 0x40000 means RECALL_ON_OPEN only in directory enumeration. In basic
    // metadata and USN records it can mean EA, so never test it there.
    needs_recall(attributes) || attributes & FILE_ATTRIBUTE_RECALL_ON_OPEN != 0
}
pub(super) fn is_cloud_tag(tag: u32) -> bool {
    tag & !IO_REPARSE_TAG_CLOUD_MASK == IO_REPARSE_TAG_CLOUD
}
pub(super) fn classify(attributes: u32, tag: u32) -> EntryKind {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 && !is_cloud_tag(tag) {
        EntryKind::Symlink
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}
pub(super) fn attributes(file: &File) -> io::Result<FILE_ATTRIBUTE_TAG_INFO> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}
#[derive(Debug)]
struct CloudOnly;
impl std::fmt::Display for CloudOnly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "This item requires a cloud download. Choose a folder available on this device.",
        )
    }
}
impl std::error::Error for CloudOnly {}
// The retained identity is 64-bit. NTFS guarantees that identity; ReFS may
// require 128-bit IDs, so do not deduplicate unknown filesystems with this key.
pub(super) fn has_ntfs_file_ids(file: &File) -> bool {
    let mut name = [0_u16; 16];
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    ok != 0 && name[..5] == [b'N' as u16, b'T' as u16, b'F' as u16, b'S' as u16, 0]
}

pub(super) fn cloud_error() -> io::Error {
    io::Error::other(CloudOnly)
}
pub(super) fn is_cloud_error(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|inner| inner.is::<CloudOnly>())
}
fn check_directory(file: &File) -> io::Result<()> {
    let info = attributes(file)?;
    if needs_recall(info.FileAttributes) {
        return Err(cloud_error());
    }
    if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "Choose a directory to scan.",
        ));
    }
    if classify(info.FileAttributes, info.ReparseTag) != EntryKind::Directory {
        return Err(io::Error::other(
            "Choose a local directory, without symbolic links or junctions in its path.",
        ));
    }
    Ok(())
}
fn nt_result(status: i32) -> io::Result<()> {
    if status < 0 {
        Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ))
    } else {
        Ok(())
    }
}

/// Open one child without data-read permission, or reopen an already-verified
/// held directory itself (empty name) with LIST_DIRECTORY. The held object
/// cannot change type through a pathname replacement.
pub(super) fn open_child(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
    let wide: Vec<u16> = name.encode_wide().collect();
    if directory && !wide.is_empty() {
        return Err(io::Error::other(
            "Directory reopen requires a held directory",
        ));
    }
    if wide.iter().any(|c| matches!(*c, 0 | 47 | 58 | 92)) || name == "." || name == ".." {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected a single child name",
        ));
    }
    let length = u16::try_from(wide.len() * 2).map_err(io::Error::other)?;
    let name = UnicodeString {
        length,
        maximum_length: length,
        buffer: wide.as_ptr(),
    };
    let object = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &name,
        attributes: 0x40,
        security_descriptor: null(),
        security_qos: null(),
    };
    let mut status = IoStatus::default();
    let mut handle = null_mut();
    // FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT, FILE_OPEN_REPARSE_POINT,
    // FILE_OPEN_NO_RECALL. The final component is never reparsed.
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_READ_ATTRIBUTES | 0x00100000 | if directory { FILE_LIST_DIRECTORY } else { 0 },
            &object,
            &mut status,
            null(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            1,
            0x20 | 0x00200000 | 0x00400000,
            null(),
            0,
        )
    };
    nt_result(result)?;
    // SAFETY: successful synchronous NtCreateFile returns an owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

/// Resolve without canonicalize(): that would follow cloud/junction ancestors
/// before they could be checked. Absolute path normalization performs no IO.
pub(super) fn open_root(path: &Path) -> io::Result<(PathBuf, File)> {
    let _mode = PlaceholderMode::new()?;
    let absolute = std::path::absolute(path)?;
    let mut components = absolute.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(io::Error::other("Expected an absolute Windows path"));
    };
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(io::Error::other("Expected a rooted Windows path"));
    }
    let mut volume = PathBuf::from(prefix.as_os_str());
    volume.push("\\");
    let wide: Vec<_> = volume.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_OPEN_NO_RECALL,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_handle(handle) };
    // Some virtual providers return ordinary attributes and fictional allocation.
    // Check the held volume before resolving descendants or listing anything.
    streams::check_volume(&file)?;
    check_directory(&file)?;
    for component in components {
        match component {
            Component::Normal(name) => {
                file = open_child(&file, name, false)?;
                check_directory(&file)?;
            }
            Component::CurDir => {}
            _ => {
                return Err(io::Error::other(
                    "Parent traversal is not supported in a scan root",
                ));
            }
        }
    }
    let mut buffer = vec![0_u16; 32_768];
    // FILE_NAME_OPENED avoids normalized-name lookup through provider ancestors.
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_OPENED,
        )
    } as usize;
    if count == 0 || count >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    use std::os::windows::ffi::OsStringExt;
    let resolved = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..count]));
    Ok((resolved, file))
}

pub(super) fn is_google_drive_root(path: &Path) -> bool {
    open_root(path).is_err_and(|error| streams::is_google_drive_error(&error))
}
pub(super) fn validate_root(path: &Path) -> Result<(PathBuf, Metadata), String> {
    let (root, file) = open_root(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotADirectory {
            e.to_string()
        } else {
            format!("Could not open {}: {e}", path.display())
        }
    })?;
    Ok((root, file.metadata().map_err(|e| e.to_string())?))
}
pub(super) fn directory_handle(file: &File) -> io::Result<File> {
    check_directory(file)?;
    // FILE_DIRECTORY_FILE cannot be combined with FILE_OPEN_NO_RECALL on
    // Windows. The original checked handle fixes the object type instead.
    let directory = open_child(file, OsStr::new(""), true)?;
    check_directory(&directory)?;
    Ok(directory)
}

/// A synchronous query against the held directory. Never retry without
/// SL_RETURN_ON_DISK_ENTRIES_ONLY, even on unsupported filesystems/providers.
pub(super) fn read_directory(file: &File, buffer: &mut [u64]) -> io::Result<usize> {
    let mut status = IoStatus::default();
    let result = unsafe {
        NtQueryDirectoryFileEx(
            file.as_raw_handle(),
            null_mut(),
            null(),
            null(),
            &mut status,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(buffer) as u32,
            1,
            0x8,
            null(),
        )
    }; // FileDirectoryInformation, SL_RETURN_ON_DISK_ENTRIES_ONLY
    if result == 0x80000006_u32 as i32 {
        return Ok(0);
    } // STATUS_NO_MORE_FILES
    nt_result(result)?;
    if status.information == 0 || status.information > std::mem::size_of_val(buffer) {
        return Err(io::Error::other("Invalid directory query length"));
    }
    Ok(status.information)
}

#[cfg(test)]
#[path = "windows_cloud_tests.rs"]
mod tests;
