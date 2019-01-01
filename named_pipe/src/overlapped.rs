use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::marker::PhantomData;
use std::mem;
use std::ptr;
use std::rc::Rc;
use std::result::Result;

use comedy::call_nonnull_handle_getter;
use comedy::error::{ErrorCode, FileLine};
use comedy::handle::Handle;

use winapi::shared::minwindef::{BOOL, DWORD, FALSE, TRUE};
use winapi::shared::winerror::{
    ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, WAIT_TIMEOUT,
};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::fileapi::{ReadFile, WriteFile};
use winapi::um::ioapiset::{CancelIoEx, GetOverlappedResult};
use winapi::um::minwinbase::OVERLAPPED;
use winapi::um::namedpipeapi::ConnectNamedPipe;
use winapi::um::synchapi::{CreateEventW, WaitForSingleObject};
use winapi::um::winbase::{INFINITE, WAIT_FAILED, WAIT_OBJECT_0};

#[derive(Debug)]
enum WaitTimeoutInner {
    Milliseconds(u32),
    Infinite,
}

#[derive(Debug)]
pub struct WaitTimeout(WaitTimeoutInner);

impl WaitTimeout {
    /// `millis` must not equal `INFINITE` (`0xFFFF_FFFF`)
    pub fn from_millis(millis: u32) -> Result<WaitTimeout, ()> {
        if millis == INFINITE {
            Err(())
        } else {
            Ok(WaitTimeout(WaitTimeoutInner::Milliseconds(millis)))
        }
    }

    pub fn infinite() -> WaitTimeout {
        WaitTimeout(WaitTimeoutInner::Infinite)
    }
}

#[derive(Debug)]
pub struct Event(Handle);

pub enum WaitResult {
    Signalled,
    Timeout,
}

impl Event {
    pub fn new() -> Result<Event, comedy::Error> {
        Ok(Event(unsafe {
            call_nonnull_handle_getter!(CreateEventW(
                ptr::null_mut(), // lpEventAttributes (cannot be inherited)
                FALSE,           // bManualReset
                FALSE,           // bInitialState
                ptr::null_mut(), // lpName (no name)
            ))
        }?))
    }

    pub fn wait(&self, timeout_millis: WaitTimeout) -> Result<WaitResult, comedy::Error> {
        let result = unsafe {
            WaitForSingleObject(
                *self.0,
                match timeout_millis.0 {
                    WaitTimeoutInner::Milliseconds(ms) => ms,
                    WaitTimeoutInner::Infinite => INFINITE,
                },
            )
        };

        match result {
            WAIT_OBJECT_0 => Ok(WaitResult::Signalled),
            WAIT_TIMEOUT => Ok(WaitResult::Timeout),
            WAIT_FAILED | _ => Err(comedy::Error {
                code: Some(ErrorCode::LastError(unsafe { GetLastError() })),
                function: Some("WaitForSingleObject"),
                file_line: Some(FileLine(file!(), line!())),
            }),
        }
    }
}

pub struct Overlapped {
    ovl: OVERLAPPED,
    event: Event,
    file: Rc<Handle>,
}

impl Debug for Overlapped {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(
            f,
            "Overlapped {{ ovl: OVERLAPPED {{ Internal: {:#x}, InternalHigh: {:#x}, \
             u: {{ s: {{ Offset: {:#x}, OffsetHigh: {:#x} }}, Pointer: {:?} }}, \
             hEvent: {:?} }}, event: {:?}, file: {:?} }}",
            self.ovl.Internal,
            self.ovl.InternalHigh,
            unsafe { self.ovl.u.s().Offset },
            unsafe { self.ovl.u.s().OffsetHigh },
            unsafe { self.ovl.u.Pointer() },
            self.ovl.hEvent,
            &self.event,
            &self.file,
        )
    }
}

pub enum OverlappedResult<'a, 'b> {
    Pending(OverlappedPending<'a, 'b>),
    Finished(OverlappedFinished),
}

