extern crate comedy;
extern crate winapi;
extern crate wio;

use std::ffi::OsString;
use std::fmt::{Debug, Display, Error, Formatter, Result};
use std::iter;
use std::mem;
use std::result;
use std::str::FromStr;

use comedy::check_succeeded;

use winapi::ctypes;
use winapi::shared::guiddef::GUID;
use winapi::um::combaseapi::{CLSIDFromString, StringFromGUID2};
use wio::wide::{FromWide, ToWide};

#[cfg(feature = "guid_serde")]
use serde_derive::{Deserialize, Serialize};

const GUID_STRING_CHARACTERS: usize = 38;

#[cfg(feature = "guid_serde")]
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize)]
#[serde(remote = "GUID")]
#[repr(C)]
struct GUIDSerde {
    pub Data1: ctypes::c_ulong,
    pub Data2: ctypes::c_ushort,
    pub Data3: ctypes::c_ushort,
    pub Data4: [ctypes::c_uchar; 8],
}

#[derive(Clone)]
#[cfg_attr(feature = "guid_serde", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct Guid(#[cfg_attr(feature = "guid_serde", serde(with = "GUIDSerde"))] pub GUID);

impl PartialEq for Guid {
    fn eq(&self, other: &Guid) -> bool {
        self.0.Data1 == other.0.Data1
            && self.0.Data2 == other.0.Data2
            && self.0.Data3 == other.0.Data3
            && self.0.Data4 == other.0.Data4
    }
}

impl Eq for Guid {}

impl Debug for Guid {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{:?}", unsafe {
            &mem::transmute::<Guid, [u8; mem::size_of::<Guid>()]>(self.clone())
        })
    }
}

impl Display for Guid {
    fn fmt(&self, f: &mut Formatter) -> Result {
        let mut s: [u16; GUID_STRING_CHARACTERS + 1] = unsafe { mem::uninitialized() };

        let len = unsafe {
            StringFromGUID2(
                &(*self).0 as *const _ as *mut _,
                s.as_mut_ptr(),
                s.len() as ctypes::c_int,
            )
        };
        if len <= 0 {
            return Err(Error);
        }

        let s = &s[..len as usize];
        if let Ok(s) = OsString::from_wide_null(&s).into_string() {
            f.write_str(&s)
        } else {
            Err(Error)
        }
    }
}

impl FromStr for Guid {
    type Err = comedy::error::Error;

    fn from_str(s: &str) -> result::Result<Self, Self::Err> {
        let mut guid = unsafe { mem::uninitialized() };

        let s = if s.chars().next() == Some('{') {
            s.to_wide_null()
        } else {
            iter::once(b'{' as u16)
                .chain(s.to_wide().into_iter())
                .chain(Some(b'}' as u16))
                .chain(Some(0))
                .collect()
        };

        unsafe { check_succeeded!(CLSIDFromString(s.as_ptr(), &mut guid)) }?;

        Ok(Guid(guid))
    }
}
