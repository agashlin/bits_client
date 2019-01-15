use std::ffi::OsString;

use guid_win::Guid;
#[cfg(feature = "local_service_task")]
use {serde::de::DeserializeOwned, serde::Serialize, serde_derive::{Deserialize, Serialize}};

#[cfg(feature = "local_service_task")]
pub fn task_name() -> OsString {
    OsString::from("FOOOO")
}

type HRESULT = i32;

// TODO: real sizes checked against something reasonable
pub const MAX_COMMAND: usize = 0x4000;
pub const MAX_RESPONSE: usize = 0x4000;

pub const PROTOCOL_VERSION: u8 = 0;

// Any command
#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub enum Command {
    StartJob(StartJobCommand),
    MonitorJob(MonitorJobCommand),
    ResumeJob(ResumeJobCommand),
    SetJobPriority(SetJobPriorityCommand),
    SetUpdateInterval(SetUpdateIntervalCommand),
    CompleteJob(CompleteJobCommand),
    CancelJob(CancelJobCommand),
}

#[cfg(feature = "local_service_task")]
pub trait CommandType: DeserializeOwned + Serialize {
    type Success: DeserializeOwned + Serialize;
    type Failure: DeserializeOwned + Serialize;
    fn new(command: Self) -> Command;
}
#[cfg(not(feature = "local_service_task"))]
pub trait CommandType {
    type Success;
    type Failure;
    fn new(command: Self) -> Command;
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct MonitorConfig {
    pub pipe_name: OsString,
    pub interval_millis: u32,
}

// Start Job
#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct StartJobCommand {
    pub url: OsString,
    pub save_path: OsString,
    pub monitor: Option<MonitorConfig>,
}

#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct StartJobSuccess {
    pub guid: Guid,
}

#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub enum StartJobFailure {
    ArgumentValidation(String),
    Create(HRESULT),
    AddFile(HRESULT),
    ApplySettings(HRESULT),
    Resume(HRESULT),
    OtherBITS(HRESULT),
    Other(String),
}

impl CommandType for StartJobCommand {
    type Success = StartJobSuccess;
    type Failure = StartJobFailure;
    fn new(cmd: Self) -> Command {
        Command::StartJob(cmd)
    }
}

// Monitor Job
#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct MonitorJobCommand {
    pub guid: Guid,
    pub monitor: MonitorConfig,
}

#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub enum MonitorJobFailure {
    ArgumentValidation(String),
    NotFound,
    GetJob(HRESULT),
    OtherBITS(HRESULT),
    Other(String),
}

impl CommandType for MonitorJobCommand {
    type Success = ();
    type Failure = MonitorJobFailure;
    fn new(cmd: Self) -> Command {
        Command::MonitorJob(cmd)
    }
}

// Resume Job
#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct ResumeJobCommand {
    pub guid: Guid,
}

#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub enum ResumeJobFailure {
    NotFound,
    GetJob(HRESULT),
    ResumeJob(HRESULT),
    OtherBITS(HRESULT),
    Other(String),
}

impl CommandType for ResumeJobCommand {
    type Success = ();
    type Failure = ResumeJobFailure;
    fn new(cmd: Self) -> Command {
        Command::ResumeJob(cmd)
    }
}

// Set Job Priority
#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct SetJobPriorityCommand {
    pub guid: Guid,
    pub foreground: bool,
}

#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub enum SetJobPriorityFailure {
    NotFound,
    GetJob(HRESULT),
    ApplySettings(HRESULT),
    OtherBITS(HRESULT),
    Other(String),
}

impl CommandType for SetJobPriorityCommand {
    type Success = ();
    type Failure = SetJobPriorityFailure;
    fn new(cmd: Self) -> Command {
        Command::SetJobPriority(cmd)
    }
}

// Set Update Interval
#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct SetUpdateIntervalCommand {
    pub guid: Guid,
    pub interval_millis: u32,
}

#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub enum SetUpdateIntervalFailure {
    ArgumentValidation(String),
    NotFound,
    GetJob(HRESULT),
    ApplySettings(HRESULT),
    OtherBITS(HRESULT),
    Other(String),
}

impl CommandType for SetUpdateIntervalCommand {
    type Success = ();
    type Failure = SetUpdateIntervalFailure;
    fn new(cmd: Self) -> Command {
        Command::SetUpdateInterval(cmd)
    }
}

// Complete Job
#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct CompleteJobCommand {
    pub guid: Guid,
}

#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub enum CompleteJobFailure {
    NotFound,
    GetJob(HRESULT),
    CompleteJob(HRESULT),
    PartialComplete,
    OtherBITS(HRESULT),
    Other(String),
}

impl CommandType for CompleteJobCommand {
    type Success = ();
    type Failure = CompleteJobFailure;
    fn new(cmd: Self) -> Command {
        Command::CompleteJob(cmd)
    }
}

// Cancel Job
#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub struct CancelJobCommand {
    pub guid: Guid,
}

#[derive(Debug)]
#[cfg_attr(feature = "local_service_task", derive(Deserialize, Serialize))]
pub enum CancelJobFailure {
    NotFound,
    GetJob(HRESULT),
    CancelJob(HRESULT),
    OtherBITS(HRESULT),
    Other(String),
}

impl CommandType for CancelJobCommand {
    type Success = ();
    type Failure = CancelJobFailure;
    fn new(cmd: Self) -> Command {
        Command::CancelJob(cmd)
    }
}
