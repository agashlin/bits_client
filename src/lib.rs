extern crate bits;
extern crate comedy;
extern crate failure;
extern crate guid_win;

#[cfg(feature = "local_service_task")]
extern crate bincode;
#[cfg(feature = "local_service_task")]
extern crate named_pipe;
#[cfg(feature = "local_service_task")]
extern crate serde;
#[cfg(feature = "local_service_task")]
extern crate serde_derive;
#[cfg(feature = "local_service_task")]
extern crate task_service;

pub mod bits_protocol;

use std::ffi;
use std::thread;
use std::time::Duration;

use bits::status::BitsJobStatus;
use bits::{BackgroundCopyManager, BitsJob, BG_JOB_PRIORITY_FOREGROUND, BG_JOB_PRIORITY_NORMAL};
use comedy::com::InitCom;
use failure::Error;
use guid_win::Guid;

#[cfg(feature = "local_service_task")]
use bincode::{deserialize, serialize};
#[cfg(feature = "local_service_task")]
use named_pipe::{PipeAccess, PipeServer, WaitTimeout};

use bits_protocol::*;

// The IPC is structured so that the client runs as a named pipe server, accepting connections
// from the BITS task server once it starts up, which it then uses to issue commands.
// This is done so that the client can create the pipe and wait for a connection to know when the
// task is ready for commands; otherwise it would have to repeatedly try to connect until the
// server creates the pipe.

pub enum BitsClient {
    #[cfg(feature = "local_service_task")]
    Task(PipeServer),
    Internal(InitCom),
}
use BitsClient::*;

impl BitsClient {
    pub fn new(com_inited: InitCom) -> BitsClient {
        BitsClient::Internal(com_inited)
    }

    // TODO needs the rest of the args
    #[cfg(feature = "local_service_task")]
    pub fn connect_task(task_name: &ffi::OsStr) -> Result<BitsClient, Error> {
        use failure::ensure;

        let mut pipe = PipeServer::new_duplex(PipeAccess::LocalService)?;

        {
            // Start the task, which will connect back to the pipe for commands.
            let mut arg = ffi::OsString::from("command-connect ");
            arg.push(pipe.name());
            let _running_task = bits_server::task::run_on_demand(task_name, arg.as_os_str())?;
            // TODO: wait for running task to start running?, get pid?
        }

        // TODO: check pid
        // TODO: real timeout (here and below)
        pipe.connect(WaitTimeout::infinite())?;

        // exchange protocol version handshake
        pipe.write(&[PROTOCOL_VERSION; 1], WaitTimeout::infinite())?;
        let mut reply = [0; 1];
        let reply = pipe.read(&mut reply, WaitTimeout::infinite())?;

        ensure!(reply.len() == 1, "wrong version length");
        ensure!(
            reply[0] == PROTOCOL_VERSION,
            "protocol version {} != expected {}",
            reply[0],
            PROTOCOL_VERSION
        );

        Ok(BitsClient::Task(pipe))
    }

