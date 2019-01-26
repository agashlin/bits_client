use std::cmp;
use std::collections::{hash_map, HashMap};
use std::ffi;
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use bits::{
    BackgroundCopyManager, BitsJob, BitsJobStatus, BG_JOB_PRIORITY_FOREGROUND,
    BG_JOB_PRIORITY_NORMAL, E_FAIL,
};
use comedy::com::InitCom;
use guid_win::Guid;

use bits_protocol::*;

use self::InProcessMonitorMessage::*;
use super::Error;

// This is a macro in order to use whatever NotFound is in scope.
macro_rules! get_job {
    ($guid:expr, $name:expr) => {{
        let bcm = BackgroundCopyManager::connect().map_err(|e| Other(e.to_string()))?;

        bcm.get_job_by_guid_and_name($guid, $name)
            .map_err(|e| GetJob(e.get_hresult().unwrap()))?
            .ok_or(NotFound)?
    }};
}

pub struct InProcessClient {
    _com: InitCom,
    job_name: ffi::OsString,
    save_path_prefix: ffi::OsString,
    monitors: HashMap<Guid, InProcessMonitorControl>,
}

impl InProcessClient {
    pub fn new(
        job_name: ffi::OsString,
        save_path_prefix: ffi::OsString,
    ) -> Result<InProcessClient, Error> {
        Ok(InProcessClient {
            _com: InitCom::init_mta()?,
            job_name,
            save_path_prefix,
            monitors: HashMap::new(),
        })
    }

    pub fn start_job(
        &mut self,
        url: ffi::OsString,
        save_path: ffi::OsString,
        monitor_interval_millis: u32,
    ) -> Result<(StartJobSuccess, InProcessMonitor), StartJobFailure> {
        use StartJobFailure::*;

        // TODO normalize and verify path after append
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

        let (client, control) = InProcessMonitor::new(job, monitor_interval_millis)
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        self.monitors.insert(guid.clone(), control);

        Ok((StartJobSuccess { guid }, client))
    }

