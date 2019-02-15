/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::ffi::OsString;

use guid_win::Guid;

use super::{BitsErrorContext, BitsJobProgress, BitsJobState, BitsJobTimes};

type HRESULT = i32;

#[derive(Clone, Debug)]
pub struct HResultMessage {
    pub hr: HRESULT,
    pub message: String,
}

// TODO: real sizes checked against something reasonable
pub const MAX_COMMAND: usize = 0x4000;
pub const MAX_RESPONSE: usize = 0x4000;

pub const PROTOCOL_VERSION: u8 = 0;

// Any command
#[derive(Clone, Debug)]
pub enum Command {
    StartJob(StartJobCommand),
    MonitorJob(MonitorJobCommand),
    SuspendJob(SuspendJobCommand),
    ResumeJob(ResumeJobCommand),
    SetJobPriority(SetJobPriorityCommand),
    SetUpdateInterval(SetUpdateIntervalCommand),
    CompleteJob(CompleteJobCommand),
    CancelJob(CancelJobCommand),
}

pub trait CommandType {
    type Success;
    type Failure;
    fn new(command: Self) -> Command;
}

#[derive(Clone, Debug)]
pub struct MonitorConfig {
    pub pipe_name: OsString,
    pub interval_millis: u32,
}

#[derive(Clone, Debug)]
pub enum ProxyUsage {
    Preconfig,
    NoProxy,
    AutoDetect,
}

// Start Job
#[derive(Clone, Debug)]
pub struct StartJobCommand {
    pub url: OsString,
    pub save_path: OsString,
    pub proxy_usage: ProxyUsage,
    pub monitor: Option<MonitorConfig>,
}

#[derive(Clone, Debug)]
pub struct StartJobSuccess {
    pub guid: Guid,
}

#[derive(Clone, Debug)]
pub enum StartJobFailure {
    ArgumentValidation(String),
    Create(HResultMessage),
    AddFile(HResultMessage),
    ApplySettings(HResultMessage),
    Resume(HResultMessage),
    OtherBITS(HResultMessage),
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
#[derive(Clone, Debug)]
pub struct MonitorJobCommand {
    pub guid: Guid,
    pub monitor: MonitorConfig,
}

#[derive(Clone, Debug)]
pub enum MonitorJobFailure {
    ArgumentValidation(String),
    NotFound,
    GetJob(HResultMessage),
    OtherBITS(HResultMessage),
    Other(String),
}

impl CommandType for MonitorJobCommand {
    type Success = ();
    type Failure = MonitorJobFailure;
    fn new(cmd: Self) -> Command {
        Command::MonitorJob(cmd)
    }
}

// Suspend Job
#[derive(Clone, Debug)]
pub struct SuspendJobCommand {
    pub guid: Guid,
}

#[derive(Clone, Debug)]
pub enum SuspendJobFailure {
    NotFound,
    GetJob(HResultMessage),
    SuspendJob(HResultMessage),
    OtherBITS(HResultMessage),
    Other(String),
}

impl CommandType for SuspendJobCommand {
    type Success = ();
    type Failure = SuspendJobFailure;
    fn new(cmd: Self) -> Command {
        Command::SuspendJob(cmd)
    }
}

// Resume Job
#[derive(Clone, Debug)]
pub struct ResumeJobCommand {
    pub guid: Guid,
}

#[derive(Clone, Debug)]
pub enum ResumeJobFailure {
    NotFound,
    GetJob(HResultMessage),
    ResumeJob(HResultMessage),
    OtherBITS(HResultMessage),
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
#[derive(Clone, Debug)]
pub struct SetJobPriorityCommand {
    pub guid: Guid,
    pub foreground: bool,
}

#[derive(Clone, Debug)]
pub enum SetJobPriorityFailure {
    NotFound,
    GetJob(HResultMessage),
    ApplySettings(HResultMessage),
    OtherBITS(HResultMessage),
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
#[derive(Clone, Debug)]
pub struct SetUpdateIntervalCommand {
    pub guid: Guid,
    pub interval_millis: u32,
}

#[derive(Clone, Debug)]
pub enum SetUpdateIntervalFailure {
    ArgumentValidation(String),
    NotFound,
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
#[derive(Clone, Debug)]
pub struct CompleteJobCommand {
    pub guid: Guid,
}

#[derive(Clone, Debug)]
pub enum CompleteJobFailure {
    NotFound,
    GetJob(HResultMessage),
    CompleteJob(HResultMessage),
    PartialComplete,
    OtherBITS(HResultMessage),
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
#[derive(Clone, Debug)]
pub struct CancelJobCommand {
    pub guid: Guid,
}

#[derive(Clone, Debug)]
pub enum CancelJobFailure {
    NotFound,
    GetJob(HResultMessage),
    CancelJob(HResultMessage),
    OtherBITS(HResultMessage),
    Other(String),
}

impl CommandType for CancelJobCommand {
    type Success = ();
    type Failure = CancelJobFailure;
    fn new(cmd: Self) -> Command {
        Command::CancelJob(cmd)
    }
}

// Status report
// This has more enums than bits::status::BitsJobStatus, and includes a URL which updates with
// redirect.
#[derive(Clone, Debug)]
pub struct JobStatus {
    pub state: BitsJobState,
    pub progress: BitsJobProgress,
    pub error_count: u32,
    pub error: Option<JobError>,
    pub times: BitsJobTimes,
    /// None means same as last time
    pub url: Option<OsString>,
}

#[derive(Clone, Debug)]
pub struct JobError {
    pub context: BitsErrorContext,
    pub context_str: String,
    pub error: HResultMessage,
}
