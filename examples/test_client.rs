extern crate bits;
extern crate comedy;
extern crate failure;
extern crate named_pipe;
extern crate update_agent;

use std::env;
use std::ffi::{OsStr, OsString};
use std::mem;
use std::process;
use std::str::FromStr;

use bits::{BG_JOB_STATE_CONNECTING, BG_JOB_STATE_TRANSFERRING, BG_JOB_STATE_TRANSIENT_ERROR};
use comedy::guid::Guid;
use failure::{bail, Error};
use named_pipe::{PipeAccess, PipeServer, WaitTimeout};

use update_agent::bits_client;
use update_agent::bits_protocol::*;
use update_agent::task;

type Result = std::result::Result<(), Error>;

pub fn main() {
    if let Err(err) = entry() {
        eprintln!("{}", err);
        for cause in err.iter_causes() {
            eprintln!("caused by {}", cause);
        }

        process::exit(1);
    } else {
        println!("OK");
    }
}

const EXE_NAME: &'static str = "test_client";

fn usage() -> String {
    format!(
        concat!(
            "Usage {0} <command> [args...]\n",
            "Commands:\n",
            "  bits-start <URL> <local file>\n",
            "  bits-monitor <GUID> <millseconds delay>\n",
            "  bits-bg <GUID>\n",
            "  bits-fg <GUID>\n",
            "  bits-resume <GUID>\n",
            "  bits-complete <GUID>\n",
            "  bits-cancel <GUID>\n"
        ),
        EXE_NAME
    )
}

fn entry() -> Result {
    let args: Vec<_> = env::args_os().collect();

    if args.len() < 2 {
        eprintln!("{}", usage());
        bail!("not enough arguments");
    }

    let cmd = &*args[1].to_string_lossy();
    let cmd_args = &args[2..];

    match cmd {
        // command line client for testing
        "bits-start" if cmd_args.len() == 2 => {
            bits_start(&task::task_name(), cmd_args[0].clone(), cmd_args[1].clone())
        }
        "bits-monitor" if cmd_args.len() == 1 => bits_monitor(&task::task_name(), &cmd_args[0]),
        // TODO: some way of testing set update interval
        "bits-bg" if cmd_args.len() == 1 => bits_bg(&task::task_name(), &cmd_args[0]),
        "bits-fg" if cmd_args.len() == 1 => bits_fg(&task::task_name(), &cmd_args[0]),
        "bits-resume" if cmd_args.len() == 1 => bits_resume(&task::task_name(), &cmd_args[0]),
        "bits-complete" if cmd_args.len() == 1 => bits_complete(&task::task_name(), &cmd_args[0]),
        "bits-cancel" if cmd_args.len() == 1 => bits_cancel(&task::task_name(), &cmd_args[0]),

        _ => {
            eprintln!("{}", usage());
            bail!("usage error");
        }
    }
}

fn bits_start(task_name: &OsStr, url: OsString, save_path: OsString) -> Result {
    let mut command_pipe = bits_client::connect(task_name)?;

    let monitor_pipe = PipeServer::new_inbound(PipeAccess::LocalService)?;

    let command = StartJobCommand {
        url,
        save_path,
        monitor: Some(MonitorConfig {
            pipe_name: monitor_pipe.name().to_os_string(),
            interval_millis: 1000,
        }),
    };

    let mut out_buf: [u8; MAX_RESPONSE] = unsafe { mem::uninitialized() };
    let result = bits_client::run_command(&mut command_pipe, command, &mut out_buf)?;

    match result {
        Ok(r) => {
            println!("start success, guid = {}", r.guid);
            monitor_loop(monitor_pipe, 1000)?;
            Ok(())
        }
        Err(e) => bail!("error from server {:?}", e),
    }
}

