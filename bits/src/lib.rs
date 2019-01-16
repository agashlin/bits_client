extern crate comedy;
extern crate guid_win;
extern crate winapi;
extern crate wio;

#[cfg(feature = "status_serde")]
extern crate serde;
#[cfg(feature = "status_serde")]
extern crate serde_derive;

mod callback;
pub mod status;

use std::ffi::OsStr;
use std::mem;
use std::result;

use comedy::com::create_instance_local_server;
use comedy::error::{Error, ResultExt};
use comedy::{com_call, com_call_getter};
use guid_win::Guid;
use winapi::um::bits::{
    IBackgroundCopyCallback, IBackgroundCopyError, IBackgroundCopyJob, IBackgroundCopyManager,
    BG_JOB_PRIORITY, BG_JOB_TYPE_DOWNLOAD, BG_NOTIFY_JOB_ERROR, BG_NOTIFY_JOB_MODIFICATION,
    BG_NOTIFY_JOB_TRANSFERRED, BG_SIZE_UNKNOWN,
};
use winapi::um::bitsmsg::BG_E_NOT_FOUND;
use winapi::um::unknwnbase::IUnknown;
use winapi::RIDL;
use wio::com::ComPtr;
use wio::wide::ToWide;

pub use status::{BitsJobError, BitsJobProgress, BitsJobStatus};

// reexport anything needed by clients here
pub use winapi::um::bits::{
    BG_JOB_PRIORITY_FOREGROUND, BG_JOB_PRIORITY_NORMAL, BG_JOB_STATE_CONNECTING,
    BG_JOB_STATE_ERROR, BG_JOB_STATE_TRANSFERRING, BG_JOB_STATE_TRANSIENT_ERROR,
};

type Result<T> = result::Result<T, Error>;

// temporarily here until https://github.com/retep998/winapi-rs/pull/704 is available
RIDL! {#[uuid(0x4991d34b, 0x80a1, 0x4291, 0x83, 0xb6, 0x33, 0x28, 0x36, 0x6b, 0x90, 0x97)]
class BcmClass;}

pub struct BackgroundCopyManager(ComPtr<IBackgroundCopyManager>);

impl BackgroundCopyManager {
    pub fn connect() -> Result<BackgroundCopyManager> {
        Ok(BackgroundCopyManager(create_instance_local_server::<
            BcmClass,
            IBackgroundCopyManager,
        >()?))
    }

    pub fn create_job(&self, display_name: &OsStr) -> Result<BitsJob> {
        unsafe {
            let mut guid = mem::uninitialized();
            Ok(BitsJob(com_call_getter!(
                |job| self.0,
                IBackgroundCopyManager::CreateJob(
                    display_name.to_wide_null().as_ptr(),
                    BG_JOB_TYPE_DOWNLOAD,
                    &mut guid,
                    job,
                )
            )?))
        }
    }

    pub fn get_job_by_guid(&self, guid: &Guid) -> Result<Option<BitsJob>> {
        Ok(
            unsafe { com_call_getter!(|job| self.0, IBackgroundCopyManager::GetJob(&guid.0, job)) }
                .map(|job| Some(BitsJob(job)))
                .allow_hresult(BG_E_NOT_FOUND as i32, None)?,
        )
    }
}

pub struct BitsJob(ComPtr<IBackgroundCopyJob>);

impl BitsJob {
    unsafe fn from_ptr(job: ComPtr<IBackgroundCopyJob>) -> BitsJob {
        BitsJob(job)
    }

    pub fn guid(&self) -> Result<Guid> {
        // TODO: cache on create or retrieved by GUID?
        unsafe {
            let mut guid = mem::uninitialized();
            com_call!(self.0, IBackgroundCopyJob::GetId(&mut guid))?;
            Ok(Guid(guid))
        }
    }

    pub fn add_file(&mut self, remote_url: &OsStr, local_file: &OsStr) -> Result<()> {
        unsafe {
            com_call!(
                self.0,
                IBackgroundCopyJob::AddFile(
                    remote_url.to_wide_null().as_ptr(),
                    local_file.to_wide_null().as_ptr(),
                )
            )
        }?;
        Ok(())
    }

    pub fn set_description(&mut self, description: &OsStr) -> Result<()> {
        unsafe {
            com_call!(
                self.0,
                IBackgroundCopyJob::SetDescription(description.to_wide_null().as_ptr())
            )
        }?;
        Ok(())
    }

