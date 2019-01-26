use winapi::shared::winerror::HRESULT;
use winapi::um::bits::{BG_ERROR_CONTEXT, BG_JOB_STATE};

#[cfg(feature = "status_serde")]
use serde_derive::{Deserialize, Serialize};

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
pub struct BitsJobError {
    pub context: BG_ERROR_CONTEXT,
    pub error: HRESULT,
}

#[derive(Debug)]
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

#[derive(Debug)]
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
