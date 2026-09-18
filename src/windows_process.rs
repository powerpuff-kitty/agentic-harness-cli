//! Windows process ownership primitives for the bounded check executor.
//!
//! A Job Object is the Windows equivalent of the POSIX process-group boundary
//! used by the native executor.  The handle is deliberately RAII-owned: closing
//! it terminates all assigned descendants after the supervisor has finished its
//! final observation.
#![cfg(windows)]

use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::process::{Child, Command};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

#[allow(dead_code)]
pub(crate) struct Job {
    handle: HANDLE,
}

#[allow(dead_code)]
impl Job {
    pub(crate) fn new() -> io::Result<Self> {
        // SAFETY: null name/security attributes create a private unnamed job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::from_raw_os_error(
                unsafe { GetLastError() } as i32
            ));
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the structure and byte count match the selected information class.
        let result = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if result == 0 {
            let error = io::Error::from_raw_os_error(unsafe { GetLastError() } as i32);
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self { handle })
    }

    pub(crate) fn assign(&self, child: &Child) -> io::Result<()> {
        let process: RawHandle = child.as_raw_handle();
        // SAFETY: the child owns a live process handle for the duration of this call.
        if unsafe { AssignProcessToJobObject(self.handle, process as HANDLE) } == 0 {
            return Err(io::Error::from_raw_os_error(
                unsafe { GetLastError() } as i32
            ));
        }
        Ok(())
    }

    /// Spawn and assign without exposing an unmanaged child to the caller.
    pub(crate) fn spawn(&self, command: &mut Command) -> io::Result<Child> {
        let mut child = command.spawn()?;
        if let Err(error) = self.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(child)
    }

    pub(crate) fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: the handle is a live, privately-owned Job Object.
        if unsafe { TerminateJobObject(self.handle, exit_code) } == 0 {
            return Err(io::Error::from_raw_os_error(
                unsafe { GetLastError() } as i32
            ));
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: this object uniquely owns the job handle.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Job;
    use std::process::{Command, Stdio};

    #[test]
    fn creates_a_kill_on_close_job() {
        let job = Job::new().expect("job object should be available on Windows CI");
        let mut command = Command::new("cmd.exe");
        command
            .args(["/C", "exit", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = job
            .spawn(&mut command)
            .expect("fixture process should spawn");
        child.wait().expect("fixture process should be reaped");
    }

    #[test]
    fn terminates_owned_job_processes() {
        let job = Job::new().expect("job object should be available on Windows CI");
        let mut command = Command::new("cmd.exe");
        command
            .args(["/C", "ping 127.0.0.1 -n 30 > NUL"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = job
            .spawn(&mut command)
            .expect("fixture process should spawn");
        job.terminate(137).expect("owned job should terminate");
        child.wait().expect("terminated process should be reaped");
    }
}
