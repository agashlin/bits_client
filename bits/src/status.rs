// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your option.
// All files in the project carrying such notice may not be copied, modified, or distributed
// except according to those terms.

//! Data types for reporting a job's status

use std::ffi::OsString;
use std::mem;

use comedy::{com_call, com_call_getter, com_call_taskmem_getter, ResultExt};
use comedy::com::ComRef;
use filetime_win::FileTime;
use winapi::shared::ntdef::{HRESULT, LANGIDFROMLCID, LPWSTR};
use winapi::shared::minwindef::DWORD;
use winapi::um::bits::{BG_ERROR_CONTEXT, BG_JOB_STATE, BG_SIZE_UNKNOWN, IBackgroundCopyJob, IBackgroundCopyError};
use winapi::um::bitsmsg::BG_E_ERROR_INFORMATION_UNAVAILABLE;
use winapi::um::winnls::GetThreadLocale;

use ::Result;
use wide::FromWidePtrNull;

#[cfg(feature = "status_serde")]
use serde_derive::{Deserialize, Serialize};

#[derive(Clone, Debug)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobStatus {
    pub state: BitsJobState,
    pub progress: BitsJobProgress,
    pub error_count: u32,
    pub error: Option<BitsJobError>,
    pub times: BitsJobTimes,
}

impl BitsJobStatus {
    pub fn for_job(job: &ComRef<IBackgroundCopyJob>) -> Result<BitsJobStatus> {
        let error = unsafe { com_call_getter!(|e| job, IBackgroundCopyJob::GetError(e)) }
            .map(Some)
            .allow_err(BG_E_ERROR_INFORMATION_UNAVAILABLE as HRESULT, None)?;

        BitsJobStatus::for_job_with_error(job, error.as_ref())
    }

    pub fn for_job_with_error(
        job: &ComRef<IBackgroundCopyJob>,
        error_obj: Option<&ComRef<IBackgroundCopyError>>,
    ) -> Result<BitsJobStatus>
    {
        let mut state = 0;
        let mut progress = unsafe { mem::zeroed() };
        let mut error_count = 0;
        let mut times = unsafe { mem::zeroed() };

        unsafe {
            com_call!(job, IBackgroundCopyJob::GetState(&mut state))?;
            com_call!(job, IBackgroundCopyJob::GetProgress(&mut progress))?;
            com_call!(job, IBackgroundCopyJob::GetErrorCount(&mut error_count))?;
            com_call!(job, IBackgroundCopyJob::GetTimes(&mut times))?;
        }

        Ok(BitsJobStatus {
            state: BitsJobState::from(state),
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
            error: if let Some(error_obj) = error_obj {
                Some(BitsJobError::for_error(error_obj)?)
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
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobError {
    pub context: BitsErrorContext,
    pub context_str: String,
    pub error: HRESULT,
    pub error_str: String,
}

impl BitsJobError {
    pub fn for_error(error_obj: &ComRef<IBackgroundCopyError>) -> Result<BitsJobError> {
        let mut context = 0;
        let mut hresult = 0;
        unsafe {
            com_call!(
                error_obj,
                IBackgroundCopyError::GetError(&mut context, &mut hresult)
            )?;

            let language_id = DWORD::from(LANGIDFROMLCID(GetThreadLocale()));

            Ok(BitsJobError {
                context: BitsErrorContext::from(context),
                context_str: OsString::from_wide_ptr_null(*com_call_taskmem_getter!(
                    |desc| error_obj,
                    IBackgroundCopyError::GetErrorContextDescription(language_id, desc)
                )? as LPWSTR)
                .to_string_lossy()
                .into_owned(),
                error: hresult,
                error_str: OsString::from_wide_ptr_null(*com_call_taskmem_getter!(
                    |desc| error_obj,
                    IBackgroundCopyError::GetErrorDescription(language_id, desc)
                )? as LPWSTR)
                .to_string_lossy()
                .into_owned(),
            })
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub enum BitsErrorContext {
    None,
    Unknown,
    GeneralQueueManager,
    QueueManagerNotification,
    LocalFile,
    RemoteFile,
    GeneralTransport,
    RemoteApplication,
    /// No other values are documented
    Other(BG_ERROR_CONTEXT),
}

impl From<BG_ERROR_CONTEXT> for BitsErrorContext {
    fn from(ec: BG_ERROR_CONTEXT) -> BitsErrorContext {
        use self::BitsErrorContext::*;
        use winapi::um::bits;
        match ec {
            bits::BG_ERROR_CONTEXT_NONE => None,
            bits::BG_ERROR_CONTEXT_UNKNOWN => Unknown,
            bits::BG_ERROR_CONTEXT_GENERAL_QUEUE_MANAGER => GeneralQueueManager,
            bits::BG_ERROR_CONTEXT_QUEUE_MANAGER_NOTIFICATION => QueueManagerNotification,
            bits::BG_ERROR_CONTEXT_LOCAL_FILE => LocalFile,
            bits::BG_ERROR_CONTEXT_REMOTE_FILE => RemoteFile,
            bits::BG_ERROR_CONTEXT_GENERAL_TRANSPORT => GeneralTransport,
            bits::BG_ERROR_CONTEXT_REMOTE_APPLICATION => RemoteApplication,
            ec => Other(ec),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub enum BitsJobState {
    Queued,
    Connecting,
    Transferring,
    Suspended,
    Error,
    TransientError,
    Transferred,
    Acknowledged,
    Cancelled,
    /// No other values are documented
    Other(BG_JOB_STATE),
}

impl From<BG_JOB_STATE> for BitsJobState {
    fn from(s: BG_JOB_STATE) -> BitsJobState {
        use self::BitsJobState::*;
        use winapi::um::bits;
        match s {
            bits::BG_JOB_STATE_QUEUED => Queued,
            bits::BG_JOB_STATE_CONNECTING => Connecting,
            bits::BG_JOB_STATE_TRANSFERRING => Transferring,
            bits::BG_JOB_STATE_SUSPENDED => Suspended,
            bits::BG_JOB_STATE_ERROR => Error,
            bits::BG_JOB_STATE_TRANSIENT_ERROR => TransientError,
            bits::BG_JOB_STATE_TRANSFERRED => Transferred,
            bits::BG_JOB_STATE_ACKNOWLEDGED => Acknowledged,
            bits::BG_JOB_STATE_CANCELLED => Cancelled,
            s => Other(s),
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobProgress {
    pub total_bytes: Option<u64>,
    pub transferred_bytes: u64,
    pub total_files: u32,
    pub transferred_files: u32,
}

#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobTimes {
    pub creation: FileTime,
    pub modification: FileTime,
    pub transfer_completion: Option<FileTime>,
}