    pub fn monitor_job(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<InProcessMonitor, MonitorJobFailure> {
        use MonitorJobFailure::*;

        let (client, control) =
            InProcessMonitor::new(get_job!(&guid, &self.job_name), interval_millis)
                .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        // Stop any preexisting monitor for the same guid
        let _ = self.stop_update(guid.clone());

        self.monitors.insert(guid, control);

        Ok(client)
    }

    pub fn resume_job(&mut self, guid: Guid) -> Result<(), ResumeJobFailure> {
        use ResumeJobFailure::*;

        get_job!(&guid, &self.job_name)
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

        get_job!(&guid, &self.job_name)
            .set_priority(priority)
            .map_err(|e| ApplySettings(e.get_hresult().unwrap()))?;

        Ok(())
    }

    fn get_monitor_control_sender(
        &mut self,
        guid: Guid,
    ) -> Option<Arc<Mutex<mpsc::Sender<InProcessMonitorMessage>>>> {
        if let hash_map::Entry::Occupied(occ) = self.monitors.entry(guid) {
            if let Some(sender) = occ.get().sender.upgrade() {
                Some(sender)
            } else {
                occ.remove_entry();
                None
            }
        } else {
            None
        }
    }

    pub fn set_update_interval(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<(), SetUpdateIntervalFailure> {
        use SetUpdateIntervalFailure::*;

        if let Some(sender) = self.get_monitor_control_sender(guid) {
            sender
                .lock()
                .unwrap()
                .send(InProcessMonitorMessage::SetInterval(interval_millis))
                .map_err(|_| NotFound)
        } else {
            Err(NotFound)
        }
    }

    pub fn stop_update(&mut self, guid: Guid) -> Result<(), SetUpdateIntervalFailure> {
        use SetUpdateIntervalFailure::*;

        if let Some(sender) = self.get_monitor_control_sender(guid) {
            sender
                .lock()
                .unwrap()
                .send(InProcessMonitorMessage::StopMonitor)
                .map_err(|_| NotFound)
        } else {
            Err(NotFound)
        }
    }

    pub fn complete_job(&mut self, guid: Guid) -> Result<(), CompleteJobFailure> {
        use CompleteJobFailure::*;

        get_job!(&guid, &self.job_name)
            .complete()
            .map_err(|e| CompleteJob(e.get_hresult().unwrap()))?;

        Ok(())
    }

    pub fn cancel_job(&mut self, guid: Guid) -> Result<(), CancelJobFailure> {
        use CancelJobFailure::*;

        get_job!(&guid, &self.job_name)
            .cancel()
            .map_err(|e| CancelJob(e.get_hresult().unwrap()))?;

        Ok(())
    }
}

struct InProcessMonitorControl {
    sender: Weak<Mutex<mpsc::Sender<InProcessMonitorMessage>>>,
}

enum InProcessMonitorMessage {
    SetInterval(u32),
    JobError,
    JobTransferred,
    #[allow(dead_code)]
    StopMonitor,
}

pub struct InProcessMonitor {
    job: BitsJob,
    control_sender: Arc<Mutex<mpsc::Sender<InProcessMonitorMessage>>>,
    receiver: Option<mpsc::Receiver<InProcessMonitorMessage>>,
    interval_millis: u32,
    last_status: Option<Instant>,
    priority_boosted: bool,
}

impl InProcessMonitor {
    fn new(
        mut job: BitsJob,
        interval_millis: u32,
    ) -> Result<(InProcessMonitor, InProcessMonitorControl), comedy::Error> {
        let (sender, receiver) = mpsc::channel();

        let transferred_sender_mutex = Mutex::new(sender.clone());
        let transferred_cb = Box::new(move || {
            let sender = transferred_sender_mutex.lock().unwrap();

            sender.send(JobTransferred).map_err(|_| E_FAIL)
        });

        let error_sender_mutex = Mutex::new(sender.clone());
        let error_cb = Box::new(move || {
            let sender = error_sender_mutex.lock().unwrap();

            sender.send(JobError).map_err(|_| E_FAIL)
        });

        job.register_callbacks(Some(transferred_cb), Some(error_cb), None)?;

        // Ignore set priority failure
        eprintln!("setting priority to foreground");
        let _ = job.set_priority(BG_JOB_PRIORITY_FOREGROUND);

        let monitor = InProcessMonitor {
            job,
            control_sender: Arc::new(Mutex::new(sender)),
            receiver: Some(receiver),
            interval_millis,
            last_status: None,
            priority_boosted: true,
        };
        let control = InProcessMonitorControl {
            sender: Arc::downgrade(&monitor.control_sender),
        };

        Ok((monitor, control))
    }

    fn disconnect(&mut self) {
        if self.priority_boosted {
            eprintln!("setting priority back to normal");
            let _ = self.job.set_priority(BG_JOB_PRIORITY_NORMAL);
            self.priority_boosted = false;
        }

        self.receiver = None;
    }

    fn get_status_now(&mut self) -> Result<BitsJobStatus, Error> {
        self.last_status = Some(Instant::now());
        let result = self.job.get_status();
        match result {
            Ok(status) => Ok(status),
            Err(_) => {
                self.disconnect();
                Err(Error::NotConnected)
            }
        }
    }

    pub fn get_status(&mut self, timeout_millis: u32) -> Result<BitsJobStatus, Error> {
        let timeout = Duration::from_millis(timeout_millis as u64);

        let started = Instant::now();

        loop {
            if self.receiver.is_none() {
                return Err(Error::NotConnected);
            }

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
                return self.get_status_now();
            }

            match self
                .receiver
                .as_ref()
                .unwrap()
                .recv_timeout(wait_until.unwrap() - now)
            {
                Ok(StopMonitor) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Disconnection, drop the receiver.
                    self.disconnect();
                    return Err(Error::NotConnected);
                }
                Ok(SetInterval(new_millis)) => {
                    self.interval_millis = new_millis;
                    // Fall through to loop
                }
                Ok(JobError) | Ok(JobTransferred) | Err(mpsc::RecvTimeoutError::Timeout) => {
                    return self.get_status_now();
                }
            }
        }
    }
}

impl Drop for InProcessMonitor {
    fn drop(&mut self) {
        self.disconnect();
    }
}
