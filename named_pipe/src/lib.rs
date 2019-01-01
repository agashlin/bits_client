extern crate comedy;
extern crate failure;
extern crate failure_derive;
extern crate winapi;
extern crate wio;

mod overlapped;

use std::convert;
use std::ffi::{CString, OsStr, OsString};
use std::mem;
use std::ptr;
use std::rc::Rc;
use std::result;

use comedy::handle::{HLocal, Handle};
use comedy::{call_handle_getter, check_true};

use failure::Fail;

use winapi::shared::minwindef::{DWORD, FALSE};
use winapi::shared::sddl::{ConvertStringSecurityDescriptorToSecurityDescriptorA, SDDL_REVISION_1};
use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
use winapi::um::minwinbase::SECURITY_ATTRIBUTES;
use winapi::um::namedpipeapi::{CreateNamedPipeW, DisconnectNamedPipe, SetNamedPipeHandleState};
use winapi::um::winbase::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, PIPE_ACCESS_INBOUND,
    PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};
use winapi::um::winnt::{FILE_READ_ATTRIBUTES, GENERIC_READ, GENERIC_WRITE};
use wio::wide::ToWide;

// TODO there's some ugliness around whether a bytes_transferred comes back following an operation,
//      maybe there need to be different types for different finishes

use overlapped::{Overlapped, OverlappedFinished};

pub use overlapped::WaitTimeout;

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum PipeError {
    #[fail(display = "Pipe is not connected")]
    NotConnected,
    #[fail(display = "Operation timed out")]
    Timeout,
    #[fail(display = "Should have written {} bytes, wrote {}", _0, _1)]
    WriteCount(usize, u32),
    #[fail(display = "Windows API error")]
    Api(#[fail(cause)] comedy::Error),
}

impl convert::From<comedy::Error> for PipeError {
    fn from(err: comedy::Error) -> PipeError {
        PipeError::Api(err)
    }
}

type Result<T> = result::Result<T, PipeError>;

pub fn format_local_pipe_path(name: &OsStr) -> OsString {
    let mut path = OsString::from(r"\\.\pipe\");
    path.push(name);
    return path;
}

#[derive(Debug)]
pub struct PipeServer {
    name: OsString,
    pipe: Rc<Handle>,
    ovl: Option<Box<Overlapped>>,
}

#[derive(Debug)]
pub enum PipeAccess {
    /// Default access control
    Default,
    /// Only allow access from LocalService
    LocalService,
}

impl PipeServer {
    /// Create a duplex, unique, asynchronous, message-mode pipe for local machine use.
    pub fn new_duplex(access: PipeAccess) -> Result<Self> {
        let (name, pipe) = new_pipe_impl(PipeServerDirection::Duplex, access)?;
        Ok(PipeServer {
            name,
            pipe: Rc::new(pipe),
            ovl: None,
        })
    }

    /// Create an inbound, unique, asynchronous, message-mode pipe for local machine use.
    pub fn new_inbound(access: PipeAccess) -> Result<Self> {
        let (name, pipe) = new_pipe_impl(PipeServerDirection::Inbound, access)?;
        Ok(PipeServer {
            name,
            pipe: Rc::new(pipe),
            ovl: None,
        })
    }

    pub fn is_connected(&self) -> bool {
        self.ovl.is_some()
    }

    pub fn name(&self) -> &OsStr {
        &self.name
    }

    pub fn connect(&mut self, timeout: WaitTimeout) -> Result<()> {
        let _ = self.disconnect();
        let ovl = Overlapped::new(self.pipe.clone())?;
        self.ovl = Some(connect_pipe_impl(ovl, timeout)?);
        Ok(())
    }

    pub fn disconnect(&mut self) -> Result<()> {
        if !self.is_connected() {
            return Ok(());
        }
        self.ovl = None;

        disconnect_pipe_impl(&*self.pipe)?;
        Ok(())
    }

    pub fn read<'a>(
        &mut self,
        out_buffer: &'a mut [u8],
        timeout: WaitTimeout,
    ) -> Result<&'a mut [u8]> {
        let ovl = self.ovl.take().ok_or(PipeError::NotConnected)?;

        match read_pipe_impl(ovl, out_buffer, timeout) {
            Ok((ovl, buf)) => {
                self.ovl = Some(ovl);
                Ok(buf)
            }
            Err(e) => {
                let _ = self.disconnect()?;
                Err(e)
            }
        }
    }

    pub fn write(&mut self, in_buffer: &[u8], timeout: WaitTimeout) -> Result<()> {
        let ovl = self.ovl.take().ok_or(PipeError::NotConnected)?;

        match write_pipe_impl(ovl, in_buffer, timeout) {
            Ok(ovl) => {
                self.ovl = Some(ovl);
                Ok(())
            }
            Err(e) => {
                let _ = self.disconnect();
                Err(e)
            }
        }
    }

    // TODO support reporting client pid
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        // TODO: FlushFileBuffers?
        let _result = self.disconnect();
    }
}

