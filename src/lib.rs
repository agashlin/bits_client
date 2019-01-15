extern crate bits;
extern crate comedy;
extern crate failure;
extern crate guid_win;

pub mod bits_protocol;

use std::ffi;
use std::thread;
use std::time::Duration;

use bits::status::BitsJobStatus;
use bits::{BackgroundCopyManager, BitsJob, BG_JOB_PRIORITY_FOREGROUND, BG_JOB_PRIORITY_NORMAL};
use comedy::com::InitCom;
use failure::Error;
use guid_win::Guid;

use bits_protocol::*;

// The IPC is structured so that the client runs as a named pipe server, accepting connections
// from the BITS task server once it starts up, which it then uses to issue commands.
// This is done so that the client can create the pipe and wait for a connection to know when the
// task is ready for commands; otherwise it would have to repeatedly try to connect until the
// server creates the pipe.

pub enum BitsClient {
    Internal {
        com: InitCom
    }
}

use BitsClient::*;

impl BitsClient {
    pub fn new(com: InitCom) -> BitsClient {
        BitsClient::Internal{ com }
    }

    pub fn start_job(
        &mut self,
        url: ffi::OsString,
        save_path: ffi::OsString,
        monitor_interval_millis: u32,
    ) -> Result<Result<(StartJobSuccess, BitsMonitorClient), StartJobFailure>, Error> {
        match self {
            Internal{..} => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                // TODO determine name
                let mut job = bcm.create_job(&ffi::OsString::from("JOBBO"))?;
                job.add_file(&url, &save_path)?;
                job.resume()?;

                // TODO setup monitor callbacks
                Ok((
                    StartJobSuccess { guid: job.guid()? },
                    BitsMonitorClient::Internal {
                        job,
                        interval_millis: monitor_interval_millis
                    },
                ))
            })()
            .map_err(|e: Error| StartJobFailure::Other(e.to_string()))),
        }
    }

    pub fn monitor_job(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<Result<BitsMonitorClient, MonitorJobFailure>, Error> {
        match self {
            Internal{..} => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                let job = bcm.find_job_by_guid(&guid)?.unwrap();

                Ok(BitsMonitorClient::Internal{ job, interval_millis })
            })()
            .map_err(|e: Error| MonitorJobFailure::Other(e.to_string()))),
        }
    }

    pub fn resume_job(&mut self, guid: Guid) -> Result<Result<(), ResumeJobFailure>, Error> {
        match self {
            Internal{..} => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                let mut job = bcm.find_job_by_guid(&guid)?.unwrap();
                job.resume()?;
                Ok(())
            })()
            .map_err(|e: Error| ResumeJobFailure::Other(e.to_string()))),
        }
    }

    pub fn set_job_priorty(
        &mut self,
        guid: Guid,
        foreground: bool,
    ) -> Result<Result<(), SetJobPriorityFailure>, Error> {
        match self {
            Internal{..} => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                let mut job = bcm.find_job_by_guid(&guid)?.unwrap();
                job.set_priority(if foreground {
                    BG_JOB_PRIORITY_FOREGROUND
                } else {
                    BG_JOB_PRIORITY_NORMAL
                })?;
                Ok(())
            })()
            .map_err(|e: Error| SetJobPriorityFailure::Other(e.to_string()))),
        }
    }

    pub fn set_update_interval(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<Result<(), SetUpdateIntervalFailure>, Error> {
        match self {
            Internal{..} => {
                // TODO: set up a registry of monitors within the client, and have the monitor
                // listen to an mpsc that can consume everything
                let _guid = guid;
                let _interval_millis = interval_millis;
                unimplemented!()
            }
        }
    }

    pub fn complete_job(&mut self, guid: Guid) -> Result<Result<(), CompleteJobFailure>, Error> {
        match self {
            Internal{..} => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                let mut job = bcm.find_job_by_guid(&guid)?.unwrap();
                job.complete()?;
                Ok(())
            })()
            .map_err(|e: Error| CompleteJobFailure::Other(e.to_string()))),
        }
    }

    pub fn cancel_job(&mut self, guid: Guid) -> Result<Result<(), CancelJobFailure>, Error> {
        match self {
            Internal{..} => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                let mut job = bcm.find_job_by_guid(&guid)?.unwrap();
                job.cancel()?;
                Ok(())
            })()
            .map_err(|e: Error| CancelJobFailure::Other(e.to_string()))),
        }
    }
}

pub enum BitsMonitorClient {
    Internal {
        job: BitsJob,
        interval_millis: u32,
    }
}

impl BitsMonitorClient {
    pub fn get_status(&mut self, timeout_millis: u32) -> Result<BitsJobStatus, Error> {
        use failure::bail;

        match self {
            BitsMonitorClient::Internal{ job, interval_millis } => {
                // TODO mpsc
                let timeout = Duration::from_millis(timeout_millis as u64);
                let interval = Duration::from_millis(*interval_millis as u64);
                if timeout <= interval {
                    thread::sleep(timeout);
                    bail!("timeout")
                } else {
                    thread::sleep(interval);
                    Ok(job.get_status()?)
                }
            }
        }
    }
}
