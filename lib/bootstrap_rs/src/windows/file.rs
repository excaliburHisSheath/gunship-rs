use std::ffi::CString;
use std::mem;
use std::ptr;

use super::winapi::*;
use super::kernel32;

pub fn file_modified(path: &str) -> Result<u64, String> {
    let cstring = CString::new(path).unwrap();

    let handle = unsafe {
        kernel32::CreateFileA(
            cstring.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut())
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("Could not open file {}", path));
    }

    let mut file_time = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };

    let result = unsafe {  kernel32::GetFileTime(handle, ptr::null_mut(), ptr::null_mut(), &mut file_time) };
    if result == 0 {
        return Err(format!("Unable to get modified time for the file {}", path));
    }

    let result = unsafe { kernel32::CloseHandle(handle) };
    if result == 0 {
        return Err(format!("Error while closing file handle for {}", path));
    }

    Ok(unsafe { mem::transmute(file_time) })
}
