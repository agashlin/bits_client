use std::cmp;
use std::collections::{hash_map, HashMap};
use std::ffi;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use bits::{
    BackgroundCopyManager, BitsJob, BitsJobPriority, BitsJobStatus, BitsProxyUsage, E_FAIL,
};
use guid_win::Guid;

use bits_protocol::*;

use super::Error;

// This is a macro in order to use NotFound from whatever enum is in scope.
macro_rules! get_job {
    ($guid:expr, $name:expr) => {{
        BackgroundCopyManager::connect()
            .map_err(|e| Other(e.to_string()))?
            .find_job_by_guid_and_name($guid, $name)
            .map_err(|e| GetJob(e.get_hresult().unwrap()))?
            .ok_or(NotFound)?
    }};
}

pub struct InProcessClient {
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
            job_name,
            save_path_prefix,
            monitors: HashMap::new(),
        })
    }

    pub fn start_job(
        &mut self,
        url: ffi::OsString,
        save_path: ffi::OsString,
        proxy_usage: BitsProxyUsage,
        monitor_interval_millis: u32,
    ) -> Result<(StartJobSuccess, InProcessMonitor), StartJobFailure> {
        use StartJobFailure::*;
        // TODO should the job be cleaned up if this fcn can't return success?

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

        job.set_proxy_usage(proxy_usage)
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        let (client, control) = InProcessMonitor::new(&mut job, monitor_interval_millis)
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        // TODO: this will need to be optional eventually
        job.set_priority(BitsJobPriority::Foreground)
            .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        job.add_file(&url, &full_path)
            .map_err(|e| AddFile(e.get_hresult().unwrap()))?;
        job.resume().map_err(|e| Resume(e.get_hresult().unwrap()))?;

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
            InProcessMonitor::new(&mut get_job!(&guid, &self.job_name), interval_millis)
                .map_err(|e| OtherBITS(e.get_hresult().unwrap()))?;

        // Stop any preexisting monitor for the same guid
        let _ = self.stop_update(guid.clone());

        self.monitors.insert(guid, control);

        Ok(client)
    }

    pub fn suspend_job(&mut self, guid: Guid) -> Result<(), SuspendJobFailure> {
        use SuspendJobFailure::*;

        get_job!(&guid, &self.job_name)
            .suspend()
            .map_err(|e| SuspendJob(e.get_hresult().unwrap()))?;

        Ok(())
    }

    pub fn resume_job(&mut self, guid: Guid) -> Result<(), ResumeJobFailure> {
        use ResumeJobFailure::*;

        get_job!(&guid, &self.job_name)
            .resume()
            .map_err(|e| ResumeJob(e.get_hresult().unwrap()))?;

        Ok(())
    }

    pub fn set_job_priority(
        &mut self,
        guid: Guid,
        foreground: bool,
    ) -> Result<(), SetJobPriorityFailure> {
        use SetJobPriorityFailure::*;

        let priority = if foreground {
            BitsJobPriority::Foreground
        } else {
            BitsJobPriority::Normal
        };

        get_job!(&guid, &self.job_name)
            .set_priority(priority)
            .map_err(|e| ApplySettings(e.get_hresult().unwrap()))?;

        Ok(())
    }

    fn get_monitor_control_sender(
        &mut self,
        guid: Guid,
    ) -> Option<Arc<ControlPair>> {
        if let hash_map::Entry::Occupied(occ) = self.monitors.entry(guid) {
            if let Some(sender) = occ.get().0.upgrade() {
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
            let mut s = sender.1.lock().unwrap();
            s.interval_millis = interval_millis;
            sender.0.notify_all();
            Ok(())
        } else {
            Err(NotFound)
        }
    }

    pub fn stop_update(&mut self, guid: Guid) -> Result<(), SetUpdateIntervalFailure> {
        use SetUpdateIntervalFailure::*;

        if let Some(sender) = self.get_monitor_control_sender(guid) {
            sender.1.lock().unwrap().shutdown = true;
            sender.0.notify_all();
            Ok(())
        } else {
            Err(NotFound)
        }
    }

    pub fn complete_job(&mut self, guid: Guid) -> Result<(), CompleteJobFailure> {
        use CompleteJobFailure::*;

        get_job!(&guid, &self.job_name)
            .complete()
            .map_err(|e| CompleteJob(e.get_hresult().unwrap()))?;

        let _ = self.stop_update(guid);

        Ok(())
    }

    pub fn cancel_job(&mut self, guid: Guid) -> Result<(), CancelJobFailure> {
        use CancelJobFailure::*;

        get_job!(&guid, &self.job_name)
            .cancel()
            .map_err(|e| CancelJob(e.get_hresult().unwrap()))?;

        let _ = self.stop_update(guid);

        Ok(())
    }
}

