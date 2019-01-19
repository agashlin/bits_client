use std::cmp;
use std::collections::HashMap;
use std::ffi;
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use bits::{
    BackgroundCopyManager, BitsJob, BitsJobStatus, BG_JOB_PRIORITY_FOREGROUND,
    BG_JOB_PRIORITY_NORMAL,
};
use comedy::com::InitCom;
use guid_win::Guid;

use bits_protocol::*;

use super::Error;
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
    job_name: ffi::OsString,
    save_path_prefix: ffi::OsString,
    monitors: Mutex<HashMap<Guid, InternalMonitorControl>>,
}

impl InternalClient {
    pub fn new(
        job_name: ffi::OsString,
        save_path_prefix: ffi::OsString,
    ) -> Result<InternalClient, Error> {
        Ok(InternalClient {
            com: InitCom::init_mta()?,
            job_name,
            save_path_prefix,
            monitors: Mutex::new(HashMap::new()),
        })
    }

    pub fn start_job(
        &self,
        url: ffi::OsString,
        save_path: ffi::OsString,
        monitor_interval_millis: u32,
    ) -> Result<(StartJobSuccess, InternalMonitor), StartJobFailure> {
        use StartJobFailure::*;

        let mut full_path = self.save_path_prefix.clone();
        full_path.push(save_path.as_os_str());

        let mut job = BackgroundCopyManager::connect()
            .map_err(|e| Other(e.to_string()))?
            .create_job(&self.job_name)
            .map_err(|e| Create(e.get_hresult().unwrap()))?;

        let guid = job
            .guid()
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        // TODO should the job be cleaned up if this fcn don't return success?
        job.add_file(&url, &full_path)
            .map_err(|e| AddFile(e.get_hresult().unwrap()))?;
        job.resume().map_err(|e| Resume(e.get_hresult().unwrap()))?;

        let (client, control) = InternalMonitor::new(job, monitor_interval_millis)
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        // TODO need to clean up defunct monitors
        self.monitors.lock().unwrap().insert(guid.clone(), control);

        Ok((StartJobSuccess { guid }, client))
    }

    pub fn monitor_job(
        &self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<InternalMonitor, MonitorJobFailure> {
        use MonitorJobFailure::*;

        let (client, control) = InternalMonitor::new(get_job!(&guid), interval_millis)
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        // This will drop any preexisting monitor for the same guid
        self.monitors.lock().unwrap().insert(guid, control);

        Ok(client)
    }

    pub fn resume_job(&self, guid: Guid) -> Result<(), ResumeJobFailure> {
        use ResumeJobFailure::*;

        get_job!(&guid)
            .resume()
            .map_err(|e| ResumeJob(e.get_hresult().unwrap()))?;

        Ok(())
    }

    pub fn set_job_priorty(
        &self,
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
        &self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<(), SetUpdateIntervalFailure> {
        use SetUpdateIntervalFailure::*;

        if let Some(ctrl) = self.monitors.lock().unwrap().get(&guid) {
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

    pub fn stop_update(&self, guid: Guid) -> Result<(), SetUpdateIntervalFailure> {
        use SetUpdateIntervalFailure::*;

        if let Some(ctrl) = self.monitors.lock().unwrap().get(&guid) {
            if ctrl.sender.lock().unwrap().send(InternalMonitorMessage::CloseMonitor).is_ok()
            {
                Ok(())
            } else {
                Err(Other(String::from("disconnected")))
            }
        } else {
            Err(NotFound)
        }
    }

    pub fn complete_job(&self, guid: Guid) -> Result<(), CompleteJobFailure> {
        use CompleteJobFailure::*;

        get_job!(&guid)
            .complete()
            .map_err(|e| CompleteJob(e.get_hresult().unwrap()))?;

        Ok(())
    }

    pub fn cancel_job(&self, guid: Guid) -> Result<(), CancelJobFailure> {
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
    JobError,
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
        let transferred_cb = Box::new(move || {
            let sender = transferred_sender_mutex.lock().unwrap();

            // TODO should try to cleanup if the send fails?
            // In particular, this callback should only be called once...
            let _result = sender.send(JobTransferred);

            Ok(())
        });

        let error_sender_mutex = Mutex::new(sender.clone());
        let error_cb = Box::new(move || {
            let sender = error_sender_mutex.lock().unwrap();

            // TODO should try to cleanup if the send fails?
            let _result = sender.send(JobError);

            Ok(())
        });

        let unregistered_sender_mutex = Mutex::new(sender.clone());
        let unregistered_cb = Box::new(move || {
            // TODO I'm not sure if we should necessarily close the monitor when the callbacks
            // become unregistered. We should probably know, but we don't want to have to start
            // up another monitor and get into a tug-of-war with whatever other process might
            // have been peeking. Then again, when that process finishes, then no one will be
            // monitoring, and we may miss events.
            let sender = unregistered_sender_mutex.lock().unwrap();
            let _result = sender.send(CloseMonitor);
        });

        let _callbacks_handle =
            job.register_callbacks(
                Some(transferred_cb),
                Some(error_cb),
                None,
                Some(unregistered_cb))?;

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
        let timeout = Duration::from_millis(timeout_millis as u64);

        let started = Instant::now();

        loop {
            if started.elapsed() > timeout {
                return Err(Error::Timeout);
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
                // Remote get_status will disconnect the pipe, simulate that here.
                return self.job.get_status()
                    .map_err(|_| Error::NotConnected);
            }

            // TODO with this implementation we are never guaranteed to eventually get messages
            // (such as CloseMonitor) from the queue
            match self.receiver.recv_timeout(wait_until.unwrap() - now) {
                Ok(CloseMonitor) => {
                    // TODO should try unregistering notifications on close?
                    return Err(Error::NotConnected);
                }
                Ok(SetInterval(new_millis)) => {
                    self.interval_millis = new_millis;
                    // fall through to loop
                }
                Ok(JobError) | Ok(JobTransferred) | Err(mpsc::RecvTimeoutError::Timeout) => {
                    self.last_status = Some(Instant::now());
                    // Remote get_status errors will disconnect the pipe, simulate that here.
                    return self.job.get_status()
                        .map_err(|_| Error::NotConnected);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Error::NotConnected);
                }
            }
        }
    }
}

impl Drop for InternalMonitor {
    fn drop(&mut self) {
        // This Drop should probably be what removes the monitor from InternalClient::monitors
        //let _result = self.job.clear_callbacks();
    }
}