    // TODO
    //pub fn set_proxy()

    pub fn set_priority(&mut self, priority: BG_JOB_PRIORITY) -> Result<()> {
        unsafe { com_call!(self.0, IBackgroundCopyJob::SetPriority(priority)) }?;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<()> {
        unsafe { com_call!(self.0, IBackgroundCopyJob::Resume()) }?;
        Ok(())
    }

    pub fn suspend(&mut self) -> Result<()> {
        unsafe { com_call!(self.0, IBackgroundCopyJob::Suspend()) }?;
        Ok(())
    }

    pub fn complete(&mut self) -> Result<()> {
        unsafe { com_call!(self.0, IBackgroundCopyJob::Complete()) }?;
        // TODO check for partial completion
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<()> {
        unsafe { com_call!(self.0, IBackgroundCopyJob::Cancel()) }?;
        Ok(())
    }

    pub fn register_callbacks(
        &mut self,
        transferred_cb: Option<Box<callback::TransferredCallback>>,
        error_cb: Option<Box<callback::ErrorCallback>>,
        modification_cb: Option<Box<callback::ModificationCallback>>,
    ) -> Result<()>
where {
        // TODO check via GetNotifyInterface that there isn't already a callback registered,
        // though we may want to override it anyway?
        /*if self.callback.is_some() {
            return Err(Error::Message("callback already registered".to_string()));
        }*/

        let mut flags = 0;
        if transferred_cb.is_some() {
            flags |= BG_NOTIFY_JOB_TRANSFERRED;
        }
        if error_cb.is_some() {
            flags |= BG_NOTIFY_JOB_ERROR;
        }
        if modification_cb.is_some() {
            flags |= BG_NOTIFY_JOB_MODIFICATION;
        }

        unsafe { com_call!(self.0, IBackgroundCopyJob::SetNotifyFlags(flags)) }?;

        let callback = Box::new(callback::BackgroundCopyCallback {
            interface: IBackgroundCopyCallback {
                lpVtbl: &callback::VTBL,
            },
            transferred: transferred_cb,
            error: error_cb,
            modification: modification_cb,
        });

        // TODO: don't just leak, proper ref counting
        unsafe {
            com_call!(
                self.0,
                IBackgroundCopyJob::SetNotifyInterface(Box::leak(callback)
                    as *mut callback::BackgroundCopyCallback
                    as *mut IUnknown)
            )
        }?;

        // TODO: this should probably return some object that owns the callback registration, which
        // handles clearing the notify interface on drop (and which maybe can check if the
        // callback is still registered)
        Ok(())
    }

    pub fn get_status(&self) -> Result<BitsJobStatus> {
        let mut state = 0;
        let mut progress = unsafe { mem::uninitialized() };
        let mut error_count = 0;

        unsafe {
            com_call!(self.0, IBackgroundCopyJob::GetState(&mut state))?;
            com_call!(self.0, IBackgroundCopyJob::GetProgress(&mut progress))?;
            com_call!(self.0, IBackgroundCopyJob::GetErrorCount(&mut error_count))?;
        }

        Ok(BitsJobStatus {
            state,
            progress: BitsJobProgress {
                total_bytes: if progress.BytesTotal == BG_SIZE_UNKNOWN {
                    None
                } else {
                    Some(progress.BytesTotal)
                },
                transferred_bytes: progress.BytesTransferred,
                total_files: progress.FilesTotal,
                transferred_files: progress.FilesTransferred,
            },
            error_count,
            error: if state == BG_JOB_STATE_ERROR || state == BG_JOB_STATE_TRANSIENT_ERROR {
                let error_obj =
                    unsafe { com_call_getter!(|e| self.0, IBackgroundCopyJob::GetError(e)) }?;

                Some(BitsJob::get_error(error_obj)?)
            } else {
                None
            },
        })
    }

    fn get_error(error_obj: ComPtr<IBackgroundCopyError>) -> Result<BitsJobError> {
        let mut context = 0;
        let mut hresult = 0;
        unsafe {
            com_call!(
                error_obj,
                IBackgroundCopyError::GetError(&mut context, &mut hresult)
            )
        }?;

        Ok(BitsJobError {
            context,
            error: hresult,
        })
    }
}
