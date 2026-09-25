//! Exclude Google's virtual streaming volume, whose ordinary file attributes
//! and allocation do not establish local residency. Identify the actual driver
//! on the held volume; never infer this from a path, label, or filesystem name.
use super::{IoStatus, ObjectAttributes, UnicodeString, nt_result};
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::mem::{size_of, size_of_val};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

type OpenDirectory =
    unsafe extern "system" fn(*mut *mut c_void, u32, *const ObjectAttributes) -> i32;
type QueryDirectory =
    unsafe extern "system" fn(*mut c_void, *mut c_void, u32, u8, u8, *mut u32, *mut u32) -> i32;

#[repr(C)]
struct DirectoryEntry {
    name: UnicodeString,
    kind: UnicodeString,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryVolumeInformationFile(
        file: *mut c_void,
        status: *mut IoStatus,
        buffer: *mut c_void,
        length: u32,
        class: u32,
    ) -> i32;
}

#[derive(Debug)]
struct GoogleDriveStreaming;
impl std::fmt::Display for GoogleDriveStreaming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "Google Drive's streaming drive does not expose which files are stored locally. \
             Scan the local disk containing its cache, or choose a mirrored folder.",
        )
    }
}
impl std::error::Error for GoogleDriveStreaming {}

pub(super) fn is_google_drive_error(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<GoogleDriveStreaming>())
}

pub(super) fn check_volume(file: &File) -> io::Result<()> {
    for name in google_drive_drivers()? {
        if driver_in_path(file, &name)? {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                GoogleDriveStreaming,
            ));
        }
    }
    Ok(())
}

fn is_google_drive_driver(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.strip_prefix("googledrivefs")
        .is_some_and(|suffix| suffix.bytes().all(|c| c.is_ascii_digit()))
}

// Read a counted string only within the initialized result buffer. Kernel
// pointers are validated before constructing a slice; no NUL scan is needed.
fn object_string(value: &UnicodeString, buffer: &[u64], returned: usize) -> io::Result<String> {
    let invalid = || io::Error::other("Invalid filesystem driver name");
    let start = value.buffer as usize;
    let base = buffer.as_ptr() as usize;
    let length = usize::from(value.length);
    let offset = start.checked_sub(base).ok_or_else(invalid)?;
    if !length.is_multiple_of(2)
        || !offset.is_multiple_of(2)
        || offset
            .checked_add(length)
            .is_none_or(|end| end > returned.min(size_of_val(buffer)))
    {
        return Err(invalid());
    }
    // SAFETY: alignment and the full counted extent lie inside buffer.
    let wide = unsafe { std::slice::from_raw_parts(value.buffer, length / 2) };
    String::from_utf16(wide).map_err(|_| invalid())
}

fn google_drive_drivers() -> io::Result<Vec<String>> {
    // Enumerate live driver objects, rather than registry installation records,
    // cached drive-letter mappings, or a version-specific driver name. Resolve
    // the native object-directory APIs dynamically as Microsoft documents.
    let library_name: Vec<u16> = "ntdll.dll\0".encode_utf16().collect();
    let unavailable = || io::Error::other("Could not inspect filesystem drivers");
    let (open, query): (OpenDirectory, QueryDirectory) = unsafe {
        let module = GetModuleHandleW(library_name.as_ptr());
        if module.is_null() {
            return Err(unavailable());
        }
        let open = GetProcAddress(module, c"NtOpenDirectoryObject".as_ptr().cast())
            .ok_or_else(unavailable)?;
        let query = GetProcAddress(module, c"NtQueryDirectoryObject".as_ptr().cast())
            .ok_or_else(unavailable)?;
        // SAFETY: these exports have the documented signatures above.
        (
            std::mem::transmute::<unsafe extern "system" fn() -> isize, OpenDirectory>(open),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, QueryDirectory>(query),
        )
    };
    let wide: Vec<u16> = "\\FileSystem".encode_utf16().collect();
    let name = UnicodeString {
        length: (wide.len() * 2) as u16,
        maximum_length: (wide.len() * 2) as u16,
        buffer: wide.as_ptr(),
    };
    let object = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: null_mut(),
        name: &name,
        attributes: 0x40, // OBJ_CASE_INSENSITIVE
        security_descriptor: null(),
        security_qos: null(),
    };
    let mut handle = null_mut();
    nt_result(unsafe { open(&mut handle, 1, &object) })?; // DIRECTORY_QUERY
    // SAFETY: the successful call transferred an owned object handle.
    let directory = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut buffer = [0_u64; 512];
    let mut context = 0;
    let mut names = Vec::new();
    // Bound a malformed or continuously changing object namespace. This is a
    // root-time check only, never a per-file syscall or a process-global cache.
    for index in 0..4096 {
        let mut returned = 0;
        let status = unsafe {
            query(
                directory.as_raw_handle(),
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as u32,
                1,
                u8::from(index == 0),
                &mut context,
                &mut returned,
            )
        };
        if status == 0x8000001A_u32 as i32 {
            return Ok(names); // STATUS_NO_MORE_ENTRIES
        }
        nt_result(status)?;
        if (returned as usize) < size_of::<DirectoryEntry>()
            || returned as usize > size_of_val(&buffer)
        {
            return Err(unavailable());
        }
        // SAFETY: the aligned buffer contains a complete directory record.
        let entry = unsafe { &*buffer.as_ptr().cast::<DirectoryEntry>() };
        let name = object_string(&entry.name, &buffer, returned as usize)?;
        if is_google_drive_driver(&name)
            && object_string(&entry.kind, &buffer, returned as usize)? == "Driver"
        {
            names.push(format!("\\FileSystem\\{name}"));
        }
    }
    Err(unavailable())
}

