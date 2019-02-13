/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::cmp;
use std::collections::{hash_map, HashMap};
use std::ffi;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use bits::{
    BackgroundCopyManager, BitsErrorContext, BitsJob, BitsJobPriority, BitsJobState,
    BitsProxyUsage, E_FAIL,
};
use guid_win::Guid;

use bits_protocol::*;

use super::Error;

// This is a macro in order to use the NotFound and GetJob variants from whatever enum is in scope.
macro_rules! get_job {
    ($bcm:ident, $guid:expr, $name:expr) => {{
        $bcm = BackgroundCopyManager::connect().map_err(|e| Other(e.to_string()))?;
        $bcm.find_job_by_guid_and_name($guid, $name)
            .map_err(|e| GetJob($crate::in_process::format_error(&$bcm, e)))?
            .ok_or(NotFound)?
    }};
}

fn format_error(bcm: &BackgroundCopyManager, error: comedy::Error) -> HResultMessage {
    let hr = error.get_hresult().unwrap();
    let bits_description = bcm.get_error_description(hr).ok();

    HResultMessage {
        hr,
        message: if let Some(desc) = bits_description {
            format!("{}: {}", error, desc)
        } else {
            format!("{}", error)
        },
    }
}

/// The in-process client uses direct BITS calls via the `bits` crate.
///
/// Note that "in-process" does not refer to the BITS COM server, which is out-of-process.
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

        let bcm = BackgroundCopyManager::connect().map_err(|e| Other(e.to_string()))?;
        let mut job = bcm
            .create_job(&self.job_name)
            .map_err(|e| Create(format_error(&bcm, e)))?;

        let guid = job.guid().map_err(|e| OtherBITS(format_error(&bcm, e)))?;

        (|| {
            job.set_proxy_usage(proxy_usage)?;
            job.set_minimum_retry_delay(60)?;
            job.set_redirect_report()?;

            // TODO: this will need to be optional eventually
            job.set_priority(BitsJobPriority::Foreground)?;

            Ok(())
        })()
        .map_err(|e| ApplySettings(format_error(&bcm, e)))?;

        let (client, control) = InProcessMonitor::new(&mut job, monitor_interval_millis)
            .map_err(|e| OtherBITS(format_error(&bcm, e)))?;

        job.add_file(&url, &full_path)
            .map_err(|e| AddFile(format_error(&bcm, e)))?;

        job.resume().map_err(|e| Resume(format_error(&bcm, e)))?;

        self.monitors.insert(guid.clone(), control);

        Ok((StartJobSuccess { guid }, client))
    }

    pub fn monitor_job(
        &mut self,
        guid: Guid,
        interval_millis: u32,
    ) -> Result<InProcessMonitor, MonitorJobFailure> {
        use MonitorJobFailure::*;

        let bcm;
        let (client, control) =
            InProcessMonitor::new(&mut get_job!(bcm, &guid, &self.job_name), interval_millis)
                .map_err(|e| OtherBITS(format_error(&bcm, e)))?;

        // Stop any preexisting monitor for the same guid
        let _ = self.stop_update(guid.clone());

        self.monitors.insert(guid, control);

        Ok(client)
    }

    pub fn suspend_job(&mut self, guid: Guid) -> Result<(), SuspendJobFailure> {
        use SuspendJobFailure::*;

        let bcm;
        get_job!(bcm, &guid, &self.job_name)
            .suspend()
            .map_err(|e| SuspendJob(format_error(&bcm, e)))?;

        Ok(())
    }

    pub fn resume_job(&mut self, guid: Guid) -> Result<(), ResumeJobFailure> {
        use ResumeJobFailure::*;

        let bcm;
        get_job!(bcm, &guid, &self.job_name)
            .resume()
            .map_err(|e| ResumeJob(format_error(&bcm, e)))?;

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

        let bcm;
        get_job!(bcm, &guid, &self.job_name)
            .set_priority(priority)
            .map_err(|e| ApplySettings(format_error(&bcm, e)))?;

        Ok(())
    }

    fn get_monitor_control_sender(&mut self, guid: Guid) -> Option<Arc<ControlPair>> {
        if let hash_map::Entry::Occupied(occ) = self.monitors.entry(guid) {
            if let Some(sender) = occ.get().0.upgrade() {
                Some(sender)
            } else {
                // Remove dangling Weak
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

        let bcm;
        get_job!(bcm, &guid, &self.job_name)
            .complete()
            .map_err(|e| CompleteJob(format_error(&bcm, e)))?;

        let _ = self.stop_update(guid);

        Ok(())
    }

    pub fn cancel_job(&mut self, guid: Guid) -> Result<(), CancelJobFailure> {
        use CancelJobFailure::*;

        let bcm;
        get_job!(bcm, &guid, &self.job_name)
            .cancel()
            .map_err(|e| CancelJob(format_error(&bcm, e)))?;

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
    last_status_time: Option<Instant>,
    last_url: Option<String>,
}

impl InProcessMonitor {
    fn new(
        job: &mut BitsJob,
        interval_millis: u32,
    ) -> Result<(InProcessMonitor, InProcessMonitorControl), comedy::Error> {
        let guid = job.guid()?;

        let vars = Arc::new((
            Condvar::new(),
            Mutex::new(InProcessMonitorVars {
                interval_millis,
                notified: false,
                shutdown: false,
            }),
        ));

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
                    return Ok(());
                }
            }
            Err(E_FAIL)
        });

        job.register_callbacks(Some(transferred_cb), Some(error_cb), None)?;

        let control = InProcessMonitorControl(Arc::downgrade(&vars));

        let monitor = InProcessMonitor {
            guid,
            vars,
            last_status_time: None,
            last_url: None,
        };

        Ok((monitor, control))
    }

    fn job(&self) -> Result<BitsJob, Error> {
        Ok(BackgroundCopyManager::connect()?.get_job_by_guid(&self.guid)?)
    }

    pub fn get_status(&mut self, timeout_millis: u32) -> Result<JobStatus, Error> {
        let timeout = Duration::from_millis(timeout_millis as u64);

        let started = Instant::now();

        {
            let mut s = self.vars.1.lock().unwrap();
            loop {
                if s.shutdown {
                    // Disconnected, immediately return error.
                    return Err(Error::NotConnected);
                }

                if started.elapsed() > timeout {
                    // Timed out, disconnect and return timeout error.
                    // This should not happen normally with the in-process monitor, but the monitor
                    // interval could potentially be too long (for instance).
                    break Err(Error::Timeout);
                }

                // Get the interval every pass through the loop, in case it has changed.
                let interval = Duration::from_millis(s.interval_millis as u64);

                let wait_until = self.last_status_time.map(|last_status_time| {
                    cmp::min(last_status_time + interval, started + timeout)
                });

                let now = Instant::now();

                if s.notified || wait_until.is_none() || wait_until.unwrap() < now {
                    // Notified, first status report, or status report due,
                    // so exit loop to get status below.
                    s.notified = false;
                    break Ok(());
                }
                let wait_until = wait_until.unwrap();

                // Wait, repeat the checks above.
                s = self.vars.0.wait_timeout(s, wait_until - now).unwrap().0;
            }
        }
        .and_then(|()| {
            // No error yet, start getting status now.
            self.last_status_time = Some(Instant::now());
            self.job()
        })
        .and_then(|mut job| {
            // Got job successfully, get status.
            let status = job.get_status().map_err(|_| Error::NotConnected)?;
            let url = job.get_first_file()?.get_remote_name()?;

            Ok(JobStatus {
                state: BitsJobState::from(status.state),
                progress: status.progress,
                error_count: status.error_count,
                error: status.error.map(|e| JobError {
                    context: BitsErrorContext::from(e.context),
                    context_str: e.context_str,
                    error: HResultMessage {
                        hr: e.error,
                        message: e.error_str,
                    },
                }),
                times: status.times,
                url: if self.last_url.is_some() && *self.last_url.as_ref().unwrap() == url {
                    None
                } else {
                    self.last_url = Some(url);
                    self.last_url.clone()
                },
            })
        })
        .or_else(|e| {
            // On any error, disconnect.
            self.vars.1.lock().unwrap().shutdown = true;
            Err(e)
        })
    }
}

#[cfg(test)]
mod tests;