pub struct OverlappedPending<'a, 'b> {
    ovl: Option<Box<Overlapped>>,
    _write_buffer: Option<PhantomData<&'a [u8]>>,
    _read_buffer: Option<PhantomData<&'b mut [u8]>>,
}

pub struct OverlappedFinished {
    pub ovl: Box<Overlapped>,
    pub bytes_transferred: Option<DWORD>,
    pub result: Result<BOOL, comedy::Error>,
}

impl Overlapped {
    pub fn new(file: Rc<Handle>) -> Result<Box<Overlapped>, comedy::Error> {
        let event = Event::new()?;

        let mut ovl = Overlapped {
            ovl: unsafe { mem::zeroed() },
            event,
            file,
        };

        ovl.ovl.hEvent = *ovl.event.0;

        Ok(Box::new(ovl))
    }

    pub fn connect_named_pipe(mut ovl: Box<Overlapped>) -> OverlappedResult<'static, 'static> {
        let result = unsafe { ConnectNamedPipe(**ovl.file, &mut ovl.ovl) };

        let last_error = unsafe { GetLastError() };

        if result == 0 && last_error == ERROR_IO_PENDING {
            OverlappedResult::Pending(OverlappedPending {
                ovl: Some(ovl),
                _write_buffer: None,
                _read_buffer: None,
            })
        } else {
            let result = if last_error == ERROR_PIPE_CONNECTED {
                TRUE
            } else {
                result
            };

            OverlappedResult::Finished(OverlappedFinished::new(
                Some("ConnectNamedPipe"),
                Some(FileLine(file!(), line!())),
                ovl,
                0,
                result,
                last_error,
            ))
        }
    }

    pub fn write_file<'a>(
        mut ovl: Box<Overlapped>,
        buffer: &'a [u8],
    ) -> OverlappedResult<'a, 'static> {
        assert!(buffer.len() as DWORD as usize == buffer.len());

        let mut bytes_transferred = 0;
        let result = unsafe {
            WriteFile(
                **ovl.file,
                buffer.as_ptr() as *const _,
                buffer.len() as DWORD,
                &mut bytes_transferred,
                &mut ovl.ovl,
            )
        };

        let last_error = unsafe { GetLastError() };

        if result == 0 && last_error == ERROR_IO_PENDING {
            OverlappedResult::Pending(OverlappedPending {
                ovl: Some(ovl),
                _write_buffer: Some(PhantomData),
                _read_buffer: None,
            })
        } else {
            // TODO does this immediate result ever happens with an overlapped op?
            OverlappedResult::Finished(OverlappedFinished::new(
                Some("WriteFile"),
                Some(FileLine(file!(), line!())),
                ovl,
                bytes_transferred,
                result,
                last_error,
            ))
        }
    }

    pub fn read_file<'b>(
        mut ovl: Box<Overlapped>,
        buffer: &'b mut [u8],
    ) -> OverlappedResult<'static, 'b> {
        assert!(buffer.len() as DWORD as usize == buffer.len());

        let mut bytes_transferred = 0;
        let result = unsafe {
            ReadFile(
                **ovl.file,
                buffer.as_mut_ptr() as *mut _,
                buffer.len() as DWORD,
                &mut bytes_transferred,
                &mut ovl.ovl,
            )
        };

        let last_error = unsafe { GetLastError() };

        if result == 0 && last_error == ERROR_IO_PENDING {
            OverlappedResult::Pending(OverlappedPending {
                ovl: Some(ovl),
                _write_buffer: None,
                _read_buffer: Some(PhantomData),
            })
        } else {
            OverlappedResult::Finished(OverlappedFinished::new(
                Some("ReadFile"),
                Some(FileLine(file!(), line!())),
                ovl,
                bytes_transferred,
                result,
                last_error,
            ))
        }
    }
}

