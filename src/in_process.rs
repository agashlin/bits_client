use std::cmp;
use std::collections::HashMap;
use std::ffi;
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use bits::{
    BackgroundCopyManager, BitsJob, BitsJobError, BitsJobStatus, BG_JOB_PRIORITY_FOREGROUND,
    BG_JOB_PRIORITY_NORMAL,
};
use comedy::com::InitCom;
use failure::Error;
use guid_win::Guid;

use bits_protocol::*;

use self::InternalMonitorMessage::*;

macro_rules! get_job {
    ($guid:expr) => {{
        let bcm = BackgroundCopyManager::connect().map_err(|e| Other(e.to_string()))?;

        bcm.get_job_by_guid($guid)
            .map_err(|e| GetJob(e.get_hresult().unwrap()))?
            .ok_or(NotFound)?
    }};
}

pub struct InternalClient {
    #[allow(dead_code)]
    com: InitCom,
    monitors: HashMap<Guid, InternalMonitorControl>,
}

impl InternalClient {
    pub fn new() -> Result<InternalClient, Error> {
        Ok(InternalClient {
            com: InitCom::init_mta()?,
            monitors: HashMap::new(),
        })
    }

    pub fn start_job(
        &mut self,
        url: ffi::OsString,
        save_path: ffi::OsString,
        monitor_interval_millis: u32,
    ) -> Result<(StartJobSuccess, InternalMonitor), StartJobFailure> {
        use StartJobFailure::*;

        // TODO determine real name
        let mut job = BackgroundCopyManager::connect()
            .map_err(|e| Other(e.to_string()))?
            .create_job(&ffi::OsString::from("JOBBO"))
            .map_err(|e| Create(e.get_hresult().unwrap()))?;

        let guid = job
            .guid()
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        // TODO should the job be cleaned up if this fcn don't return success?
        job.add_file(&url, &save_path)
            .map_err(|e| AddFile(e.get_hresult().unwrap()))?;
        job.resume().map_err(|e| Resume(e.get_hresult().unwrap()))?;

        let (client, control) = InternalMonitor::new(job, monitor_interval_millis)
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        self.monitors.insert(guid.clone(), control);

        Ok((StartJobSuccess { guid }, client))
    }

    pub fn monitor_job(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<InternalMonitor, MonitorJobFailure> {
        use MonitorJobFailure::*;

        let (client, control) = InternalMonitor::new(get_job!(&guid), interval_millis)
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        // This will drop any preexisting monitor for the same guid
        self.monitors.insert(guid, control);

        Ok(client)
    }

    pub fn resume_job(&mut self, guid: Guid) -> Result<(), ResumeJobFailure> {
        use ResumeJobFailure::*;

        get_job!(&guid)
            .resume()
            .map_err(|e| ResumeJob(e.get_hresult().unwrap()))?;

        Ok(())
    }

    pub fn set_job_priorty(
        &mut self,
        guid: Guid,
        foreground: bool,
    ) -> Result<(), SetJobPriorityFailure> {
        use SetJobPriorityFailure::*;

        let priority = if foreground {
            BG_JOB_PRIORITY_FOREGROUND
        } else {
            BG_JOB_PRIORITY_NORMAL
        };

        get_job!(&guid)
            .set_priority(priority)
            .map_err(|e| ApplySettings(e.get_hresult().unwrap()))?;

        Ok(())
    }

    pub fn set_update_interval(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<(), SetUpdateIntervalFailure> {
        use SetUpdateIntervalFailure::*;

        if let Some(ctrl) = self.monitors.get(&guid) {
            if ctrl
                .sender
                .lock()
                .unwrap()
                .send(InternalMonitorMessage::SetInterval(interval_millis))
                .is_ok()
            {
                Ok(())
            } else {
                Err(Other(String::from("disconnected")))
            }
        } else {
            Err(NotFound)
        }
    }

    pub fn complete_job(&mut self, guid: Guid) -> Result<(), CompleteJobFailure> {
        use CompleteJobFailure::*;

        get_job!(&guid)
            .complete()
            .map_err(|e| CompleteJob(e.get_hresult().unwrap()))?;

        Ok(())
    }

    pub fn cancel_job(&mut self, guid: Guid) -> Result<(), CancelJobFailure> {
        use CancelJobFailure::*;

        get_job!(&guid)
            .cancel()
            .map_err(|e| CancelJob(e.get_hresult().unwrap()))?;

        Ok(())
    }
}

struct InternalMonitorControl {
    sender: Mutex<mpsc::Sender<InternalMonitorMessage>>,
}

enum InternalMonitorMessage {
    SetInterval(u32),
    JobError(BitsJobError),
    JobTransferred,
    #[allow(dead_code)]
    CloseMonitor,
}

pub struct InternalMonitor {
    job: BitsJob,
    receiver: mpsc::Receiver<InternalMonitorMessage>,
    interval_millis: u32,
    last_status: Option<Instant>,
}

impl InternalMonitor {
    fn new(
        mut job: BitsJob,
        interval_millis: u32,
    ) -> Result<(InternalMonitor, InternalMonitorControl), comedy::Error> {
        let (sender, receiver) = mpsc::channel();

        let transferred_sender_mutex = Mutex::new(sender.clone());
        let transferred_cb = Box::new(move |_job| {
            let sender = transferred_sender_mutex.lock().unwrap();

            // TODO should try to cleanup if the send fails?
            // In particular, this callback should only be called once...
            let _result = sender.send(JobTransferred);
        });

        let error_sender_mutex = Mutex::new(sender.clone());
        let error_cb = Box::new(move |_job, err| {
            let sender = error_sender_mutex.lock().unwrap();

            // TODO should try to cleanup if the send fails?
            let _result = sender.send(JobError(err));
        });

        let _callbacks_handle =
            job.register_callbacks(Some(transferred_cb), Some(error_cb), None)?;

        Ok((
            InternalMonitor {
                job,
                receiver,
                interval_millis,
                last_status: None,
            },
            InternalMonitorControl {
                sender: Mutex::new(sender),
            },
        ))
    }

    pub fn get_status(&mut self, timeout_millis: u32) -> Result<BitsJobStatus, Error> {
        use failure::bail;

        let timeout = Duration::from_millis(timeout_millis as u64);

        let started = Instant::now();

        loop {
            if started.elapsed() > timeout {
                bail!("timeout");
            }

            let interval = Duration::from_millis(self.interval_millis as u64);
            let wait_until = if let Some(last_status) = self.last_status {
                Some(cmp::min(last_status + interval, started + timeout))
            } else {
                None
            };

            let now = Instant::now();

            if wait_until.is_none() || wait_until.unwrap() < now {
                self.last_status = Some(now);
                return Ok(self.job.get_status()?);
            }

            // TODO with this implementation we are never guaranteed to eventually get messages
            // (such as CloseMonitor) from the queue
            match self.receiver.recv_timeout(wait_until.unwrap() - now) {
                Ok(CloseMonitor) => {
                    // TODO should try unregistering notifications on close?
                    bail!("monitor shutting down");
                }
                Ok(SetInterval(new_millis)) => {
                    self.interval_millis = new_millis;
                    // fall through to loop
                }
                Ok(JobError(_)) | Ok(JobTransferred) | Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.last_status = Some(Instant::now());
                    return Ok(self.job.get_status()?);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("disconnected");
                }
            }
        }
    }
}