#[derive(Debug)]
pub struct PipeClient {
    ovl: Option<Box<Overlapped>>,
}

impl PipeClient {
    pub fn open_duplex(name: &OsStr) -> Result<Self> {
        let pipe = open_pipe_impl(name, GENERIC_READ | GENERIC_WRITE)?;

        let mut mode = PIPE_READMODE_MESSAGE;
        unsafe {
            check_true!(SetNamedPipeHandleState(
                *pipe,
                &mut mode,
                ptr::null_mut(), // lpMaxCollectionCount
                ptr::null_mut(), // lpCollectDataTimeout
            ))
        }?;

        Ok(PipeClient {
            ovl: Some(Overlapped::new(Rc::new(pipe))?),
        })
    }

    pub fn open_outbound(name: &OsStr) -> Result<Self> {
        let pipe = open_pipe_impl(name, GENERIC_WRITE)?;

        Ok(PipeClient {
            ovl: Some(Overlapped::new(Rc::new(pipe))?),
        })
    }

    pub fn read<'a>(
        &mut self,
        out_buffer: &'a mut [u8],
        timeout: WaitTimeout,
    ) -> Result<&'a mut [u8]> {
        let ovl = self.ovl.take().ok_or(PipeError::NotConnected)?;

        match read_pipe_impl(ovl, out_buffer, timeout) {
            Ok((ovl, buf)) => {
                self.ovl = Some(ovl);
                Ok(buf)
            }
            Err(e) => Err(e),
        }
    }

    pub fn write(&mut self, in_buffer: &[u8], timeout: WaitTimeout) -> Result<()> {
        let ovl = self.ovl.take().ok_or(PipeError::NotConnected)?;

        match write_pipe_impl(ovl, in_buffer, timeout) {
            Ok(ovl) => {
                self.ovl = Some(ovl);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

enum PipeServerDirection {
    Duplex,
    Inbound,
}

fn new_pipe_impl(
    direction: PipeServerDirection,
    access: PipeAccess,
) -> result::Result<(OsString, Handle), comedy::Error> {
    // Create a random 32 character name from the hex of a 128-bit random uint.
    let pipe_name = OsString::from(format!("{:032x}", rand::random::<u128>()));
    let pipe_path = format_local_pipe_path(&pipe_name).to_wide_null();

    // TODO: are these sizes appropriate?
    // Buffer sizes
    let out_buffer_size = match direction {
        PipeServerDirection::Duplex => 0x10000,
        PipeServerDirection::Inbound => 0,
    };
    let in_buffer_size = 0x10000;

    // Open mode
    let open_mode = match direction {
        PipeServerDirection::Duplex => PIPE_ACCESS_DUPLEX,
        PipeServerDirection::Inbound => PIPE_ACCESS_INBOUND,
    } | FILE_FLAG_FIRST_PIPE_INSTANCE
        | FILE_FLAG_OVERLAPPED;

    // Pipe mode
    let pipe_mode =
        PIPE_WAIT | PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS;

    // Build security attributes to allow Local Service access.
    let sa = match access {
        PipeAccess::LocalService => {
            let sddl = match direction {
                PipeServerDirection::Duplex => {
                    // Allow read/write access by Local Service.
                    CString::new("D:(A;;GRGW;;;LS)")
                }
                PipeServerDirection::Inbound => {
                    // Allow write access by Local Service (also need to be able to read attributes).
                    CString::new(format!(
                        "D:(A;;{:#010x};;;LS)",
                        GENERIC_WRITE | FILE_READ_ATTRIBUTES
                    ))
                }
            }
            .unwrap();

            Some(SecurityAttributes::new(sddl).unwrap())
        }
        PipeAccess::Default => None,
    };

    Ok((pipe_name, unsafe {
        call_handle_getter!(CreateNamedPipeW(
            pipe_path.as_ptr(),
            open_mode,
            pipe_mode,
            1, // nMaxInstances
            out_buffer_size,
            in_buffer_size,
            0, // nDefaultTimeOut (0 means 50ms default for WaitNamedPipe)
            if let Some(mut sa) = sa {
                &mut sa.sa
            } else {
                ptr::null_mut()
            },
        ))
    }?))
}

struct SecurityAttributes {
    _psd: HLocal,
    pub sa: SECURITY_ATTRIBUTES,
}

impl SecurityAttributes {
    fn new(sddl: CString) -> Result<SecurityAttributes> {
        let mut raw_psd = ptr::null_mut();
        let psd = unsafe {
            check_true!(ConvertStringSecurityDescriptorToSecurityDescriptorA(
                sddl.to_bytes_with_nul().as_ptr() as *const i8,
                SDDL_REVISION_1 as DWORD,
                &mut raw_psd,
                ptr::null_mut(),
            ))?;
            HLocal::wrap(raw_psd).unwrap()
        };

        Ok(SecurityAttributes {
            _psd: psd,
            sa: SECURITY_ATTRIBUTES {
                nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as DWORD,
                lpSecurityDescriptor: raw_psd,
                bInheritHandle: FALSE,
            },
        })
    }
}

fn connect_pipe_impl(ovl: Box<Overlapped>, timeout: WaitTimeout) -> Result<Box<Overlapped>> {
    match Overlapped::connect_named_pipe(ovl).finish(timeout)? {
        None => Err(PipeError::Timeout),
        Some(OverlappedFinished { ovl, result, .. }) => match result {
            Ok(_) => Ok(ovl),
            Err(e) => Err(PipeError::Api(e)),
        },
    }
}

fn disconnect_pipe_impl(handle: &Handle) -> Result<()> {
    unsafe { check_true!(DisconnectNamedPipe(**handle)) }?;

    Ok(())
}

fn open_pipe_impl(name: &OsStr, desired_access: DWORD) -> result::Result<Handle, comedy::Error> {
    let pipe_path = format_local_pipe_path(name).to_wide_null();

    unsafe {
        call_handle_getter!(CreateFileW(
            pipe_path.as_ptr(),
            desired_access,
            0,               // dwShareMode
            ptr::null_mut(), // lpSecurityAttributes
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            ptr::null_mut(), // hTemplateFile
        ))
    }
}

fn write_pipe_impl(
    ovl: Box<Overlapped>,
    in_buffer: &[u8],
    timeout: WaitTimeout,
) -> Result<Box<Overlapped>> {
    match Overlapped::write_file(ovl, in_buffer).finish(timeout)? {
        None => Err(PipeError::Timeout),
        Some(OverlappedFinished {
            ovl,
            bytes_transferred,
            result,
        }) => match result {
            Ok(_) => {
                if bytes_transferred.unwrap() != in_buffer.len() as DWORD {
                    Err(PipeError::WriteCount(
                        in_buffer.len(),
                        bytes_transferred.unwrap(),
                    ))
                } else {
                    Ok(ovl)
                }
            }
            Err(e) => Err(PipeError::Api(e)),
        },
    }
}

fn read_pipe_impl<'a>(
    ovl: Box<Overlapped>,
    out_buffer: &'a mut [u8],
    timeout: WaitTimeout,
) -> Result<(Box<Overlapped>, &'a mut [u8])> {
    let OverlappedFinished {
        ovl,
        bytes_transferred,
        result,
    } = match Overlapped::read_file(ovl, out_buffer).finish(timeout)? {
        None => return Err(PipeError::Timeout),
        Some(finished) => finished,
    };

    match result {
        Ok(_) => Ok((ovl, &mut out_buffer[..bytes_transferred.unwrap() as usize])),
        Err(e) => Err(PipeError::Api(e)),
    }
}

#[cfg(test)]
mod test_mod {
    use std::thread::{sleep, spawn};
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn test_inbound_no_timeout() {
        let mut server = PipeServer::new_inbound(PipeAccess::Default).unwrap();
        let name = OsString::from(server.name());

        let client_thread = spawn(move || {
            let mut client = PipeClient::open_outbound(&name).unwrap();
            let buf = [0x80; 50];
            client.write(&buf, WaitTimeout::infinite()).unwrap();
            //client.flush().unwrap();
        });

        let mut buf = [0xff; 100];
        let err = server.read(&mut buf, WaitTimeout::infinite()).unwrap_err();
        assert_eq!(err, PipeError::NotConnected);

        server.connect(WaitTimeout::infinite()).unwrap();

        sleep(Duration::from_millis(200));

        let buf = server.read(&mut buf, WaitTimeout::infinite()).unwrap();

        assert_eq!(buf.len(), 50);
        assert!(buf.iter().all(|x| *x == 0x80));

        client_thread.join().unwrap();
        ()
    }

    #[test]
    fn test_inbound_timeout() {
        let mut server = PipeServer::new_inbound(PipeAccess::Default).unwrap();
        let name = OsString::from(server.name());

        let client_thread = spawn(move || {
            let mut client = PipeClient::open_outbound(&name).unwrap();
            sleep(Duration::from_millis(1000));
            let buf = [0x80; 50];
            client.write(&buf, WaitTimeout::infinite()).unwrap();
            //client.flush().unwrap();
        });

        server.connect(WaitTimeout::infinite()).unwrap();

        let mut buf = [0xff; 100];
        let t_start = Instant::now();
        let err = server
            .read(&mut buf, WaitTimeout::from_millis(100).unwrap())
            .unwrap_err();
        let t_end = Instant::now();

        assert_eq!(err, PipeError::Timeout);

        let err = server
            .read(&mut buf, WaitTimeout::from_millis(100).unwrap())
            .unwrap_err();

        assert_eq!(err, PipeError::NotConnected);

        client_thread.join().unwrap();

        let dur = t_end - t_start;
        assert!(
            dur < Duration::from_millis(500),
            "timed out in {}, expected < 500ms",
            dur.as_secs() * 1000 + dur.subsec_millis() as u64
        );

        assert!(buf.iter().all(|x| *x == 0xff));
    }

    #[test]
    fn test_inbound_delay() {
        let mut server = PipeServer::new_inbound(PipeAccess::Default).unwrap();
        let name = OsString::from(server.name());

        let client_thread = spawn(move || {
            let mut client = PipeClient::open_outbound(&name).unwrap();

            sleep(Duration::from_millis(100));

            let buf = [0x80; 50];
            client.write(&buf, WaitTimeout::infinite()).unwrap();

            sleep(Duration::from_millis(100));

            let buf = [0x81; 75];
            client.write(&buf, WaitTimeout::infinite()).unwrap();
        });

        server.connect(WaitTimeout::infinite()).unwrap();

        let mut buf = [0xff; 100];
        let buf = server
            .read(&mut buf, WaitTimeout::from_millis(200).unwrap())
            .unwrap();

        assert_eq!(buf.len(), 50);
        assert!(buf.iter().all(|x| *x == 0x80));

        let mut buf = [0xff; 100];
        let buf = server
            .read(&mut buf, WaitTimeout::from_millis(200).unwrap())
            .unwrap();

        assert_eq!(buf.len(), 75);
        assert!(buf.iter().all(|x| *x == 0x81));

        client_thread.join().unwrap();
        ()
    }

    #[test]
    fn test_inbound_connect_timeout() {
        let mut server = PipeServer::new_inbound(PipeAccess::Default).unwrap();
        let err = server
            .connect(WaitTimeout::from_millis(250).unwrap())
            .unwrap_err();

        assert_eq!(err, PipeError::Timeout);
    }

    #[test]
    fn test_inbound_access_denied() {
        use comedy::error::ErrorCode;
        use winapi::shared::winerror::ERROR_ACCESS_DENIED;

        let server = PipeServer::new_inbound(PipeAccess::LocalService).unwrap();

        let client_err = PipeClient::open_outbound(server.name()).unwrap_err();
        assert_eq!(
            client_err
                .cause()
                .unwrap()
                .downcast_ref::<comedy::Error>()
                .unwrap()
                .code,
            Some(ErrorCode::LastError(ERROR_ACCESS_DENIED))
        );
    }

    // TODO test pipe close
    // TODO test pipe close while still writing
    // TODO impl and test flush
}
