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

use std::ffi::{OsStr, OsString};
use std::mem;
use std::ptr;
use std::result;

use comedy::com::{create_instance_local_server, INIT_MTA};
use comedy::error::{Error, ErrorCode, FileLine, ResultExt};
use comedy::handle::CoTaskMem;
use comedy::{com_call, com_call_getter};
use guid_win::Guid;
use winapi::shared::ntdef::LPWSTR;
use winapi::um::bits::{
    IBackgroundCopyError, IBackgroundCopyJob, IBackgroundCopyManager, BG_JOB_PRIORITY,
    BG_JOB_PRIORITY_FOREGROUND, BG_JOB_PRIORITY_HIGH, BG_JOB_PRIORITY_LOW, BG_JOB_PRIORITY_NORMAL,
    BG_JOB_PROXY_USAGE, BG_JOB_PROXY_USAGE_AUTODETECT, BG_JOB_PROXY_USAGE_NO_PROXY,
    BG_JOB_PROXY_USAGE_PRECONFIG, BG_JOB_STATE_ERROR, BG_JOB_STATE_TRANSIENT_ERROR,
    BG_JOB_TYPE_DOWNLOAD, BG_NOTIFY_DISABLE, BG_NOTIFY_JOB_ERROR, BG_NOTIFY_JOB_MODIFICATION,
    BG_NOTIFY_JOB_TRANSFERRED, BG_SIZE_UNKNOWN,
};
use winapi::um::bitsmsg::BG_E_NOT_FOUND;
use winapi::um::unknwnbase::IUnknown;
use winapi::RIDL;
use wio::com::ComPtr;
use wio::wide::{FromWide, ToWide};

pub use status::{BitsJobError, BitsJobProgress, BitsJobStatus};

pub use winapi::shared::winerror::E_FAIL;

#[repr(u32)]
pub enum BitsJobPriority {
    Foreground = BG_JOB_PRIORITY_FOREGROUND,
    High = BG_JOB_PRIORITY_HIGH,
    Normal = BG_JOB_PRIORITY_NORMAL,
    Low = BG_JOB_PRIORITY_LOW,
}

#[repr(u32)]
pub enum BitsProxyUsage {
    NoProxy = BG_JOB_PROXY_USAGE_NO_PROXY,
    /// Default
    Preconfig = BG_JOB_PROXY_USAGE_PRECONFIG,
    AutoDetect = BG_JOB_PROXY_USAGE_AUTODETECT,
}

type Result<T> = result::Result<T, Error>;

// temporarily here until https://github.com/retep998/winapi-rs/pull/704 is available
RIDL! {#[uuid(0x4991d34b, 0x80a1, 0x4291, 0x83, 0xb6, 0x33, 0x28, 0x36, 0x6b, 0x90, 0x97)]
class BcmClass;}

pub struct BackgroundCopyManager(ComPtr<IBackgroundCopyManager>);

impl BackgroundCopyManager {
    pub fn connect() -> Result<BackgroundCopyManager> {
        // Methods do not have to check once we have successfully initialized COM once for the
        // thread, as BackgroundCopyManager can only be used on one thread.
        INIT_MTA.with(|com| {
            if let Err(e) = com {
                return Err(e.clone());
            }
            Ok(())
        })?;

        Ok(BackgroundCopyManager(create_instance_local_server::<
            BcmClass,
            IBackgroundCopyManager,
        >()?))
    }

    pub fn create_job(&self, display_name: &OsStr) -> Result<BitsJob> {
        unsafe {
            let mut guid = mem::zeroed();
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

    /// Returns Err if the job was not found.
    pub fn get_job_by_guid(&self, guid: &Guid) -> Result<BitsJob> {
        unsafe { com_call_getter!(|job| self.0, IBackgroundCopyManager::GetJob(&guid.0, job)) }
            .map(|job| BitsJob(job))
    }

    /// Returns Ok(None) if the job was not found but there was no other error.
    pub fn find_job_by_guid(&self, guid: &Guid) -> Result<Option<BitsJob>> {
        Ok(self
            .get_job_by_guid(guid)
            .map(|job| Some(job))
            .allow_hresult(BG_E_NOT_FOUND as i32, None)?)
    }

    /// Returns Ok(None) if the job was not found, or if it had the wrong name.
    pub fn find_job_by_guid_and_name(
        &self,
        guid: &Guid,
        match_name: &OsStr,
    ) -> Result<Option<BitsJob>> {
        if let Some(BitsJob(job)) = self.find_job_by_guid(guid)? {
            let job_name = unsafe {
                let mut job_name = ptr::null_mut() as LPWSTR;

                com_call!(job, IBackgroundCopyJob::GetDisplayName(&mut job_name))?;

                let _job_name_handle = CoTaskMem::wrap(job_name as *mut _).map_err(|()| Error {
                    code: Some(ErrorCode::NullPtr),
                    function: Some("IBackgroundCopyJob::GetDisplayName"),
                    file_line: Some(FileLine(file!(), line!())),
                })?;

                OsString::from_wide_ptr_null(job_name)
            };

            if job_name == match_name {
                Ok(Some(BitsJob(job)))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

pub struct BitsJob(ComPtr<IBackgroundCopyJob>);

impl BitsJob {
    pub fn guid(&self) -> Result<Guid> {
        // TODO: cache on create or retrieved by GUID?
        unsafe {
            let mut guid = mem::zeroed();
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

    pub fn set_proxy_usage(&mut self, usage: BitsProxyUsage) -> Result<()> {
        use BitsProxyUsage::*;

        match usage {
            Preconfig | NoProxy | AutoDetect => {
                unsafe {
                    com_call!(
                        self.0,
                        IBackgroundCopyJob::SetProxySettings(
                            usage as BG_JOB_PROXY_USAGE,
                            ptr::null_mut(),
                            ptr::null_mut(),
                        )
                    )
                }?;
                Ok(())
            }
        }
    }

    pub fn set_priority(&mut self, priority: BitsJobPriority) -> Result<()> {
        unsafe {
            com_call!(
                self.0,
                IBackgroundCopyJob::SetPriority(priority as BG_JOB_PRIORITY)
            )
        }?;
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
    ) -> Result<()> {
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

        callback::BackgroundCopyCallback::register(
            self,
            transferred_cb,
            error_cb,
            modification_cb,
        )?;

        unsafe { com_call!(self.0, IBackgroundCopyJob::SetNotifyFlags(flags)) }?;

        Ok(())
    }

    fn _clear_callbacks(&mut self) -> Result<()> {
        unsafe {
            com_call!(
                self.0,
                IBackgroundCopyJob::SetNotifyFlags(BG_NOTIFY_DISABLE)
            )?;

            self.set_notify_interface(ptr::null_mut() as *mut IUnknown)
        }
    }

    pub fn get_status(&self) -> Result<BitsJobStatus> {
        let mut state = 0;
        let mut progress = unsafe { mem::zeroed() };
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

    unsafe fn set_notify_interface(&self, interface: *mut IUnknown) -> Result<()> {
        com_call!(self.0, IBackgroundCopyJob::SetNotifyInterface(interface))?;
        Ok(())
    }
}
