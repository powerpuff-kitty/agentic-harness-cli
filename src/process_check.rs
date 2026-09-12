//! Bounded local command supervision. Not a filesystem, network or hostile-code sandbox.
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn empty(status: &str) -> Value {
    json!({
        "status":status,"started_at_ms":null,"ended_at_ms":null,"duration_ms":0,
        "exit_code":null,"signal":null,"spawned":false,"direct_child_reaped":false,
        "process_group_cleanup":"not-attempted","output_disclosure":"omitted",
        "stdout":null,"stderr":null
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::{empty, now_ms};
    use crate::check_inputs;
    use serde_json::{Value, json};
    use std::io::{self, Read};
    use std::os::fd::AsRawFd;
    use std::os::raw::c_int;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    // Minimal POSIX ABI surface, compiled only for the two tested OS families.
    // F_GETFL/F_SETFL, SIGKILL and ESRCH have these values on Linux and Darwin.
    unsafe extern "C" {
        fn fcntl(fd: c_int, command: c_int, ...) -> c_int;
        fn kill(pid: c_int, signal: c_int) -> c_int;
    }
    #[cfg(target_os = "linux")]
    const NONBLOCK: c_int = 2048;
    #[cfg(target_os = "macos")]
    const NONBLOCK: c_int = 4;

    fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
        let fd = pipe.as_raw_fd();
        // SAFETY: fd is a live child pipe owned by this function's caller. No ownership transfer.
        let flags = unsafe { fcntl(fd, 3) };
        if flags < 0 || unsafe { fcntl(fd, 4, flags | NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    struct Managed {
        child: Child,
        group: c_int,
        stopped: Option<bool>,
    }

    impl Managed {
        fn stop(&mut self) -> bool {
            if let Some(result) = self.stopped {
                return result;
            }
            // SAFETY: group is the positive PID of our child, started with process_group(0).
            // Negative PID addresses only that group; SIGKILL takes no pointer arguments.
            let result = unsafe { kill(-self.group, 9) };
            let success = result == 0 || io::Error::last_os_error().raw_os_error() == Some(3);
            self.stopped = Some(success);
            success
        }
    }

    impl Drop for Managed {
        fn drop(&mut self) {
            self.stop();
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
            }
        }
    }

    #[derive(Default)]
    struct Capture {
        bytes: Vec<u8>,
        eof: bool,
    }

    impl Capture {
        fn read(&mut self, pipe: &mut impl Read, remaining: usize) -> io::Result<()> {
            let initial = self.bytes.len();
            let mut buffer = [0u8; 4096];
            // Bound work per polling iteration even when the writer never becomes idle.
            for _ in 0..16 {
                let allowance = remaining.saturating_sub(self.bytes.len() - initial);
                let size = buffer.len().min(allowance + 1);
                match pipe.read(&mut buffer[..size]) {
                    Ok(0) => {
                        self.eof = true;
                        break;
                    }
                    Ok(count) => {
                        self.bytes.extend_from_slice(&buffer[..count]);
                        if self.bytes.len() - initial > remaining {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }

        fn evidence(&self) -> Value {
            json!({
                "observed_bytes":self.bytes.len(),"sha256":check_inputs::hash(&self.bytes),
                "complete":self.eof,"hash_scope":if self.eof {"complete-stream"} else {"observed-prefix"}
            })
        }
    }

    pub(super) fn execute(
        executable: &str,
        args: &[String],
        cwd: &std::path::Path,
        environment: &serde_json::Map<String, Value>,
        timeout_ms: u64,
        max_output: usize,
    ) -> Value {
        let start = Instant::now();
        let started_at_ms = now_ms();
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .envs(environment.iter().map(|(key, value)| (key, value.as_str().unwrap())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let child = match command.spawn() {
            Ok(child) => child,
            Err(_) => return empty("execution-error"),
        };
        let mut managed = Managed { group: child.id() as c_int, child, stopped: None };
        let mut stdout = managed.child.stdout.take().unwrap();
        let mut stderr = managed.child.stderr.take().unwrap();
        let mut out = Capture::default();
        let mut err = Capture::default();
        let mut cause = None;
        if nonblocking(&stdout).is_err() || nonblocking(&stderr).is_err() {
            cause = Some("execution-error");
        }
        let mut exit = None;
        let mut stopping = None;
        loop {
            if cause.is_none() {
                let remaining = max_output.saturating_sub(out.bytes.len() + err.bytes.len());
                if out.read(&mut stdout, remaining).is_err() {
                    cause = Some("execution-error");
                }
                if out.bytes.len() + err.bytes.len() <= max_output {
                    let remaining = max_output - out.bytes.len() - err.bytes.len();
                    if err.read(&mut stderr, remaining).is_err() {
                        cause = Some("execution-error");
                    }
                }
                if out.bytes.len() + err.bytes.len() > max_output {
                    cause = Some("output-limit");
                }
            }
            if exit.is_none() {
                match managed.child.try_wait() {
                    Ok(status) => exit = status,
                    Err(_) => cause = Some("execution-error"),
                }
            }
            if cause.is_none() && exit.is_none() && start.elapsed() >= Duration::from_millis(timeout_ms) {
                cause = Some("timeout");
            }
            if exit.is_some() || cause.is_some() {
                let stopped_at = stopping.get_or_insert_with(|| {
                    managed.stop();
                    Instant::now()
                });
                if exit.is_some() && (cause.is_some() || (out.eof && err.eof)) {
                    break;
                }
                if stopped_at.elapsed() >= Duration::from_millis(250) {
                    cause = Some("execution-error");
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let cleanup = managed.stop();
        if !cleanup || exit.is_none() {
            cause = Some("execution-error");
        }
        let status = cause.unwrap_or_else(|| {
            if exit.as_ref().is_some_and(|status| status.success()) { "passed" } else { "failed" }
        });
        json!({
            "status":status,"started_at_ms":started_at_ms,"ended_at_ms":now_ms(),
            "duration_ms":start.elapsed().as_millis() as u64,"spawned":true,
            "exit_code":exit.as_ref().and_then(|status| status.code()),
            "signal":exit.as_ref().and_then(|status| status.signal()),
            "direct_child_reaped":exit.is_some(),
            "process_group_cleanup":if cleanup {"signal-sent-or-group-absent"} else {"failed"},
            "output_disclosure":"omitted","stdout":out.evidence(),"stderr":err.evidence()
        })
    }
}

pub(crate) fn execute(
    executable: &str,
    args: &[String],
    cwd: &std::path::Path,
    environment: &serde_json::Map<String, Value>,
    timeout_ms: u64,
    max_output: usize,
) -> Value {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        native::execute(executable, args, cwd, environment, timeout_ms, max_output)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (executable, args, cwd, environment, timeout_ms, max_output);
        empty("unsupported")
    }
}
