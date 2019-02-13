extern crate comedy;
extern crate filetime_win;
extern crate guid_win;
extern crate winapi;

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

use comedy::com::{create_instance_local_server, ComPtr, INIT_MTA};
use comedy::error::{Error, ResultExt};
use comedy::wide::{FromWide, ToWide};
use comedy::{com_call, com_call_cotaskmem_getter, com_call_getter};
use filetime_win::FileTime;
use guid_win::Guid;
use winapi::shared::minwindef::DWORD;
use winapi::shared::ntdef::{LANGIDFROMLCID, LPWSTR, ULONG};
use winapi::um::bits::{
    IBackgroundCopyError, IBackgroundCopyFile, IBackgroundCopyJob, IBackgroundCopyManager,
    IEnumBackgroundCopyFiles, IEnumBackgroundCopyJobs, BG_JOB_PRIORITY, BG_JOB_PRIORITY_FOREGROUND,
    BG_JOB_PRIORITY_HIGH, BG_JOB_PRIORITY_LOW, BG_JOB_PRIORITY_NORMAL, BG_JOB_PROXY_USAGE,
    BG_JOB_PROXY_USAGE_AUTODETECT, BG_JOB_PROXY_USAGE_NO_PROXY, BG_JOB_PROXY_USAGE_PRECONFIG,
    BG_JOB_STATE_ERROR, BG_JOB_STATE_TRANSIENT_ERROR, BG_JOB_TYPE_DOWNLOAD, BG_NOTIFY_DISABLE,
    BG_NOTIFY_JOB_ERROR, BG_NOTIFY_JOB_MODIFICATION, BG_NOTIFY_JOB_TRANSFERRED, BG_SIZE_UNKNOWN,
};
use winapi::um::bits2_5::BG_HTTP_REDIRECT_POLICY_ALLOW_REPORT;
use winapi::um::bitsmsg::BG_E_NOT_FOUND;
use winapi::um::unknwnbase::IUnknown;
use winapi::um::winnls::GetThreadLocale;
use winapi::RIDL;

pub use winapi::um::bits::{BG_ERROR_CONTEXT, BG_JOB_STATE};
pub use winapi::um::bitsmsg::{BG_S_PARTIAL_COMPLETE, BG_S_UNABLE_TO_DELETE_FILES};

pub use status::{
    BitsErrorContext, BitsJobError, BitsJobProgress, BitsJobState, BitsJobStatus, BitsJobTimes,
};

pub use winapi::shared::winerror::E_FAIL;

#[repr(u32)]
#[derive(Copy, Clone, Debug)]
pub enum BitsJobPriority {
    Foreground = BG_JOB_PRIORITY_FOREGROUND,
    High = BG_JOB_PRIORITY_HIGH,
    Normal = BG_JOB_PRIORITY_NORMAL,
    Low = BG_JOB_PRIORITY_LOW,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug)]
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