fn driver_in_path(file: &File, name: &str) -> io::Result<bool> {
    let wide: Vec<u16> = name.encode_utf16().collect();
    // FILE_FS_DRIVER_PATH_INFORMATION requires 8-byte alignment. Its counted
    // DriverName starts at byte 8; allocate at least the 12-byte base struct.
    let length = 8 + wide.len() * 2;
    let mut buffer = vec![0_u64; length.max(12).div_ceil(8)];
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), size_of_val(&*buffer))
    };
    bytes[4..8].copy_from_slice(&((wide.len() * 2) as u32).to_ne_bytes());
    for (word, target) in wide.iter().zip(bytes[8..length].chunks_exact_mut(2)) {
        target.copy_from_slice(&word.to_ne_bytes());
    }
    let mut status = IoStatus::default();
    let result = unsafe {
        NtQueryVolumeInformationFile(
            file.as_raw_handle(),
            &mut status,
            buffer.as_mut_ptr().cast(),
            size_of_val(&*buffer) as u32,
            9, // FileFsDriverPathInformation
        )
    };
    if result == 0xC0000034_u32 as i32 {
        return Ok(false); // Driver was unloaded after object enumeration.
    }
    nt_result(result)?;
    if status.information < 12 || status.information > size_of_val(&*buffer) {
        return Err(io::Error::other("Invalid filesystem driver query"));
    }
    // This filesystem-independent kernel query does not send an IRP to the
    // provider. It tests the stack attached to this held handle, including aliases.
    Ok(buffer[0].to_ne_bytes()[0] != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_google_filesystem_driver_names() {
        for name in ["googledrivefs", "googledrivefs31931", "GoogleDriveFS123"] {
            assert!(is_google_drive_driver(name));
        }
        for name in [
            "Google Drive",
            "FAT32",
            "Ntfs",
            "googledrivefs.sys",
            "googledrivefs1extra",
        ] {
            assert!(!is_google_drive_driver(name));
        }
    }

    #[test]
    fn driver_query_distinguishes_the_actual_stack_from_an_unrelated_driver() {
        let temp = tempfile::tempdir().unwrap();
        let (_, file) = super::super::open_root(temp.path()).unwrap();
        if super::super::has_ntfs_file_ids(&file) {
            assert!(driver_in_path(&file, "\\FileSystem\\Ntfs").unwrap());
        }
        assert!(!driver_in_path(&file, "\\FileSystem\\CepaNonexistentTestDriver").unwrap());
        check_volume(&file).unwrap();
    }

    #[test]
    fn object_names_must_be_aligned_and_within_the_returned_buffer() {
        let buffer = [0_u64; 4];
        let mut name = UnicodeString {
            length: 2,
            maximum_length: 2,
            buffer: buffer.as_ptr().cast(),
        };
        assert_eq!(object_string(&name, &buffer, 2).unwrap(), "\0");
        assert!(object_string(&name, &buffer, 1).is_err());
        name.length = 3;
        assert!(object_string(&name, &buffer, 8).is_err());
        name.length = 2;
        name.buffer = (buffer.as_ptr() as *const u8).wrapping_add(1).cast();
        assert!(object_string(&name, &buffer, 8).is_err());
        name.buffer = null();
        assert!(object_string(&name, &buffer, 8).is_err());
    }
}
