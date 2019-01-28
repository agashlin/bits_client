extern crate bits;
extern crate comedy;
extern crate failure;
extern crate failure_derive;
extern crate guid_win;

pub mod bits_protocol;

mod in_process;

use std::convert;
use std::ffi;

use guid_win::Guid;

use bits_protocol::*;
use failure::Fail;

pub use bits::status::{BitsErrorContext, BitsJobState};
pub use bits::{BitsJobError, BitsJobProgress, BitsJobStatus, BitsProxyUsage};

// These errors would come from a Local Service client, this structure properly lives in the
// crate that deals with named pipes.
#[derive(Clone, Debug, Eq, Fail, PartialEq)]
pub enum PipeError {
    #[fail(display = "Pipe is not connected")]
    NotConnected,
    #[fail(display = "Operation timed out")]
    Timeout,
    #[fail(display = "Should have written {} bytes, wrote {}", _0, _1)]
    WriteCount(usize, u32),
    #[fail(display = "Windows API error")]
    Api(#[fail(cause)] comedy::Error),
}

impl convert::From<comedy::Error> for PipeError {
    fn from(err: comedy::Error) -> PipeError {
        PipeError::Api(err)
    }
}

pub use PipeError as Error;

pub enum BitsClient {
    /// The InProcess variant does all BITS calls in-process.
    InProcess(in_process::InProcessClient),
    // Space is reserved here for the LocalService variant, which will work through an external
    // process running as Local Service.
}

use BitsClient::*;

impl BitsClient {
    /// Create an in-process BitsClient.
    pub fn new(
        job_name: ffi::OsString,
        save_path_prefix: ffi::OsString,
    ) -> Result<BitsClient, Error> {
        Ok(InProcess(in_process::InProcessClient::new(
            job_name,
            save_path_prefix,
        )?))
    }

    pub fn start_job(
        &mut self,
        url: ffi::OsString,
        save_path: ffi::OsString,
        proxy_usage: BitsProxyUsage,
        monitor_interval_millis: u32,
    ) -> Result<Result<(StartJobSuccess, BitsMonitorClient), StartJobFailure>, Error> {
        match self {
            InProcess(client) => Ok(client
                .start_job(url, save_path, proxy_usage, monitor_interval_millis)
                .map(|(success, monitor)| (success, BitsMonitorClient::InProcess(monitor)))),
        }
    }

    pub fn monitor_job(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<Result<BitsMonitorClient, MonitorJobFailure>, Error> {
        match self {
            InProcess(client) => Ok(client
                .monitor_job(guid, interval_millis)
                .map(|monitor| BitsMonitorClient::InProcess(monitor))),
        }
    }

    pub fn resume_job(&mut self, guid: Guid) -> Result<Result<(), ResumeJobFailure>, Error> {
        match self {
            InProcess(client) => Ok(client.resume_job(guid)),
        }
    }

    pub fn set_job_priority(
        &mut self,
        guid: Guid,
        foreground: bool,
    ) -> Result<Result<(), SetJobPriorityFailure>, Error> {
        match self {
            InProcess(client) => Ok(client.set_job_priority(guid, foreground)),
        }
    }

    pub fn set_update_interval(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<Result<(), SetUpdateIntervalFailure>, Error> {
        match self {
            InProcess(client) => Ok(client.set_update_interval(guid, interval_millis)),
        }
    }

    pub fn stop_update(
        &mut self,
        guid: Guid,
    ) -> Result<Result<(), SetUpdateIntervalFailure>, Error> {
        match self {
            InProcess(client) => Ok(client.stop_update(guid)),
        }
    }

    pub fn complete_job(&mut self, guid: Guid) -> Result<Result<(), CompleteJobFailure>, Error> {
        match self {
            InProcess(client) => Ok(client.complete_job(guid)),
        }
    }

    pub fn cancel_job(&mut self, guid: Guid) -> Result<Result<(), CancelJobFailure>, Error> {
        match self {
            InProcess(client) => Ok(client.cancel_job(guid)),
        }
    }
}

pub enum BitsMonitorClient {
    InProcess(in_process::InProcessMonitor),
}

impl BitsMonitorClient {
    pub fn get_status(&mut self, timeout_millis: u32) -> Result<BitsJobStatus, Error> {
        match self {
            BitsMonitorClient::InProcess(client) => client.get_status(timeout_millis),
        }
    }
}