// temporarily here until https://github.com/retep998/winapi-rs/pull/737 is available
use winapi::shared::ntdef::{HRESULT, LPCWSTR};
use winapi::shared::rpcndr::byte;
use winapi::um::bits2_5::BG_CERT_STORE_LOCATION;
use winapi::um::unknwnbase::IUnknownVtbl;
RIDL! {#[uuid(0xf1bd1079, 0x9f01, 0x4bdc, 0x80, 0x36, 0xf0, 0x9b, 0x70, 0x09, 0x50, 0x66)]
interface IBackgroundCopyJobHttpOptions(IBackgroundCopyJobHttpOptionsVtbl):
    IUnknown(IUnknownVtbl) {
    fn SetClientCertificateByID(
        StoreLocation: BG_CERT_STORE_LOCATION,
        StoreName: LPCWSTR,
        pCertHashBlob: *mut byte,
    ) -> HRESULT,
    fn SetClientCertificateByName(
        StoreLocation: BG_CERT_STORE_LOCATION,
        StoreName: LPCWSTR,
        SubjectName: LPCWSTR,
    ) -> HRESULT,
    fn RemoveClientCertificate() -> HRESULT,
    fn GetClientCertificate(
        pStoreLocation: *mut BG_CERT_STORE_LOCATION,
        pStoreName: *mut LPWSTR,
        ppCertHashBlob: *mut *mut byte,
        pSubjectName: *mut LPWSTR,
    ) -> HRESULT,
    fn SetCustomHeaders(
        RequestHeaders: LPCWSTR,
    ) -> HRESULT,
    fn GetCustomHeaders(
        pRequestHeaders: *mut LPWSTR,
    ) -> HRESULT,
    fn SetSecurityFlags(
        Flags: ULONG,
    ) -> HRESULT,
    fn GetSecurityFlags(
        pFlags: *mut ULONG,
    ) -> HRESULT,
}}

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

    pub fn cancel_jobs_by_name(&self, match_name: &OsStr) -> Result<()> {
        unsafe {
            let jobs = com_call_getter!(|jobs| self.0, IBackgroundCopyManager::EnumJobs(0, jobs))?;

            loop {
                match com_call_getter!(
                    |job| jobs,
                    IEnumBackgroundCopyJobs::Next(1, job, ptr::null_mut())
                ) {
                    Ok(job) => {
                        if job_name_eq(&job, match_name)? {
                            let _ = com_call!(job, IBackgroundCopyJob::Cancel());
                        }
                    }
                    Err(_) => {
                        break Ok(());
                    }
                }
            }
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
            if job_name_eq(&job, match_name)? {
                Ok(Some(BitsJob(job)))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    pub fn get_error_description(&self, hr: HRESULT) -> Result<String> {
        unsafe {
            let language_id = LANGIDFROMLCID(GetThreadLocale()) as DWORD;

            Ok(OsString::from_wide_ptr_null(*com_call_cotaskmem_getter!(
                |desc| self.0,
                IBackgroundCopyManager::GetErrorDescription(hr, language_id, desc)
            )? as LPWSTR)
            .to_string_lossy()
            .into_owned())
        }
    }
}

fn job_name_eq(job: &ComPtr<IBackgroundCopyJob>, match_name: &OsStr) -> Result<bool> {
    let job_name = unsafe {
        OsString::from_wide_ptr_null(*com_call_cotaskmem_getter!(
            |name| job,
            IBackgroundCopyJob::GetDisplayName(name)
        )? as LPWSTR)
    };

    Ok(job_name == match_name)
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

    pub fn get_first_file(&mut self) -> Result<BitsFile> {
        unsafe {
            let files = com_call_getter!(|e| self.0, IBackgroundCopyJob::EnumFiles(e))?;
            let file = com_call_getter!(
                |f| files,
                IEnumBackgroundCopyFiles::Next(1, f, ptr::null_mut())
            )?;
            Ok(BitsFile(file))
        }
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

    pub fn set_minimum_retry_delay(&mut self, seconds: ULONG) -> Result<()> {
        unsafe { com_call!(self.0, IBackgroundCopyJob::SetMinimumRetryDelay(seconds)) }?;
        Ok(())
    }

    /// First available in Windows Vista
    pub fn set_redirect_report(&mut self) -> Result<()> {
        unsafe {
            com_call!(
                comedy::com::cast(&self.0)?,
                IBackgroundCopyJobHttpOptions::SetSecurityFlags(
                    BG_HTTP_REDIRECT_POLICY_ALLOW_REPORT
                )
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

    /// Has two interesting success `HRESULT`s: `BG_S_PARTIAL_COMPLETE` and
    /// `BG_S_UNABLE_TO_DELETE_FILES`.
    pub fn complete(&mut self) -> Result<HRESULT> {
        unsafe { com_call!(self.0, IBackgroundCopyJob::Complete()) }
    }

    /// Has an interesting success `HRESULT`: `BG_S_UNABLE_TO_DELETE_FILES`.
    pub fn cancel(&mut self) -> Result<HRESULT> {
        unsafe { com_call!(self.0, IBackgroundCopyJob::Cancel()) }
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
        let mut times = unsafe { mem::zeroed() };

        unsafe {
            com_call!(self.0, IBackgroundCopyJob::GetState(&mut state))?;
            com_call!(self.0, IBackgroundCopyJob::GetProgress(&mut progress))?;
            com_call!(self.0, IBackgroundCopyJob::GetErrorCount(&mut error_count))?;
            com_call!(self.0, IBackgroundCopyJob::GetTimes(&mut times))?;
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
            times: BitsJobTimes {
                creation: FileTime(times.CreationTime),
                modification: FileTime(times.ModificationTime),
                transfer_completion: if times.TransferCompletionTime.dwLowDateTime == 0
                    && times.TransferCompletionTime.dwHighDateTime == 0
                {
                    None
                } else {
                    Some(FileTime(times.TransferCompletionTime))
                },
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
            )?;

            let language_id = LANGIDFROMLCID(GetThreadLocale()) as DWORD;

            Ok(BitsJobError {
                context,
                context_str: OsString::from_wide_ptr_null(*com_call_cotaskmem_getter!(
                    |desc| error_obj,
                    IBackgroundCopyError::GetErrorContextDescription(language_id, desc)
                )? as LPWSTR)
                .to_string_lossy()
                .into_owned(),
                error: hresult,
                error_str: OsString::from_wide_ptr_null(*com_call_cotaskmem_getter!(
                    |desc| error_obj,
                    IBackgroundCopyError::GetErrorDescription(language_id, desc)
                )? as LPWSTR)
                .to_string_lossy()
                .into_owned(),
            })
        }
    }

    unsafe fn set_notify_interface(&self, interface: *mut IUnknown) -> Result<()> {
        com_call!(self.0, IBackgroundCopyJob::SetNotifyInterface(interface))?;
        Ok(())
    }
}

pub struct BitsFile(ComPtr<IBackgroundCopyFile>);

impl BitsFile {
    pub fn get_remote_name(&self) -> Result<String> {
        unsafe {
            Ok(OsString::from_wide_ptr_null(*com_call_cotaskmem_getter!(
                |name| self.0,
                IBackgroundCopyFile::GetRemoteName(name)
            )? as LPWSTR)
            .to_string_lossy()
            .into_owned())
        }
    }
}
