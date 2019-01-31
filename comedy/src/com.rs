use std::marker::PhantomData;
use std::ptr;
use std::rc::Rc;
use std::result::Result::*;

use winapi::shared::{
    guiddef::GUID,
    minwindef::DWORD,
    winerror::{CO_E_NOTINITIALIZED, HRESULT},
    wtypesbase::{CLSCTX, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER},
};
use winapi::um::{
    cguid::CLSID_StdGlobalInterfaceTable,
    combaseapi::{CoCreateInstance, CoInitializeEx, CoUninitialize},
    objbase::{COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED},
    objidlbase::IGlobalInterfaceTable,
};
use winapi::{Class, Interface};
use wio::com::ComPtr;

use check_succeeded;
use error::{succeeded_or_err, Error, ErrorCode::*, Result, ResultExt};

use com_call;

#[derive(Debug, Default)]
pub struct ComApartmentScope {
    /// PhantomData used in lieu of unstable impl !Send + !Sync.
    /// It must be dropped on the same thread it was created on so it can't be Send,
    /// and references are meant to indicate that COM has been inited on the current thread so it
    /// can't be Sync.
    _do_not_send: PhantomData<Rc<()>>,
}

impl ComApartmentScope {
    /// This thread should be the sole occupant of a single thread apartment
    pub fn init_sta() -> Result<Self> {
        unsafe { check_succeeded!(CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED)) }?;

        Ok(Default::default())
    }

    /// This thread should join the process's multi thread apartment
    pub fn init_mta() -> Result<Self> {
        unsafe { check_succeeded!(CoInitializeEx(ptr::null_mut(), COINIT_MULTITHREADED)) }?;

        Ok(Default::default())
    }
}

impl Drop for ComApartmentScope {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

thread_local! {
    // TODO these examples should probably be in convenience functions.
    /// A single thread apartment scope for the duration of the current thread.
    /// ```
    /// use comedy::com::{ComApartmentScope, INIT_STA};
    ///
    /// fn do_com_stuff(_com: &ComApartmentScope) {
    /// }
    ///
    /// INIT_STA.with(|com| {
    ///     let com = match com {
    ///         Err(e) => return Err(e.clone()),
    ///         Ok(ref com) => com,
    ///     };
    ///     do_com_stuff(com);
    ///     Ok(())
    /// }).unwrap()
    /// ```
    pub static INIT_STA: Result<ComApartmentScope> = ComApartmentScope::init_sta();

    /// A multithread apartment scope for the duration of the current thread.
    /// ```
    /// use comedy::com::{ComApartmentScope, INIT_MTA};
    ///
    /// fn do_com_stuff(_com: &ComApartmentScope) {
    /// }
    ///
    /// INIT_MTA.with(|com| {
    ///     let com = match com {
    ///         Err(e) => return Err(e.clone()),
    ///         Ok(ref com) => com,
    ///     };
    ///     do_com_stuff(com);
    ///     Ok(())
    /// }).unwrap()
    /// ```
    pub static INIT_MTA: Result<ComApartmentScope> = ComApartmentScope::init_mta();
}

// TODO something with CoInitializeSecurity

enum StdGlobalInterfaceTable {}

impl Class for StdGlobalInterfaceTable {
    fn uuidof() -> GUID {
        CLSID_StdGlobalInterfaceTable
    }
}

pub struct GlobalInterface<T: Interface> {
    cookie: DWORD,
    _interface_phantom: PhantomData<T>,
}

unsafe impl<T: Interface> Send for GlobalInterface<T> {}

impl<T: Interface> GlobalInterface<T> {
    fn get_git() -> Result<ComPtr<IGlobalInterfaceTable>> {
        create_instance_inproc_server::<StdGlobalInterfaceTable, IGlobalInterfaceTable>()
    }

    pub fn new(_com: &ComApartmentScope, v: ComPtr<T>) -> Result<Self> {
        let git = Self::get_git()?;
        unsafe {
            let mut cookie = 0;

            com_call!(
                git,
                IGlobalInterfaceTable::RegisterInterfaceInGlobal(
                    v.as_raw() as *mut _,
                    &T::uuidof(),
                    &mut cookie,
                )
            )?;

            Ok(GlobalInterface {
                cookie,
                _interface_phantom: Default::default(),
            })
        }
    }

    pub fn get(&self, _com: &ComApartmentScope) -> Result<ComPtr<T>> {
        let git = Self::get_git()?;
        unsafe {
            let mut raw_v = ptr::null_mut();
            com_call!(
                git,
                IGlobalInterfaceTable::GetInterfaceFromGlobal(
                    self.cookie,
                    &T::uuidof(),
                    &mut raw_v,
                )
            )?;

            Ok(ComPtr::from_raw(raw_v as *mut T))
        }
    }
}

impl<T: Interface> Drop for GlobalInterface<T> {
    fn drop(&mut self) {
        // In case we return early on error, the worst that will happen is the interface is leaked.

        let mut _com = None;
        let git = match Self::get_git() {
            Ok(v) => v,
            Err(ref e) if e.get_hresult() == Some(CO_E_NOTINITIALIZED) => {
                // COM wasn't initialized yet on this thread, try initializing.
                _com = if let Ok(com) = ComApartmentScope::init_sta() {
                    Some(com)
                } else {
                    // Couldn't initialize COM.
                    return;
                };

                match Self::get_git() {
                    Ok(v) => v,
                    // Couldn't get GIT even after initializing COM.
                    Err(_) => return,
                }
            }
            // Couldn't get GIT for some other reason.
            Err(_) => return,
        };

        unsafe {
            let _ = com_call!(
                git,
                IGlobalInterfaceTable::RevokeInterfaceFromGlobal(self.cookie)
            );
        }
    }
}

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

    // support for trailing comma in argument list
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