// The `Condvar` is notified when `InProcessMonitorVars` changes.
type ControlPair = (Condvar, Mutex<InProcessMonitorVars>);
struct InProcessMonitorControl(Weak<ControlPair>);

// see https://github.com/rust-lang/rust/issues/54768
impl std::panic::RefUnwindSafe for InProcessMonitorControl {}

struct InProcessMonitorVars {
    interval_millis: u32,
    notified: bool,
    shutdown: bool,
}

pub struct InProcessMonitor {
    vars: Arc<ControlPair>,
    guid: Guid,
    last_status: Option<Instant>,
}

impl InProcessMonitor {
    fn new(
        job: &mut BitsJob,
        interval_millis: u32,
    ) -> Result<(InProcessMonitor, InProcessMonitorControl), comedy::Error> {
        let guid = job.guid()?;

        let vars = Arc::new((Condvar::new(),
            Mutex::new(InProcessMonitorVars {
                interval_millis,
                notified: false,
                shutdown: false,
            })));

        let transferred_control = InProcessMonitorControl(Arc::downgrade(&vars));
        let transferred_cb = Box::new(move || {
            if let Some(control) = transferred_control.0.upgrade() {
                if let Ok(mut vars) = control.1.lock() {
                    vars.notified = true;
                    control.0.notify_all();
                    return Ok(());
                }
            }
            Err(E_FAIL)
        });

        let error_control = InProcessMonitorControl(Arc::downgrade(&vars));
        let error_cb = Box::new(move || {
            if let Some(control) = error_control.0.upgrade() {
                if let Ok(mut vars) = control.1.lock() {
                    vars.notified = true;
                    control.0.notify_all();
                    return Ok(())
                }
            }
            Err(E_FAIL)
        });

        job.register_callbacks(Some(transferred_cb), Some(error_cb), None)?;

        let control = InProcessMonitorControl(Arc::downgrade(&vars));

        let monitor = InProcessMonitor {
            guid,
            vars,
            last_status: None,
        };

        Ok((monitor, control))
    }

    fn job(&self) -> Result<BitsJob, Error> {
        Ok(BackgroundCopyManager::connect()?.get_job_by_guid(&self.guid)?)
    }

    pub fn get_status(&mut self, timeout_millis: u32) -> Result<BitsJobStatus, Error> {
        let timeout = Duration::from_millis(timeout_millis as u64);

        let started = Instant::now();

        let result = loop {
            let mut s = self.vars.1.lock().unwrap();

            if s.shutdown {
                // Already disconnected, return error.
                return Err(Error::NotConnected);
            }

            if started.elapsed() > timeout {
                // Disconnect and return error.
                break Err(Error::Timeout);
            }

            // Get the interval every pass through the loop, in case it has changed.
            let interval = Duration::from_millis(s.interval_millis as u64);

            let wait_until = if let Some(last_status) = self.last_status {
                Some(cmp::min(last_status + interval, started + timeout))
            } else {
                None
            };

            let now = Instant::now();

            if s.notified || wait_until.is_none() || wait_until.unwrap() < now {
                // Aready notified, or this is the first status report, so
                // immediately get status below.
                s.notified = false;
                break Ok(());
            }
            let wait_until = wait_until.unwrap();

            // unwrap instead of handling PoisonError
            let (mut s, _timeout_result) = self.vars.0.wait_timeout(s, wait_until - now).unwrap();
            if s.shutdown {
                // Disconnected, return error.
                return Err(Error::NotConnected);
            }

            if !s.notified && Instant::now() < wait_until {
                // Non-notification wakeup (spurious or interval changed), wait again
                continue;
            }

            // Got a notification or waited the set interval, get status below.
            s.notified = false;
            break Ok(());
        };

        let result = if result.is_ok() {
            self.last_status = Some(Instant::now());
            if let Ok(job) = self.job() {
                if let Ok(status) = job.get_status() {
                    return Ok(status);
                }
            }
            Err(Error::NotConnected)
        } else {
            result
        };

        self.vars.1.lock().unwrap().shutdown = true;
        Err(result.unwrap_err())
    }
}