    pub fn start_job(
        &mut self,
        url: ffi::OsString,
        save_path: ffi::OsString,
        monitor_interval_millis: u32,
    ) -> Result<Result<(StartJobSuccess, BitsMonitorClient), StartJobFailure>, Error> {
        match self {
            #[cfg(feature = "local_service_task")]
            Task(ref mut pipe) => {
                let mut monitor_pipe = PipeServer::new_inbound(PipeAccess::LocalService)?;

                let reply = run_command(
                    pipe,
                    StartJobCommand {
                        url,
                        save_path,
                        monitor: Some(MonitorConfig {
                            pipe_name: monitor_pipe.name().to_os_string(),
                            interval_millis: monitor_interval_millis,
                        }),
                    },
                )?;

                Ok(if let Ok(success) = reply {
                    // TODO: should not be infinite
                    monitor_pipe.connect(WaitTimeout::infinite())?;
                    Ok((success, BitsMonitorClient::Task(monitor_pipe)))
                } else {
                    Err(reply.unwrap_err())
                })
            }
            Internal(_) => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                // TODO determine name
                let mut job = bcm.create_job(&ffi::OsString::from("JOBBO"))?;
                job.add_file(&url, &save_path)?;
                job.resume()?;

                // TODO setup monitor callbacks
                Ok((
                    StartJobSuccess { guid: job.guid()? },
                    BitsMonitorClient::Internal((job, monitor_interval_millis)),
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
            #[cfg(feature = "local_service_task")]
            Task(ref mut pipe) => {
                let mut monitor_pipe = PipeServer::new_inbound(PipeAccess::LocalService)?;

                let reply = run_command(
                    pipe,
                    MonitorJobCommand {
                        guid,
                        monitor: MonitorConfig {
                            pipe_name: monitor_pipe.name().to_os_string(),
                            interval_millis: interval_millis,
                        },
                    },
                )?;

                Ok(if reply.is_ok() {
                    // TODO: should not be infinite
                    monitor_pipe.connect(WaitTimeout::infinite())?;
                    Ok(BitsMonitorClient::Task(monitor_pipe))
                } else {
                    Err(reply.unwrap_err())
                })
            }
            Internal(_) => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                let job = bcm.find_job_by_guid(&guid)?.unwrap();

                Ok(BitsMonitorClient::Internal((job, interval_millis)))
            })()
            .map_err(|e: Error| MonitorJobFailure::Other(e.to_string()))),
        }
    }

    pub fn resume_job(&mut self, guid: Guid) -> Result<Result<(), ResumeJobFailure>, Error> {
        match self {
            #[cfg(feature = "local_service_task")]
            Task(ref mut pipe) => run_command(pipe, ResumeJobCommand { guid }),
            Internal(_) => Ok((move || {
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
            #[cfg(feature = "local_service_task")]
            Task(ref mut pipe) => run_command(pipe, SetJobPriorityCommand { guid, foreground }),
            Internal(_) => Ok((move || {
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
            #[cfg(feature = "local_service_task")]
            Task(ref mut pipe) => run_command(
                pipe,
                SetUpdateIntervalCommand {
                    guid,
                    interval_millis,
                },
            ),
            Internal(_) => {
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
            #[cfg(feature = "local_service_task")]
            Task(ref mut pipe) => run_command(pipe, CompleteJobCommand { guid }),
            Internal(_) => Ok((move || {
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
            #[cfg(feature = "local_service_task")]
            Task(ref mut pipe) => run_command(pipe, CancelJobCommand { guid }),
            Internal(_) => Ok((move || {
                let bcm = BackgroundCopyManager::connect()?;
                let mut job = bcm.find_job_by_guid(&guid)?.unwrap();
                job.cancel()?;
                Ok(())
            })()
            .map_err(|e: Error| CancelJobFailure::Other(e.to_string()))),
        }
    }
}

#[cfg(feature = "local_service_task")]
fn run_command<T>(pipe: &mut PipeServer, cmd: T) -> Result<Result<T::Success, T::Failure>, Error>
where
    T: CommandType,
{
    use failure::bail;

    let cmd = serialize(&T::new(cmd)).unwrap();
    assert!(cmd.len() <= MAX_COMMAND);
    let mut out_buf: [u8; MAX_RESPONSE] = unsafe { std::mem::uninitialized() };

    pipe.write(&cmd, WaitTimeout::infinite())?;
    let reply = pipe.read(&mut out_buf, WaitTimeout::infinite())?;

    match deserialize(reply) {
        Err(e) => bail!("deserialize failed: {}", e),
        Ok(r) => Ok(r),
    }
}

pub enum BitsMonitorClient {
    #[cfg(feature = "local_service_task")]
    Task(PipeServer),
    Internal((BitsJob, u32)),
}

impl BitsMonitorClient {
    pub fn get_status(&mut self, timeout_millis: u32) -> Result<BitsJobStatus, Error> {
        use failure::bail;

        match self {
            #[cfg(feature = "local_service_task")]
            BitsMonitorClient::Task(ref mut pipe) => {
                let mut out_buf: [u8; MAX_RESPONSE] = unsafe { std::mem::uninitialized() };
                Ok(deserialize(pipe.read(
                    &mut out_buf,
                    WaitTimeout::from_millis(timeout_millis).unwrap(),
                )?)?)
            }
            BitsMonitorClient::Internal((job, period_millis)) => {
                // TODO mpsc
                let timeout = Duration::from_millis(timeout_millis as u64);
                let period = Duration::from_millis(*period_millis as u64);
                if timeout <= period {
                    thread::sleep(timeout);
                    bail!("timeout")
                } else {
                    thread::sleep(period);
                    Ok(job.get_status()?)
                }
            }
        }
    }
}
