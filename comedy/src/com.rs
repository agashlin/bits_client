use std::ptr;
use std::result::Result::*;

use winapi::shared::{
    winerror::HRESULT,
    wtypesbase::{CLSCTX, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER},
};
use winapi::um::{
    combaseapi::{CoCreateInstance, CoInitializeEx, CoUninitialize},
    objbase::{COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED},
};
use winapi::{Class, Interface};
use wio::com::ComPtr;

use check_succeeded;
use error::{succeeded_or_err, Error, ErrorCode::*, Result, ResultExt};

#[derive(Debug)]
pub struct InitCom {
    _init_only: (),
}

impl InitCom {
    /// This thread should be the sole occupant of a single thread apartment
    pub fn init_sta() -> Result<Self> {
        unsafe { check_succeeded!(CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED)) }?;

        Ok(InitCom { _init_only: () })
    }

    /// This thread should join the process's multi thread apartment
    pub fn init_mta() -> Result<Self> {
        unsafe { check_succeeded!(CoInitializeEx(ptr::null_mut(), COINIT_MULTITHREADED)) }?;

        Ok(InitCom { _init_only: () })
    }
}

impl Drop for InitCom {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

// TODO something with CoInitializeSecurity

pub fn create_instance<C, I>(ctx: CLSCTX) -> Result<ComPtr<I>>
where
    C: Class,
    I: Interface,
{
    get(|interface| unsafe {
        CoCreateInstance(
            &C::uuidof(),
            ptr::null_mut(), // pUnkOuter
            ctx,
            &I::uuidof(),
            interface as *mut *mut _,
        )
    })
    .function("CoCreateInstance")
    .file_line(file!(), line!())
}

pub fn create_instance_local_server<C, I>() -> Result<ComPtr<I>>
where
    C: Class,
    I: Interface,
{
    create_instance::<C, I>(CLSCTX_LOCAL_SERVER)
}

pub fn create_instance_inproc_server<C, I>() -> Result<ComPtr<I>>
where
    C: Class,
    I: Interface,
{
    create_instance::<C, I>(CLSCTX_INPROC_SERVER)
}

pub fn cast<I, J>(interface: ComPtr<I>) -> Result<ComPtr<J>>
where
    I: Interface,
    J: Interface,
{
    interface.cast().map_err(|hr| Error {
        code: Some(HResult(hr)),
        function: Some("IUnknown::QueryInterface"),
        file_line: None,
    })
}

/// Call a method.
#[macro_export]
macro_rules! com_call {
    ($obj:expr, $interface:ident :: $method:ident ( $($arg:expr),* )) => {
        $crate::error::succeeded_or_err({
            let obj: &$interface = &*$obj;
            obj.$method($($arg),*)
        }).function(concat!(stringify!($interface), "::", stringify!($method)))
          .file_line(file!(), line!())
    };
    // support for trailing command in argument list
    ($obj:expr, $interface:ident :: $method:ident ( $($arg:expr),+ , )) => {
        $crate::com_call!($obj, $interface::$method($($arg),+))
    };
}

pub fn get<I, F>(getter: F) -> Result<ComPtr<I>>
where
    I: Interface,
    F: FnOnce(*mut *mut I) -> HRESULT,
{
    let mut interface: *mut I = ptr::null_mut();

    // Throw away successful HRESULT.
    succeeded_or_err(getter(&mut interface as *mut *mut I))?;

    if interface.is_null() {
        Err(Error {
            code: Some(NullPtr),
            function: None,
            file_line: None,
        })
    } else {
        Ok(unsafe { ComPtr::from_raw(interface) })
    }
}

/// Call a method, getting an interface pointer that is returned through an output parameter.
#[macro_export]
macro_rules! com_call_getter {
    (| $outparam:ident | $obj:expr, $interface:ident :: $method:ident ( $($arg:expr),* )) => {{
        let obj: &$interface = &*$obj;
        $crate::com::get(|$outparam| {
            obj.$method($($arg),*)
        }).function(concat!(stringify!($interface), "::", stringify!($method)))
          .file_line(file!(), line!())
    }};
    // support for trailing comma in argument list
    (| $outparam:ident | $obj:expr, $interface:ident :: $method:ident ( $($arg:expr),+ , )) => {
        $crate::com_call_getter!(|$outparam| $obj, $interface::$method($($arg),+))
    };
}