impl<'a, 'b> OverlappedPending<'a, 'b> {
    fn wait(&mut self, timeout: WaitTimeout) -> Result<WaitResult, comedy::Error> {
        if let Some(ovl) = self.ovl.as_ref() {
            ovl.event.wait(timeout)
        } else {
            Ok(WaitResult::Signalled)
        }
    }

    /// Will panic if get_result had previously returned OverlappedResult::Finished
    fn get_result(mut self) -> OverlappedResult<'a, 'b> {
        let mut bytes_transferred = 0;

        let result = {
            let ovl = self.ovl.as_mut().unwrap();

            unsafe {
                GetOverlappedResult(
                    **ovl.file,
                    &mut ovl.ovl,
                    &mut bytes_transferred,
                    FALSE, // bWait
                )
            }
        };

        let last_error = unsafe { GetLastError() };

        if result == 0 && last_error == ERROR_IO_INCOMPLETE {
            OverlappedResult::Pending(self)
        } else {
            OverlappedResult::Finished(OverlappedFinished::new(
                Some("GetOverlappedResult"),
                Some(FileLine(file!(), line!())),
                self.ovl.take().unwrap(),
                bytes_transferred,
                result,
                last_error,
            ))
        }
    }
}

impl<'a, 'b> Drop for OverlappedPending<'a, 'b> {
    fn drop(&mut self) {
        let ovl = if let Some(ovl) = self.ovl.as_mut() {
            ovl
        } else {
            return;
        };

        unsafe {
            if CancelIoEx(**ovl.file, &mut ovl.ovl) != 0 {
                // The cancel has been requested, wait for it to finish.

                // FIXME TODO: I'm not happy with this blocking call in drop, but I don't see
                // a safe way around it. If we were dealing with an owned buffer we could leak it
                // as a last resort. We could maybe panic?
                // It may be worthwhile to check if closing the handle will effectively cancel
                // operations.
                // I think in the practical usage with named pipes the cancellation will finish
                // immediately anyway?

                let mut bytes_transferred = 0;
                let _result = GetOverlappedResult(
                    **ovl.file,
                    &mut ovl.ovl,
                    &mut bytes_transferred,
                    TRUE, // bWait
                );
            }
        }
    }
}

impl<'a, 'b> OverlappedResult<'a, 'b> {
    pub fn finish(self, timeout: WaitTimeout) -> Result<Option<OverlappedFinished>, comedy::Error> {
        match self {
            OverlappedResult::Finished(finished) => Ok(Some(finished)),
            OverlappedResult::Pending(mut pending) => match pending.wait(timeout)? {
                WaitResult::Signalled => match pending.get_result() {
                    OverlappedResult::Finished(OverlappedFinished {
                        ovl,
                        bytes_transferred,
                        result,
                    }) => {
                        let result = result?;
                        Ok(Some(OverlappedFinished {
                            ovl,
                            bytes_transferred,
                            result: Ok(result),
                        }))
                    }
                    OverlappedResult::Pending(_pending) => panic!("Signalled but still pending"),
                },
                WaitResult::Timeout => Ok(None),
            },
        }
    }
}

impl OverlappedFinished {
    fn new(
        function: Option<&'static str>,
        file_line: Option<FileLine>,
        ovl: Box<Overlapped>,
        bytes_transferred: DWORD,
        result: BOOL,
        last_error: DWORD,
    ) -> OverlappedFinished {
        if result == 0 {
            assert_ne!(last_error, ERROR_IO_PENDING);
            assert_ne!(last_error, ERROR_IO_INCOMPLETE);

            OverlappedFinished {
                ovl,
                bytes_transferred: None,
                result: Err(comedy::Error {
                    code: Some(ErrorCode::LastError(last_error)),
                    function: function,
                    file_line: file_line,
                }),
            }
        } else {
            OverlappedFinished {
                ovl,
                bytes_transferred: Some(bytes_transferred),
                result: Ok(result),
            }
        }
    }
}
