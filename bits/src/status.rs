use std::fmt;

use winapi::shared::minwindef::ULONG;
use winapi::shared::winerror::HRESULT;
use winapi::um::bits::{BG_ERROR_CONTEXT, BG_JOB_PROGRESS, BG_JOB_STATE};

#[cfg(feature = "status_serde")]
use serde_derive::{Deserialize, Serialize};
#[cfg(feature = "status_serde")]
use winapi::shared::basetsd::UINT64;

#[cfg(feature = "status_serde")]
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize)]
#[serde(remote = "BG_JOB_PROGRESS")]
#[repr(C)]
pub struct BG_JOB_PROGRESS_Serde {
    pub BytesTotal: UINT64,
    pub BytesTransferred: UINT64,
    pub FilesTotal: ULONG,
    pub FilesTransferred: ULONG,
}

#[derive(Debug)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobError {
    pub context: BG_ERROR_CONTEXT,
    pub error: HRESULT,
}

#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobStatus {
    pub state: BG_JOB_STATE,
    #[cfg_attr(feature = "status_serde", serde(with = "BG_JOB_PROGRESS_Serde"))]
    pub progress: BG_JOB_PROGRESS,
    pub error_count: ULONG,
    pub error: Option<BitsJobError>,
}

impl fmt::Debug for BitsJobStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BitsJobStatus {{ ")?;
        write!(f, "state: {:?}, ", self.state)?;
        write!(f, "progress: BG_JOB_PROGRESS {{ ")?;
        write!(f, "BytesTotal: {:?}, ", self.progress.BytesTotal)?;
        write!(
            f,
            "BytesTransferred: {:?}, ",
            self.progress.BytesTransferred
        )?;
        write!(f, "FilesTotal: {:?}, ", self.progress.FilesTotal)?;
        write!(
            f,
            "FilesTransferred: {:?} }}, ",
            self.progress.FilesTransferred
        )?;
        write!(f, "error_count: {:?}, ", self.error_count)?;
        write!(f, "error: {:?} }}", self.error)
    }
}
