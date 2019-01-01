use std::ffi::{OsStr, OsString};
use std::mem;
use std::result::Result;
use std::thread;
use std::time::Duration;

use bincode::{deserialize, serialize};
use bits::{BackgroundCopyManager, BG_JOB_PRIORITY_FOREGROUND, BG_JOB_PRIORITY_NORMAL};
use comedy::com::InitCom;
use comedy::guid::Guid;
use failure::{bail, ensure, Error};
use named_pipe::{PipeClient, WaitTimeout};

use bits_protocol::*;

// TODO set priority on monitor start and end

// TODO needs args:
// - install path hash
// - local path prefix
// - log
pub fn run(args: &[OsString]) -> Result<(), Error> {
    if args[0] == "command-connect" && args.len() == 2 {
        let _inited = InitCom::init_sta().unwrap();

        run_commands(&args[1])
    } else {
        bail!("Bad command: {:?}", args)
    }
}

fn run_commands(pipe_name: &OsStr) -> Result<(), Error> {
    let mut control_pipe = PipeClient::open_duplex(pipe_name)?;

    // protocol version handshake
    let mut version = [0xFF; 1];
    let version = control_pipe.read(&mut version, WaitTimeout::infinite())?;
    ensure!(version.len() == 1, "wrong version length");
    ensure!(
        version[0] == PROTOCOL_VERSION,
        "protocol version {} != expected {}",
        version[0],
        PROTOCOL_VERSION
    );
    control_pipe.write(&[PROTOCOL_VERSION; 1], WaitTimeout::infinite())?;

    loop {
        let mut buf: [u8; MAX_COMMAND] = unsafe { mem::uninitialized() };
        // TODO better handling of errors, not really a disaster if the pipe closes, and
        // we may want to do something with ERROR_MORE_DATA
        let buf = control_pipe.read(&mut buf, WaitTimeout::infinite())?;

        // TODO setup logging
        let deserialized_command = deserialize(buf);
        let mut serialized_response = match deserialized_command {
            Err(_) => bail!("deserialize failed"),
            Ok(cmd) => match cmd {
                Command::StartJob(cmd) => serialize(&run_start(&cmd)),
                Command::MonitorJob(cmd) => serialize(&run_monitor(&cmd)),
                Command::ResumeJob(cmd) => serialize(&run_resume(&cmd)),
                Command::SetJobPriority(cmd) => serialize(&run_set_priority(&cmd)),
                Command::SetUpdateInterval(cmd) => serialize(&run_set_update_interval(&cmd)),
                Command::CompleteJob(cmd) => serialize(&run_complete(&cmd)),
                Command::CancelJob(cmd) => serialize(&run_cancel(&cmd)),
            },
        }
        .unwrap();
        assert!(serialized_response.len() <= MAX_RESPONSE);

        control_pipe.write(&mut serialized_response, WaitTimeout::infinite())?;
    }
}

fn run_start(cmd: &StartJobCommand) -> Result<StartJobSuccess, StartJobFailure> {
    // TODO: nicer way of bulk wrapping errors
    (move || {
        // TODO: determine name from install path hash
        // TODO: gotta capture, return, log errors
        let bcm = BackgroundCopyManager::connect()?;
        let mut job = bcm.create_job(&OsString::from("JOBBO"))?;
        job.add_file(&cmd.url, &cmd.save_path)?;
        job.resume()?;

        if let Some(ref monitor) = cmd.monitor {
            spawn_monitor(job.guid()?, monitor);
        }
        Ok(StartJobSuccess { guid: job.guid()? })
    })()
    .map_err(|e: Error| StartJobFailure::Other(format!("{}", e))) // TODO use the other errors
}

fn run_monitor(cmd: &MonitorJobCommand) -> Result<(), MonitorJobFailure> {
    (move || {
        let bcm = BackgroundCopyManager::connect()?;
        // TODO: return Err on None instead of unwrap, same with all below
        let job = bcm.find_job_by_guid(&cmd.guid)?.unwrap();

        // TODO: check name
        spawn_monitor(job.guid()?, &cmd.monitor);
        Ok(())
    })()
    .map_err(|e: Error| MonitorJobFailure::Other(format!("{}", e)))
}

