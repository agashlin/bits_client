use std::ops::Deref;

use winapi::shared::minwindef::{DWORD, HLOCAL};
use winapi::shared::ntdef::NULL;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::winbase::LocalFree;
use winapi::um::winnt::HANDLE;

#[repr(transparent)]
#[derive(Debug)]
pub struct Handle(HANDLE);

impl Handle {
    /// Take ownership of a `HANDLE`, which will be closed with `CloseHandle` upon drop.
    /// Checks for `INVALID_HANDLE_VALUE` but not `NULL`.
    ///
    /// # Safety
    ///
    /// `h` should be the only copy of the handle. `GetLastError` is called to
    /// return an error, so the last Windows API called should have been what produced
    /// the invalid handle.
    pub unsafe fn wrap_valid(h: HANDLE) -> Result<Handle, DWORD> {
        if h == INVALID_HANDLE_VALUE {
            Err(GetLastError())
        } else {
            Ok(Handle(h))
        }
    }

    /// Take ownership of a `HANDLE`, which will be closed with `CloseHandle` upon drop.
    /// Checks for `NULL` but not `INVALID_HANDLE_VALUE`.
    ///
    /// # Safety
    ///
    /// `h` should be the only copy of the handle. `GetLastError` is called to
    /// return an error, so the last Windows API called should have been what produced
    /// the invalid handle.
    pub unsafe fn wrap_nonnull(h: HANDLE) -> Result<Handle, DWORD> {
        if h == NULL {
            Err(GetLastError())
        } else {
            Ok(Handle(h))
        }
    }
}

impl Deref for Handle {
    type Target = HANDLE;
    fn deref(&self) -> &HANDLE {
        &self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[macro_export]
macro_rules! call_valid_handle_getter {
    ($f:ident ( $($arg:expr),* )) => {
        {
            use $crate::error::{Error, ErrorCode, FileLine};
            $crate::handle::Handle::wrap_valid($f($($arg),*))
                .map_err(|rc| Error {
                    code: Some(ErrorCode::LastError(rc)),
                    function: Some(stringify!($f)),
                    file_line: Some(FileLine(file!(), line!())),
                })
        }
    };

    // support for trailing comma in argument list
    ($f:ident ( $($arg:expr),+ , )) => {
        $crate::call_valid_handle_getter!($f($($arg),*))
    };
}

#[macro_export]
macro_rules! call_nonnull_handle_getter {
    ($f:ident ( $($arg:expr),* )) => {
        {
            use $crate::error::{Error, ErrorCode, FileLine};
            $crate::handle::Handle::wrap_nonnull($f($($arg),*))
                .map_err(|rc| Error {
                    code: Some(ErrorCode::LastError(rc)),
                    function: Some(stringify!($f)),
                    file_line: Some(FileLine(file!(), line!())),
                })
        }
    };

    // support for trailing comma in argument list
    ($f:ident ( $($arg:expr),+ , )) => {
        $crate::call_nonnull_handle_getter!($f($($arg),*))
    };
}

#[repr(transparent)]
#[derive(Debug)]
pub struct HLocal(HLOCAL);

impl HLocal {
    /// Take ownership of a `HLOCAL`, which will be closed with `LocalFree` upon drop.
    /// Checks for `NULL`.
    ///
    /// # Safety
    ///
    /// `h` should be the only copy of the handle. `GetLastError` is called to
    /// return an error, so the last Windows API called should have been what produced
    /// the invalid handle.
    pub unsafe fn wrap(h: HLOCAL) -> Result<HLocal, DWORD> {
        if h == NULL {
            Err(GetLastError())
        } else {
            Ok(HLocal(h))
        }
    }
}

impl Deref for HLocal {
    type Target = HLOCAL;
    fn deref(&self) -> &HLOCAL {
        &self.0
    }
}

impl Drop for HLocal {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
