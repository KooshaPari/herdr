use crate::{Child, ChildKiller, ExitStatus};
use anyhow::Context as _;
use std::io::{Error as IoError, Result as IoResult};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use winapi::shared::minwindef::DWORD;
use winapi::um::processthreadsapi::*;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::{INFINITE, WAIT_FAILED, WAIT_OBJECT_0};

pub mod conpty;
mod procthreadattr;
mod psuedocon;

use filedescriptor::OwnedHandle;

const WAIT_TIMEOUT_RESULT: DWORD = 258;

fn termination_result(result: i32, last_error: impl FnOnce() -> IoError) -> IoResult<()> {
    if result == 0 {
        Err(last_error())
    } else {
        Ok(())
    }
}

fn wait_result(result: DWORD, last_error: impl FnOnce() -> IoError) -> IoResult<()> {
    if result == WAIT_OBJECT_0 {
        Ok(())
    } else if result == WAIT_FAILED {
        Err(last_error())
    } else {
        Err(IoError::other(format!(
            "unexpected process wait result: {result}"
        )))
    }
}

fn poll_wait_result(result: DWORD, last_error: impl FnOnce() -> IoError) -> IoResult<bool> {
    if result == WAIT_TIMEOUT_RESULT {
        Ok(false)
    } else {
        wait_result(result, last_error)?;
        Ok(true)
    }
}

#[derive(Debug)]
pub struct WinChild {
    proc: Mutex<OwnedHandle>,
}

impl WinChild {
    fn is_complete(&mut self) -> IoResult<Option<ExitStatus>> {
        let proc = self
            .proc
            .lock()
            .map_err(|_| IoError::other("child process handle lock poisoned"))?
            .try_clone()?;
        let waited = unsafe { WaitForSingleObject(proc.as_raw_handle() as _, 0) };
        if !poll_wait_result(waited, IoError::last_os_error)? {
            return Ok(None);
        }
        let mut status: DWORD = 0;
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            Ok(Some(ExitStatus::with_exit_code(status)))
        } else {
            Err(IoError::last_os_error())
        }
    }

    fn do_kill(&mut self) -> IoResult<()> {
        let proc = self
            .proc
            .lock()
            .map_err(|_| IoError::other("child process handle lock poisoned"))?
            .try_clone()?;
        let res = unsafe { TerminateProcess(proc.as_raw_handle() as _, 1) };
        termination_result(res, IoError::last_os_error)
    }
}

impl ChildKiller for WinChild {
    fn kill(&mut self) -> IoResult<()> {
        self.do_kill()
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.lock().unwrap().try_clone().unwrap();
        Box::new(WinChildKiller { proc })
    }
}

#[derive(Debug)]
pub struct WinChildKiller {
    proc: OwnedHandle,
}

impl ChildKiller for WinChildKiller {
    fn kill(&mut self) -> IoResult<()> {
        let res = unsafe { TerminateProcess(self.proc.as_raw_handle() as _, 1) };
        termination_result(res, IoError::last_os_error)
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        let proc = self.proc.try_clone().unwrap();
        Box::new(WinChildKiller { proc })
    }
}

#[cfg(test)]
mod termination_result_tests {
    use super::*;

    #[test]
    fn successful_termination_does_not_read_stale_error() {
        assert!(termination_result(1, || panic!("success must not read last error")).is_ok());
    }

    #[test]
    fn failed_termination_returns_the_actual_error() {
        let error = termination_result(0, || IoError::from_raw_os_error(5)).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
    }

    #[test]
    fn wait_failure_and_unexpected_wait_are_not_success() {
        assert!(wait_result(WAIT_OBJECT_0, || panic!("no error on success")).is_ok());
        assert_eq!(
            wait_result(WAIT_FAILED, || IoError::from_raw_os_error(5))
                .unwrap_err()
                .raw_os_error(),
            Some(5)
        );
        assert!(wait_result(258, || panic!("unexpected status is not WAIT_FAILED")).is_err());
    }

    #[test]
    fn poll_wait_distinguishes_pending_signaled_and_failed() {
        assert!(!poll_wait_result(WAIT_TIMEOUT_RESULT, || panic!("no error on timeout")).unwrap());
        assert!(poll_wait_result(WAIT_OBJECT_0, || panic!("no error on success")).unwrap());
        assert_eq!(
            poll_wait_result(WAIT_FAILED, || IoError::from_raw_os_error(5))
                .unwrap_err()
                .raw_os_error(),
            Some(5)
        );
        assert!(poll_wait_result(42, || panic!("unexpected status is not WAIT_FAILED")).is_err());
    }
}

impl Child for WinChild {
    fn try_wait(&mut self) -> IoResult<Option<ExitStatus>> {
        self.is_complete()
    }

    fn wait(&mut self) -> IoResult<ExitStatus> {
        if let Some(status) = self.try_wait()? {
            return Ok(status);
        }
        let proc = self
            .proc
            .lock()
            .map_err(|_| IoError::other("child process handle lock poisoned"))?
            .try_clone()?;
        let waited = unsafe { WaitForSingleObject(proc.as_raw_handle() as _, INFINITE) };
        wait_result(waited, IoError::last_os_error)?;
        let mut status: DWORD = 0;
        let res = unsafe { GetExitCodeProcess(proc.as_raw_handle() as _, &mut status) };
        if res != 0 {
            Ok(ExitStatus::with_exit_code(status))
        } else {
            Err(IoError::last_os_error())
        }
    }

    fn process_id(&self) -> Option<u32> {
        let res = unsafe { GetProcessId(self.proc.lock().unwrap().as_raw_handle() as _) };
        if res == 0 {
            None
        } else {
            Some(res)
        }
    }

    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        let proc = self.proc.lock().unwrap();
        Some(proc.as_raw_handle())
    }
}

impl std::future::Future for WinChild {
    type Output = anyhow::Result<ExitStatus>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<anyhow::Result<ExitStatus>> {
        match self.is_complete() {
            Ok(Some(status)) => Poll::Ready(Ok(status)),
            Err(err) => Poll::Ready(Err(err).context("Failed to retrieve process exit status")),
            Ok(None) => {
                struct PassRawHandleToWaiterThread(pub RawHandle);
                unsafe impl Send for PassRawHandleToWaiterThread {}

                let proc = self.proc.lock().unwrap().try_clone()?;
                let handle = PassRawHandleToWaiterThread(proc.as_raw_handle());

                let waker = cx.waker().clone();
                std::thread::spawn(move || {
                    unsafe {
                        WaitForSingleObject(handle.0 as _, INFINITE);
                    }
                    waker.wake();
                });
                Poll::Pending
            }
        }
    }
}
