use winapi::shared::winerror::HRESULT;
use winapi::um::bits::{BG_ERROR_CONTEXT, BG_JOB_STATE};

#[cfg(feature = "status_serde")]
use serde_derive::{Deserialize, Serialize};

#[derive(Debug)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobError {
    pub context: BG_ERROR_CONTEXT,
    pub error: HRESULT,
}

#[derive(Debug)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobStatus {
    pub state: BG_JOB_STATE,
    pub progress: BitsJobProgress,
    pub error_count: u32,
    pub error: Option<BitsJobError>,
}

#[derive(Debug)]
#[cfg_attr(feature = "status_serde", derive(Serialize, Deserialize))]
pub struct BitsJobProgress {
    pub total_bytes: Option<u64>,
    pub transferred_bytes: u64,
    pub total_files: u32,
    pub transferred_files: u32,
}

/*
impl fmt::Debug for BitsJobStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BitsJobStatus {{ ")?;
        write!(f, "state: {:?}, ", self.state)?;
        write!(f, "progress: {:?} ", self.progress)?;
        write!(f, "error_count: {:?}, ", self.error_count)?;
        write!(f, "error: {:?} }}", self.error)
    }
}
*/