fn bits_monitor(task_name: &OsStr, guid: &OsStr) -> Result {
    let guid = Guid::from_str(&guid.to_string_lossy())?;

    let mut command_pipe = bits_client::connect(task_name)?;

    let monitor_pipe = PipeServer::new_inbound(PipeAccess::LocalService)?;

    let command = MonitorJobCommand {
        guid,
        monitor: MonitorConfig {
            pipe_name: monitor_pipe.name().to_os_string(),
            interval_millis: 1000,
        },
    };

    let mut out_buf: [u8; MAX_RESPONSE] = unsafe { mem::uninitialized() };
    let result = bits_client::run_command(&mut command_pipe, command, &mut out_buf)?;

    match result {
        Ok(_) => {
            println!("monitor success");
            monitor_loop(monitor_pipe, 1000)?;
            Ok(())
        }
        Err(e) => bail!("error from server {:?}", e),
    }
}

fn monitor_loop(mut monitor_pipe: PipeServer, wait_millis: u32) -> Result {
    monitor_pipe.connect(WaitTimeout::infinite())?;

    println!("connected to monitor pipe");

    loop {
        let status = bits_client::get_status(
            &mut monitor_pipe,
            WaitTimeout::from_millis(wait_millis * 10).unwrap(),
        )?;

        println!("{:?}", status);

        if !(status.state == BG_JOB_STATE_CONNECTING
            || status.state == BG_JOB_STATE_TRANSFERRING
            || status.state == BG_JOB_STATE_TRANSIENT_ERROR)
        {
            break;
        }
    }
    Ok(())
}

fn bits_bg(task_name: &OsStr, guid: &OsStr) -> Result {
    bits_set_priority(false, task_name, guid)
}

fn bits_fg(task_name: &OsStr, guid: &OsStr) -> Result {
    bits_set_priority(true, task_name, guid)
}

fn bits_set_priority(foreground: bool, task_name: &OsStr, guid: &OsStr) -> Result {
    let guid = Guid::from_str(&guid.to_string_lossy())?;

    let mut command_pipe = bits_client::connect(task_name)?;

    let command = SetJobPriorityCommand { guid, foreground };
    let mut out_buf: [u8; MAX_RESPONSE] = unsafe { mem::uninitialized() };
    match bits_client::run_command(&mut command_pipe, command, &mut out_buf)? {
        Ok(_) => Ok(()),
        Err(e) => bail!("error from server {:?}", e),
    }
}

fn bits_resume(task_name: &OsStr, guid: &OsStr) -> Result {
    let guid = Guid::from_str(&guid.to_string_lossy())?;

    let mut command_pipe = bits_client::connect(task_name)?;

    let command = ResumeJobCommand { guid };
    let mut out_buf: [u8; MAX_RESPONSE] = unsafe { mem::uninitialized() };
    match bits_client::run_command(&mut command_pipe, command, &mut out_buf)? {
        Ok(_) => Ok(()),
        Err(e) => bail!("error from server {:?}", e),
    }
}

fn bits_complete(task_name: &OsStr, guid: &OsStr) -> Result {
    let guid = Guid::from_str(&guid.to_string_lossy())?;

    let mut command_pipe = bits_client::connect(task_name)?;

    let command = CompleteJobCommand { guid };
    let mut out_buf: [u8; MAX_RESPONSE] = unsafe { mem::uninitialized() };
    match bits_client::run_command(&mut command_pipe, command, &mut out_buf)? {
        Ok(_) => Ok(()),
        Err(e) => bail!("error from server {:?}", e),
    }
}

fn bits_cancel(task_name: &OsStr, guid: &OsStr) -> Result {
    let guid = Guid::from_str(&guid.to_string_lossy())?;

    let mut command_pipe = bits_client::connect(task_name)?;

    let command = CancelJobCommand { guid };
    let mut out_buf: [u8; MAX_RESPONSE] = unsafe { mem::uninitialized() };

    match bits_client::run_command(&mut command_pipe, command, &mut out_buf)? {
        Ok(_) => Ok(()),
        Err(e) => bail!("error from server {:?}", e),
    }
}