fn spawn_monitor(
    guid: Guid,
    MonitorConfig {
        pipe_name,
        interval_millis,
    }: &MonitorConfig,
) {
    let interval_millis = *interval_millis;
    let pipe_name = pipe_name.clone();
    thread::spawn(move || {
        let result = std::panic::catch_unwind(|| {
            use std::sync::mpsc::channel;
            let (tx, rx) = channel();

            // TODO none of this stuff (except serialize) should be `unwrap`
            let _inited = InitCom::init_mta().unwrap();
            let mut job = {
                BackgroundCopyManager::connect()
                    .unwrap()
                    .find_job_by_guid(&guid)
                    .unwrap()
                    .unwrap()
            };
            let mut pipe = PipeClient::open_outbound(&pipe_name).unwrap();
            let delay = Duration::from_millis(interval_millis as u64);

            let tx_mutex = std::sync::Mutex::new(tx);
            job.register_callbacks(
                Some(Box::new(move |mut _job| {
                    /*
                    // TODO need to report the outcome of complete somehow, or just don't do it
                    // TODO complete can succeed with partial completion, fail on that
                    job.complete().expect("complete failed?!");
                    */

                    let tx = tx_mutex.lock().unwrap().clone();

                    #[allow(unused_must_use)]
                    {
                        tx.send(());
                    }
                })),
                None,
                None,
            )
            .unwrap();

            loop {
                let status = job.get_status().unwrap();
                pipe.write(&mut serialize(&status).unwrap(), WaitTimeout::infinite())
                    .unwrap();
                {
                    let _ = rx.recv_timeout(delay);
                }
            }
        });
        if let Err(e) = result {
            use std::io::Write;
            std::fs::File::create("C:\\ProgramData\\monitorfail.log")
                .unwrap()
                .write(format!("{:?}", e.downcast_ref::<String>()).as_bytes())
                .unwrap();
        }
    });
}

fn run_resume(cmd: &ResumeJobCommand) -> Result<(), ResumeJobFailure> {
    (move || {
        let bcm = BackgroundCopyManager::connect()?;
        let mut job = bcm.find_job_by_guid(&cmd.guid)?.unwrap();

        job.resume()?;

        Ok(())
    })()
    .map_err(|e: Error| ResumeJobFailure::Other(format!("{}", e)))
}

fn run_set_priority(cmd: &SetJobPriorityCommand) -> Result<(), SetJobPriorityFailure> {
    (move || {
        let bcm = BackgroundCopyManager::connect()?;
        let mut job = bcm.find_job_by_guid(&cmd.guid)?.unwrap();

        job.set_priority(if cmd.foreground {
            BG_JOB_PRIORITY_FOREGROUND
        } else {
            BG_JOB_PRIORITY_NORMAL
        })?;

        Ok(())
    })()
    .map_err(|e: Error| SetJobPriorityFailure::Other(format!("{}", e)))
}

fn run_set_update_interval(cmd: &SetUpdateIntervalCommand) -> Result<(), SetUpdateIntervalFailure> {
    (move || {
        let bcm = BackgroundCopyManager::connect()?;
        let _job = bcm.find_job_by_guid(&cmd.guid)?.unwrap();

        // TODO: implement when there is a monitor registry available
        Ok(())
    })()
    .map_err(|e: Error| SetUpdateIntervalFailure::Other(format!("{}", e)))
}

fn run_complete(cmd: &CompleteJobCommand) -> Result<(), CompleteJobFailure> {
    (move || {
        let bcm = BackgroundCopyManager::connect()?;
        let mut job = bcm.find_job_by_guid(&cmd.guid)?.unwrap();
        job.complete()?;

        Ok(())
    })()
    .map_err(|e: Error| CompleteJobFailure::Other(format!("{}", e)))
}

fn run_cancel(cmd: &CancelJobCommand) -> Result<(), CancelJobFailure> {
    (move || {
        let bcm = BackgroundCopyManager::connect()?;
        let mut job = bcm.find_job_by_guid(&cmd.guid)?.unwrap(); // TODO error on not found
        job.cancel()?;

        Ok(())
    })()
    .map_err(|e: Error| CancelJobFailure::Other(format!("{}", e)))
}
