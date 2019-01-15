use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::result::Result;

use winapi::shared::minwindef::{DWORD, MAX_PATH};
use winapi::um::processthreadsapi::GetCurrentProcess;
use winapi::um::winbase::QueryFullProcessImageNameW;

use check_true;
use error::Error;

pub fn current_process_image_name() -> Result<OsString, Error> {
    let mut image_path = [0u16; MAX_PATH + 1];
    let mut image_path_size_chars = (image_path.len() - 1) as DWORD;

    unsafe {
        check_true!(QueryFullProcessImageNameW(
            GetCurrentProcess(),
            0, // dwFlags
            image_path.as_mut_ptr(),
            &mut image_path_size_chars as *mut _,
        ))
    }?;

    Ok(OsString::from_wide(
        &image_path[..image_path_size_chars as usize],
    ))
}
