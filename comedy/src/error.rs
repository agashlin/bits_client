use std::fmt;
use std::ptr::NonNull;
use std::result;

use failure::Fail;

use winapi::shared::minwindef::DWORD;
use winapi::shared::winerror::{HRESULT, SUCCEEDED};
use winapi::um::errhandlingapi::GetLastError;

#[derive(Debug, Default, Eq, Fail, PartialEq)]
pub struct Error {
    pub code: Option<ErrorCode>,
    pub function: Option<&'static str>,
    pub file_line: Option<FileLine>,
}

use self::ErrorCode::*;

impl Error {
    pub fn function(self, function: &'static str) -> Error {
        Error {
            function: Some(function),
            ..self
        }
    }

    pub fn file_line(self, file: &'static str, line: u32) -> Error {
        Error {
            file_line: Some(FileLine(file, line)),
            ..self
        }
    }

    pub fn is_nullptr(&self) -> bool {
        if let Some(NullPtr) = self.code {
            true
        } else {
            false
        }
    }

    pub fn get_rc(&self) -> Option<DWORD> {
        if let Some(Rc(rc)) = self.code {
            Some(rc)
        } else {
            None
        }
    }

    pub fn get_last_error(&self) -> Option<DWORD> {
        if let Some(LastError(last_err)) = self.code {
            Some(last_err)
        } else {
            None
        }
    }

    pub fn get_hresult(&self) -> Option<HRESULT> {
        if let Some(HResult(hr)) = self.code {
            Some(hr)
        } else {
            None
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ErrorCode {
    NullPtr,
    Rc(DWORD),
    LastError(DWORD),
    HResult(HRESULT),
}

#[derive(Debug, Eq, PartialEq)]
pub struct FileLine(pub &'static str, pub u32);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> result::Result<(), fmt::Error> {
        if let Some(function) = self.function {
            write!(f, "{} ", function)?;

            if let Some(FileLine(file, line)) = self.file_line {
                write!(f, "{}:{} ", file, line)?;
            }

            write!(f, "failed.")?;
        }

        if self.function.is_some() && self.code.is_some() {
            write!(f, " ")?;
        }

        if let Some(ref ec) = self.code {
            match ec {
                NullPtr => write!(f, "null pointer")?,
                Rc(rc) => write!(f, "rc = {:#010x}", rc)?,
                LastError(rc) => write!(f, "GetLastError = {:#010x}", rc)?,
                HResult(hr) => write!(f, "hr = {:#010x}", hr)?,
            };
        }

        Ok(())
    }
}

pub type Result<T> = result::Result<T, Error>;

pub trait ResultExt<T> {
    fn function(self, function: &'static str) -> Result<T>;

    fn file_line(self, file: &'static str, line: u32) -> Result<T>;

    fn allow_err(self, code: ErrorCode, replacement: T) -> Result<T>;

    fn allow_nullptr(self, replacement: T) -> Result<T>;

    fn allow_rc(self, rc: DWORD, replacement: T) -> Result<T>;

    fn allow_last_error(self, last_err: DWORD, replacement: T) -> Result<T>;

    fn allow_hresult(self, hr: HRESULT, replacement: T) -> Result<T>;

    fn allow_err_with<F>(self, code: ErrorCode, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T;

    fn allow_nullptr_with<F>(self, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T;

    fn allow_rc_with<F>(self, rc: DWORD, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T;

    fn allow_last_error_with<F>(self, last_err: DWORD, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T;

    fn allow_hresult_with<F>(self, hr: HRESULT, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T;
}

impl<T> ResultExt<T> for Result<T> {
    fn function(self, function: &'static str) -> Result<T> {
        self.map_err(|e| e.function(function))
    }

    fn file_line(self, file: &'static str, line: u32) -> Result<T> {
        self.map_err(|e| e.file_line(file, line))
    }

    fn allow_err(self, code: ErrorCode, replacement: T) -> Result<T> {
        match self {
            Ok(r) => Ok(r),
            Err(ref e) if e.code == Some(code) => Ok(replacement),
            Err(e) => Err(e),
        }
    }

    fn allow_nullptr(self, replacement: T) -> Result<T> {
        self.allow_err(NullPtr, replacement)
    }

    fn allow_rc(self, rc: DWORD, replacement: T) -> Result<T> {
        self.allow_err(Rc(rc), replacement)
    }

    fn allow_last_error(self, last_err: DWORD, replacement: T) -> Result<T> {
        self.allow_err(LastError(last_err), replacement)
    }

    fn allow_hresult(self, hr: HRESULT, replacement: T) -> Result<T> {
        self.allow_err(HResult(hr), replacement)
    }

    fn allow_err_with<F>(self, code: ErrorCode, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T,
    {
        match self {
            Ok(r) => Ok(r),
            Err(ref e) if e.code == Some(code) => Ok(replacement()),
            Err(e) => Err(e),
        }
    }

    fn allow_nullptr_with<F>(self, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T,
    {
        self.allow_err_with(NullPtr, replacement)
    }

    fn allow_rc_with<F>(self, rc: DWORD, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T,
    {
        self.allow_err_with(Rc(rc), replacement)
    }

    fn allow_last_error_with<F>(self, last_err: DWORD, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T,
    {
        self.allow_err_with(LastError(last_err), replacement)
    }

    fn allow_hresult_with<F>(self, hr: HRESULT, replacement: F) -> Result<T>
    where
        F: FnOnce() -> T,
    {
        self.allow_err_with(HResult(hr), replacement)
    }
}

pub fn succeeded_or_err(hr: HRESULT) -> Result<HRESULT> {
    if !SUCCEEDED(hr) {
        Err(Error {
            code: Some(HResult(hr)),
            function: None,
            file_line: None,
        })
    } else {
        Ok(hr)
    }
}

#[macro_export]
macro_rules! check_succeeded {
    ($f:ident ( $($arg:expr),* )) => {
        {
            use $crate::error::ResultExt;
            $crate::error::succeeded_or_err($f($($arg),*))
                .function(stringify!($f))
                .file_line(file!(), line!())
        }
    };

    // support for trailing comma in argument list
    ($f:ident ( $($arg:expr),+ , )) => {
        $crate::check_succeeded!($f($($arg),+))
    };
}

pub fn true_or_last_err<T>(rv: T) -> Result<T>
where
    T: Eq,
    T: From<bool>,
{
    if rv == T::from(false) {
        Err(Error {
            code: Some(LastError(unsafe { GetLastError() })),
            function: None,
            file_line: None,
        })
    } else {
        Ok(rv)
    }
}

#[macro_export]
macro_rules! check_true {
    ($f:ident ( $($arg:expr),* )) => {
        {
            use $crate::error::ResultExt;
            $crate::error::true_or_last_err($f($($arg),*))
                .function(stringify!($f))
                .file_line(file!(), line!())
        }
    };

    // support for trailing comma in argument list
    ($f:ident ( $($arg:expr),+ , )) => {
        $crate::check_true!($f($($arg),+))
    };
}

pub fn nonnull_or_last_err<T>(p: *mut T) -> Result<NonNull<T>> {
    match NonNull::new(p) {
        None => Err(Error {
            code: Some(LastError(unsafe { GetLastError() })),
            function: None,
            file_line: None,
        }),
        Some(p) => Ok(p),
    }
}
