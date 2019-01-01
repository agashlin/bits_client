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
}

#[derive(Debug, Eq, PartialEq)]
pub enum ErrorCode {
    NullPtr,
    Rc(DWORD),
    LastError(DWORD),
    HResult(HRESULT),
}
use self::ErrorCode::*;

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

    fn allow_err_with<F>(self, code: ErrorCode, replacement: F) -> Result<T>
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
