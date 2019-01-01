use std::ffi::{OsStr, OsString};
use std::mem;
use std::result;

use bincode::{deserialize, serialize};
use bits::status::BitsJobStatus;
use failure::{bail, ensure, Error};
use named_pipe::{PipeAccess, PipeServer, WaitTimeout};

use bits_protocol::*;
use task;

// The IPC is structured so that the client runs as a named pipe server, accepting connections
// from the BITS task server once it starts up, which it then uses to issue commands.
// This is done so that the client can create the pipe and wait for a connection to know when the
// task is ready for commands; otherwise it would have to repeatedly try to connect until the
// server creates the pipe.

// TODO needs the rest of the args
pub fn connect(task_name: &OsStr) -> Result<PipeServer, Error> {
    let mut pipe = PipeServer::new_duplex(PipeAccess::LocalService)?;

    {
        // Start the task, which will connect back to the pipe for commands.
        let mut arg = OsString::from("command-connect ");
        arg.push(pipe.name());
        let _running_task = task::run_on_demand(task_name, arg.as_os_str())?;
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

    Ok(pipe)
}

pub fn run_command<'a, 'b, T>(
    pipe: &mut PipeServer,
    cmd: T,
    out_buf: &'b mut [u8],
) -> Result<result::Result<T::Success, T::Failure>, Error>
where
    T: CommandType<'a, 'b, 'b>,
{
    let cmd = serialize(&T::new(cmd)).unwrap();
    assert!(cmd.len() <= MAX_COMMAND);

    pipe.write(&cmd, WaitTimeout::infinite())?;
    let reply = pipe.read(out_buf, WaitTimeout::infinite())?;

    match deserialize(reply) {
        Err(e) => bail!("deserialize failed: {}", e),
        Ok(r) => Ok(r),
    }
}

pub fn get_status(
    monitor_pipe: &mut PipeServer,
    timeout: WaitTimeout,
) -> Result<BitsJobStatus, Error> {
    let mut out_buf: [u8; MAX_RESPONSE] = unsafe { mem::uninitialized() };
    Ok(deserialize(monitor_pipe.read(&mut out_buf, timeout)?)?)
}
