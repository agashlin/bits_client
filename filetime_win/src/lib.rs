extern crate comedy;
#[cfg(feature = "filetime_serde")]
extern crate serde;
#[cfg(feature = "filetime_serde")]
extern crate serde_derive;
extern crate winapi;

use std::fmt::{Debug, Display, Formatter, Result};
use std::mem;
use std::result;

use comedy::check_true;

use winapi::shared::minwindef::FILETIME;
#[cfg(feature = "filetime_serde")]
use winapi::shared::minwindef::{DWORD, WORD};
use winapi::um::minwinbase::SYSTEMTIME;
use winapi::um::timezoneapi::FileTimeToSystemTime;

#[cfg(feature = "filetime_serde")]
use serde_derive::{Deserialize, Serialize};

#[cfg(feature = "filetime_serde")]
#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(remote = "FILETIME")]
#[repr(C)]
struct FileTimeSerde {
    pub dwLowDateTime: DWORD,
    pub dwHighDateTime: DWORD,
}

#[derive(Copy, Clone)]
#[cfg_attr(feature = "filetime_serde", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct FileTime(
    #[cfg_attr(feature = "filetime_serde", serde(with = "FileTimeSerde"))] pub FILETIME,
);

#[cfg(feature = "filetime_serde")]
#[allow(non_snake_case)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(remote = "SYSTEMTIME")]
#[repr(C)]
struct SystemTimeSerde {
    pub wYear: WORD,
    pub wMonth: WORD,
    pub wDayOfWeek: WORD,
    pub wDay: WORD,
    pub wHour: WORD,
    pub wMinute: WORD,
    pub wSecond: WORD,
    pub wMilliseconds: WORD,
}

#[derive(Copy, Clone)]
#[cfg_attr(feature = "filetime_serde", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct SystemTimeUTC(
    #[cfg_attr(feature = "filetime_serde", serde(with = "SystemTimeSerde"))] pub SYSTEMTIME,
);

impl FileTime {
    pub fn to_u64(&self) -> u64 {
        ((self.0.dwHighDateTime as u64) << 32) | (self.0.dwLowDateTime as u64)
    }
    pub fn to_system_time_utc(&self) -> result::Result<SystemTimeUTC, comedy::Error> {
        unsafe {
            let mut system_time = mem::zeroed();

            check_true!(FileTimeToSystemTime(&self.0, &mut system_time))?;

            Ok(SystemTimeUTC(system_time))
        }
    }
}

impl PartialEq for FileTime {
    fn eq(&self, other: &FileTime) -> bool {
        self.0.dwLowDateTime == other.0.dwLowDateTime
            && self.0.dwHighDateTime == other.0.dwHighDateTime
    }
}

impl Eq for FileTime {}

impl Debug for FileTime {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(
            f,
            "FileTime {{ dwLowDateTime: {:?}, dwHighDateTime: {:?} }}",
            self.0.dwLowDateTime, self.0.dwHighDateTime
        )
    }
}

impl Debug for SystemTimeUTC {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "SystemTimeUTC {{ wYear: {:?}, wMonth: {:?}, wDayOfWeek: {:?}, wDay: {:?}, wHour: {:?}, wMinute: {:?}, wSecond: {:?}, wMilliseconds: {:?}",
               self.0.wYear, self.0.wMonth, self.0.wDayOfWeek, self.0.wDay,
               self.0.wHour, self.0.wMinute, self.0.wSecond, self.0.wMilliseconds)
    }
}

impl Display for SystemTimeUTC {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            self.0.wYear,
            self.0.wMonth,
            self.0.wDay,
            self.0.wHour,
            self.0.wMinute,
            self.0.wSecond,
            self.0.wMilliseconds
        )
    }
}
