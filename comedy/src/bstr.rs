use std::fmt;
use std::mem;
use std::ptr;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use winapi::shared::minwindef::UINT;
use winapi::shared::wtypes::BSTR;
use winapi::shared::wtypesbase::OLECHAR;
use winapi::um::oleauto::{SysAllocStringLen, SysFreeString, SysStringLen};

// Originally from winrt 0.5.0

/// A wrapper around `BSTR`, a string type used by classic COM.
#[derive(Debug)]
pub struct BStr(BSTR);

impl<'a> From<&'a str> for BStr {
    fn from(s: &'a str) -> Self {
        // Every UTF-8 byte results in either 1 or 2 UTF-16 bytes. This size expectation is
        // correct in most cases, so the vector doesn't need to reallocate.
        let mut s16: Vec<u16> = Vec::with_capacity(s.len());
        for c in s.encode_utf16() {
            s16.push(c);
        }
        let len = s16.len();
        let slice: &[u16] = &s16;
        let bstr = unsafe { SysAllocStringLen(slice as *const _ as *const OLECHAR, len as UINT) };
        BStr(bstr)
    }
}

impl<'a> From<&'a OsStr> for BStr {
    fn from(s: &'a OsStr) -> Self {
        let s16: Vec<u16> = s.encode_wide().collect();
        let len = s16.len();
        let bstr = unsafe { SysAllocStringLen(s16.as_ptr() as *const OLECHAR, len as UINT) };
        BStr(bstr)
    }
}

impl<'a> From<&'a [u16]> for BStr {
    fn from(s: &'a [u16]) -> Self {
        let len = s.len();
        let bstr = unsafe { SysAllocStringLen(s.as_ptr() as *const OLECHAR, len as UINT) };
        BStr(bstr)
    }
}

impl<'a> From<&'a BStr> for String {
    fn from(bs: &'a BStr) -> String {
        bs.internal_to_string()
    }
}

impl BStr {
    #[inline(always)]
    pub fn get(&self) -> BSTR {
        self.0
    }

    #[inline(always)]
    pub unsafe fn wrap(bstr: BSTR) -> BStr {
        BStr(bstr)
    }

    #[inline(always)]
    pub fn empty() -> BStr {
        BStr(ptr::null_mut())
    }

    #[inline(always)]
    pub fn get_address(&mut self) -> &mut BSTR {
        &mut self.0
    }

    #[inline(always)]
    pub fn len(&self) -> u32 {
        // This is okay even if pointer is null (returns 0)
        unsafe { SysStringLen(self.0) }
    }

    #[inline(always)]
    fn internal_to_string(&self) -> String {
        if self.0.is_null() {
            String::new()
        } else {
            unsafe {
                let len = self.len();
                let slice: &[u16] = ::std::slice::from_raw_parts(self.0, len as usize);
                String::from_utf16_lossy(slice)
            }
        }
    }

    #[inline(always)]
    pub fn take(&mut self) -> BSTR {
        mem::replace(&mut self.0, ptr::null_mut())
    }
}

#[cfg(feature = "nightly")]
impl ToString for BStr {
    fn to_string(&self) -> String {
        self.internal_to_string()
    }
}

impl fmt::Display for BStr {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        formatter.write_str(&self.internal_to_string())
    }
}

impl Drop for BStr {
    #[inline(always)]
    fn drop(&mut self) {
        // This is okay even if the pointer is null
        unsafe { SysFreeString(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = "12345";
        let bstr: BStr = s.into();
        assert!(bstr.len() as usize == s.len());
        assert!(s == bstr.to_string());
    }

    #[test]
    fn empty() {
        let bstr = BStr::empty();
        assert!(bstr.len() == 0);
        assert!(bstr.to_string().len() == 0);
    }
}

#[cfg(all(test, feature = "nightly"))]
mod nightly_tests {
    use self::test::Bencher;

    #[bench]
    fn bench_create(b: &mut Bencher) {
        let s = "123456789";
        b.iter(|| {
            let _: BStr = s.into();
        });;
    }

    #[bench]
    fn bench_to_string(b: &mut Bencher) {
        let bstr: BStr = "123456789".into();
        b.iter(|| {
            let _ = bstr.to_string();
        });
    }
}
