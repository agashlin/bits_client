use std::panic::{catch_unwind, RefUnwindSafe};

use comedy::guid::Guid;
use winapi::ctypes::c_void;
use winapi::shared::guiddef::REFIID;
use winapi::shared::minwindef::DWORD;
use winapi::shared::ntdef::ULONG;
use winapi::shared::winerror::{E_NOINTERFACE, HRESULT, NOERROR, S_OK};
use winapi::um::bits::{
    IBackgroundCopyCallback, IBackgroundCopyCallbackVtbl, IBackgroundCopyError, IBackgroundCopyJob,
};
use winapi::um::unknwnbase::{IUnknown, IUnknownVtbl};
use winapi::Interface;
use wio::com::ComPtr;

use {BitsJob, BitsJobError};

pub type TransferredCallback = (Fn(BitsJob) -> () + RefUnwindSafe + Send + Sync + 'static);
pub type ErrorCallback = (Fn(BitsJob, BitsJobError) -> () + RefUnwindSafe + Send + Sync + 'static);
pub type ModificationCallback = (Fn(BitsJob) -> () + RefUnwindSafe + Send + Sync + 'static);

#[repr(C)]
pub struct BackgroundCopyCallback {
    pub interface: IBackgroundCopyCallback,
    // TODO return from callback should be an error that can be logged?
    pub transferred: Option<Box<TransferredCallback>>,
    pub error: Option<Box<ErrorCallback>>,
    pub modification: Option<Box<ModificationCallback>>,
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

extern "system" fn addref(_this: *mut IUnknown) -> ULONG {
    // TODO learn Rust synchronization
    1
}

extern "system" fn release(_this: *mut IUnknown) -> ULONG {
    // TODO
    1
}

extern "system" fn transferred_stub(
    this: *mut IBackgroundCopyCallback,
    job: *mut IBackgroundCopyJob,
) -> HRESULT {
    unsafe {
        let this = this as *mut BackgroundCopyCallback;
        if let Some(ref cb) = (*this).transferred {
            // TODO: argue about this, BitsJob should probably have an unsafe from_raw that
            // does this and also ComPtr::from_raw internally
            (*job).AddRef();
            // TODO: we probably don't need to bother with catch_unwind as we'll be building
            // with abort on panic
            let result = catch_unwind(|| cb(BitsJob::from_ptr(ComPtr::from_raw(job))));
            // TODO: proper logging
            if let Err(e) = result {
                use std::io::Write;
                if let Ok(mut file) = std::fs::File::create("C:\\ProgramData\\callbackfail.log") {
                    #[allow(unused_must_use)]
                    {
                        file.write(format!("{:?}", e.downcast_ref::<String>()).as_bytes());
                    }
                }
            }
        }
    }
    S_OK
}

extern "system" fn error_stub(
    this: *mut IBackgroundCopyCallback,
    job: *mut IBackgroundCopyJob,
    error: *mut IBackgroundCopyError,
) -> HRESULT {
    unsafe {
        let this = this as *mut BackgroundCopyCallback;
        if let Some(ref cb) = (*this).error {
            (*job).AddRef();
            (*error).AddRef();
            if let Err(_e) = catch_unwind(|| {
                cb(
                    BitsJob::from_ptr(ComPtr::from_raw(job)),
                    BitsJob::get_error(ComPtr::from_raw(error)).expect("unwrapping"),
                )
            }) {
                // TODO logging
            }
        }
    }
    S_OK
}

extern "system" fn modification_stub(
    this: *mut IBackgroundCopyCallback,
    job: *mut IBackgroundCopyJob,
    _reserved: DWORD,
) -> HRESULT {
    unsafe {
        let this = this as *mut BackgroundCopyCallback;
        if let Some(ref cb) = (*this).modification {
            (*job).AddRef();
            if let Err(_e) = catch_unwind(|| cb(BitsJob::from_ptr(ComPtr::from_raw(job)))) {
                // TODO logging
            }
        }
    }
    S_OK
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
