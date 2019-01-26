use std::panic::{catch_unwind, RefUnwindSafe};
use std::sync::Mutex;

use comedy::Error;
use guid_win::Guid;
use winapi::ctypes::c_void;
use winapi::shared::guiddef::REFIID;
use winapi::shared::minwindef::DWORD;
use winapi::shared::ntdef::ULONG;
use winapi::shared::winerror::{E_FAIL, E_NOINTERFACE, HRESULT, NOERROR, S_OK};
use winapi::um::bits::{
    IBackgroundCopyCallback, IBackgroundCopyCallbackVtbl, IBackgroundCopyError, IBackgroundCopyJob,
};
use winapi::um::unknwnbase::{IUnknown, IUnknownVtbl};
use winapi::Interface;

use BitsJob;

pub type TransferredCallback =
    (Fn() -> Result<(), HRESULT>) + RefUnwindSafe + Send + Sync + 'static;
pub type ErrorCallback =
    (Fn() -> Result<(), HRESULT>) + RefUnwindSafe + Send + Sync + 'static;
pub type ModificationCallback =
    (Fn() -> Result<(), HRESULT>) + RefUnwindSafe + Send + Sync + 'static;

#[repr(C)]
pub struct BackgroundCopyCallback {
    interface: IBackgroundCopyCallback,
    rc: Mutex<ULONG>,
    transferred_cb: Option<Box<TransferredCallback>>,
    error_cb: Option<Box<ErrorCallback>>,
    modification_cb: Option<Box<ModificationCallback>>,
}

impl BackgroundCopyCallback {
    pub fn register(
        job: &mut BitsJob,
        transferred_cb: Option<Box<TransferredCallback>>,
        error_cb: Option<Box<ErrorCallback>>,
        modification_cb: Option<Box<ModificationCallback>>) -> Result<(), Error>
    {
        let mut cb = Box::new(BackgroundCopyCallback {
            interface: IBackgroundCopyCallback {
                lpVtbl: &VTBL,
            },
            rc: Mutex::new(1),
            transferred_cb,
            error_cb,
            modification_cb,
        });

        assert!(&mut *cb as *mut BackgroundCopyCallback as *mut IUnknownVtbl ==
                &mut cb.interface as *mut IBackgroundCopyCallback as *mut IUnknownVtbl);

        let cb = Box::leak(cb) as *mut BackgroundCopyCallback as *mut IUnknown;

        unsafe {
            job.set_notify_interface(cb)?;
            (*cb).Release();
        };

        Ok(())
    }
}

extern "system" fn query_interface(
    this: *mut IUnknown,
    riid: REFIID,
    obj: *mut *mut c_void,
) -> HRESULT {
    unsafe {
        if Guid(*riid) == Guid(IUnknown::uuidof())
            || Guid(*riid) == Guid(IBackgroundCopyCallback::uuidof())
        {
            addref(this);
            *obj = this as *mut c_void;
            NOERROR
        } else {
            E_NOINTERFACE
        }
    }
}

extern "system" fn addref(this: *mut IUnknown) -> ULONG {
    unsafe {
        let this = this as *const BackgroundCopyCallback;
        if let Ok(mut rc) = (*this).rc.lock() {
            *rc += 1;
            *rc
        } else {
            // HACK
            // Can't reliably panic, what to do?
            1
        }
    }
}

extern "system" fn release(this: *mut IUnknown) -> ULONG {
    unsafe {
        let this = this as *const BackgroundCopyCallback;
        if let Ok(mut rc) = (*this).rc.lock() {
            *rc -= 1;

            if *rc > 0 {
                return *rc;
            } else {
                // fall through (to get out of the scope of *this above)
            }
        } else {
            // HACK
            // Can't reliably panic, what to do?
            return 1;
        }


        // rc will have been 0 when we get here.
        // re-Box in order to drop it.
        let _ = Box::from_raw(this as *mut BackgroundCopyCallback);

        return 0;
    }
}

extern "system" fn transferred_stub(
    this: *mut IBackgroundCopyCallback,
    _job: *mut IBackgroundCopyJob,
) -> HRESULT {
    unsafe {
        let this = this as *const BackgroundCopyCallback;
        if let Some(ref cb) = (*this).transferred_cb {
            match catch_unwind(|| { cb() }) {
                Ok(Ok(())) => S_OK,
                Ok(Err(hr)) => hr,
                Err(_) => E_FAIL,
            }
        } else {
            S_OK
        }
    }
}

extern "system" fn error_stub(
    this: *mut IBackgroundCopyCallback,
    _job: *mut IBackgroundCopyJob,
    _error: *mut IBackgroundCopyError,
) -> HRESULT {
    unsafe {
        let this = this as *const BackgroundCopyCallback;
        if let Some(ref cb) = (*this).error_cb {
            match catch_unwind(|| { cb() }) {
                Ok(Ok(())) => S_OK,
                Ok(Err(hr)) => hr,
                Err(_) => E_FAIL,
            }
        } else {
            S_OK
        }
    }
}

extern "system" fn modification_stub(
    this: *mut IBackgroundCopyCallback,
    _job: *mut IBackgroundCopyJob,
    _reserved: DWORD,
) -> HRESULT {
    unsafe {
        let this = this as *const BackgroundCopyCallback;
        if let Some(ref cb) = (*this).modification_cb {
            match catch_unwind(|| { cb() }) {
                Ok(Ok(())) => S_OK,
                Ok(Err(hr)) => hr,
                Err(_) => E_FAIL,
            }
        } else {
            S_OK
        }
    }
}

pub static VTBL: IBackgroundCopyCallbackVtbl = IBackgroundCopyCallbackVtbl {
    parent: IUnknownVtbl {
        QueryInterface: query_interface,
        AddRef: addref,
        Release: release,
    },
    JobTransferred: transferred_stub,
    JobError: error_stub,
    JobModification: modification_stub,
};
