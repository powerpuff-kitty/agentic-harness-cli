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
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::time::{Duration, Instant};

    #[cfg(test)]
    thread_local! { static INJECT: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") }; }
    fn fault(name: &str) -> bool {
        #[cfg(test)]
        {
            INJECT.with(|fault| fault.get() == name)
        }
        #[cfg(not(test))]
        {
            let _ = name;
            false
        }
    }

    fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
        let fd = pipe.as_raw_fd();
        // SAFETY: fd is a live, owned child pipe. libc supplies platform ABI constants.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn no_live_group_members(group: libc::pid_t) -> bool {
        // Darwin killpg filters zombies then returns EPERM for zero eligible
        // members. Verify that case, never treat arbitrary EPERM as success.
        // Fixed capacity: overflow or any inspection uncertainty fails closed.
        let mut members = [0 as libc::pid_t; 1024];
        let capacity = std::mem::size_of_val(&members) as libc::c_int;
        // SAFETY: libc supplies the native ABI; buffer and byte count agree.
        // proc_listpgrppids returns a PID COUNT, unlike proc_listpids' byte count.
        unsafe {
            *libc::__error() = 0;
        }
        let count =
            unsafe { libc::proc_listpgrppids(group, members.as_mut_ptr().cast(), capacity) };
        if (count == 0 && io::Error::last_os_error().raw_os_error() != Some(0))
            || count < 0
            || count as usize >= members.len()
        {
            return false;
        }
        for pid in members.iter().take(count as usize) {
            if *pid <= 0 {
                return false;
            }
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of_val(&info) as libc::c_int;
            // SAFETY: proc_pidinfo writes the verified libc proc_bsdinfo layout.
            let read = unsafe {
                libc::proc_pidinfo(
                    *pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    (&mut info as *mut libc::proc_bsdinfo).cast(),
                    size,
                )
            };
            if read != size {
                if read == 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                    continue;
                }
                return false;
            }
            if info.pbi_pgid == group as u32 && info.pbi_status != libc::SZOMB {
                return false;
            }
        }
        true
    }

    struct Managed {
        child: Option<Child>,
        group: libc::pid_t,
        owned: bool,
        stopped: Option<bool>,
        no_live_members: bool,
    }

    impl Managed {
        // Never consumes status: a zombie leader reserves its identity until our
        // last group signal. No other code/thread may wait for this direct child.
        fn terminal(&mut self) -> io::Result<bool> {
            if !self.owned {
                return Err(io::Error::from_raw_os_error(libc::ECHILD));
            }
            // SAFETY: libc defines siginfo_t and waitid for each supported OS.
            // WNOWAIT leaves the sole owned child waitable; WNOHANG never blocks.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.group as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result != 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ECHILD) {
                    self.owned = false;
                }
                return Err(error);
            }
            // SAFETY: the initialized siginfo was populated by waitid, or remains zero.
            Ok(unsafe { info.si_pid() } != 0)
        }
        fn stop(&mut self) -> bool {
            if let Some(result) = self.stopped {
                return result;
            }
            // Recheck ownership before signaling; never signal after ECHILD/reaping.
            let observed = self.terminal();
            let success = if observed.is_ok() {
                // SAFETY: the child/group ID remains reserved by our unreaped
                // direct child. Negative PID addresses only its original group.
                let result = unsafe { libc::kill(-self.group, libc::SIGKILL) };
                let error = io::Error::last_os_error().raw_os_error();
                if result == 0 || error == Some(libc::ESRCH) {
                    true
                } else {
                    #[cfg(target_os = "macos")]
                    {
                        self.no_live_members = error == Some(libc::EPERM)
                            && matches!(observed, Ok(true))
                            && no_live_group_members(self.group);
                        self.no_live_members
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        false
                    }
                }
            } else {
                false
            };
            let success = success && !fault("signal");
            self.stopped = Some(success);
            success
        }
        fn reap(&mut self) -> io::Result<Option<ExitStatus>> {
            if !self.owned || self.stopped.is_none() {
                return Err(io::Error::from_raw_os_error(libc::ECHILD));
            }
            let result = self.child.as_mut().unwrap().try_wait();
            if matches!(result, Ok(Some(_)))
                || result
                    .as_ref()
                    .is_err_and(|e| e.raw_os_error() == Some(libc::ECHILD))
            {
                self.owned = false;
            }
            if fault("wait") && matches!(result, Ok(Some(_))) {
                return Err(io::Error::other("injected wait observation failure"));
            }
            result
        }
        fn recover(&mut self) -> Option<ExitStatus> {
            if !self.owned {
                return None;
            }
            self.stop();
            if self.terminal().is_ok() {
                // Still owns an unreaped child; this fallback never signals a group.
                let _ = self.child.as_mut().unwrap().kill();
            }
            let until = Instant::now() + Duration::from_millis(250);
            while self.owned && Instant::now() < until {
                if let Some(status) = self.reap().ok().flatten() {
                    return Some(status);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            if self.owned {
                // Retain direct-child wait responsibility without further signals.
                // The report remains an integrity failure; CLI termination can end
                // this recovery thread before an uninterruptible child exits.
                if let Some(mut child) = self.child.take() {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                self.owned = false;
            }
            None
        }
    }
    impl Drop for Managed {
        fn drop(&mut self) {
            let _ = self.recover();
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
            .envs(
                environment
                    .iter()
                    .map(|(key, value)| (key, value.as_str().unwrap())),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let child = match command.spawn() {
            Ok(child) => child,
            Err(_) => return empty("execution-error"),
        };
        let mut managed = Managed {
            group: child.id() as libc::pid_t,
            child: Some(child),
            owned: true,
            stopped: None,
            no_live_members: false,
        };
        let mut stdout = managed.child.as_mut().unwrap().stdout.take().unwrap();
        let mut stderr = managed.child.as_mut().unwrap().stderr.take().unwrap();
        let mut out = Capture::default();
        let mut err = Capture::default();
        let mut cause = None;
        if fault("setup") || nonblocking(&stdout).is_err() || nonblocking(&stderr).is_err() {
            cause = Some("execution-error");
        }
        let mut exit = None;
        let mut stopping = None;
        loop {
            if cause.is_none() && fault("read") {
                cause = Some("execution-error");
            }
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
                if cause.is_none() && out.bytes.len() + err.bytes.len() > max_output {
                    cause = Some("output-limit");
                }
            }
            // Completion must be observed before the deadline, including pipe EOF.
            // A late terminal observation never retroactively qualifies as success.
            if cause.is_none() && start.elapsed() >= Duration::from_millis(timeout_ms) {
                cause = Some("timeout");
            }
            if crate::execution_cancel::cancelled() {
                cause = Some("execution-error");
            }
            let terminal = if stopping.is_none() {
                match managed.terminal() {
                    Ok(value) => value,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => false,
                    Err(_) => {
                        cause = Some("execution-error");
                        false
                    }
                }
            } else {
                false
            };
            if terminal || cause.is_some() || stopping.is_some() {
                let stopped_at = stopping.get_or_insert_with(|| {
                    let started = Instant::now();
                    managed.stop();
                    started
                });
                if exit.is_none() {
                    match managed.reap() {
                        Ok(status) => exit = status,
                        Err(_) => cause = Some("execution-error"),
                    }
                }
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
        let recovery_started = Instant::now();
        if let Some(recovered) = managed.recover() {
            exit = Some(recovered);
        }
        let recovery_ms = recovery_started.elapsed().as_millis() as u64;
        let execution_ms = stopping
            .map_or_else(|| start.elapsed(), |t| t.duration_since(start))
            .as_millis() as u64;
        let cleanup_ms =
            stopping.map_or(0, |t| recovery_started.duration_since(t).as_millis() as u64);
        let status = cause.unwrap_or_else(|| {
            if exit.as_ref().is_some_and(|status| status.success()) {
                "passed"
            } else {
                "failed"
            }
        });
        json!({
            "status":status,"started_at_ms":started_at_ms,"ended_at_ms":now_ms(),
            "duration_ms":start.elapsed().as_millis() as u64,"spawned":true,
            "exit_code":exit.as_ref().and_then(|status| status.code()),
            "signal":exit.as_ref().and_then(|status| status.signal()),
            "direct_child_reaped":exit.is_some(),
            "process_group_cleanup":if !cleanup {"failed"} else if managed.no_live_members {"no-live-group-members"} else {"signal-sent-or-group-absent"},
            "output_disclosure":"omitted","stdout":out.evidence(),"stderr":err.evidence(),
            "timing":{"execution_ms":execution_ms,"cleanup_ms":cleanup_ms,"recovery_ms":recovery_ms}
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn owned_child_fixture() { /* A synthetic immediate-exit native child. */
        }
        fn child_command() -> Command {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "process_check::native::tests::owned_child_fixture",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            command
        }
        #[test]
        fn terminal_observation_keeps_identity_until_group_cleanup_and_reaping() {
            let child = child_command().spawn().unwrap();
            let mut managed = Managed {
                group: child.id() as libc::pid_t,
                child: Some(child),
                owned: true,
                stopped: None,
                no_live_members: false,
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            while !managed.terminal().unwrap() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(managed.owned);
            assert!(managed.terminal().unwrap()); // WNOWAIT was non-consuming.
            assert!(managed.stop());
            assert!(managed.reap().unwrap().is_some());
            assert!(!managed.owned);
            assert!(managed.stop()); // cached result, no post-reap signal.
        }
        #[test]
        fn lost_child_ownership_never_authorizes_group_signaling() {
            let child = child_command().spawn().unwrap();
            let mut managed = Managed {
                group: child.id() as libc::pid_t,
                child: Some(child),
                owned: true,
                stopped: None,
                no_live_members: false,
            };
            managed.child.as_mut().unwrap().wait().unwrap();
            assert!(!managed.stop());
            assert!(!managed.owned);
            assert_eq!(managed.stopped, Some(false));
        }
        #[test]
        fn real_backend_faults_are_integrity_failures_and_clean_up_owned_child() {
            let exe = std::env::current_exe().unwrap();
            let args = vec![
                "--exact".into(),
                "process_check::native::tests::owned_child_fixture".into(),
            ];
            let cwd = tempfile::tempdir().unwrap();
            for fault in ["setup", "read", "signal", "wait"] {
                INJECT.with(|value| value.set(fault));
                let result = execute(
                    exe.to_str().unwrap(),
                    &args,
                    cwd.path(),
                    &serde_json::Map::new(),
                    5000,
                    65536,
                );
                INJECT.with(|value| value.set(""));
                assert_eq!(result["status"], "execution-error", "{fault}: {result}");
                assert_eq!(result["spawned"], true);
                if fault != "wait" {
                    assert_eq!(result["direct_child_reaped"], true, "{fault}: {result}");
                }
                // The actual executor ledger must halt even when this check is optional.
                let mut ledger = crate::check_verdict::RunLedger::new(vec![
                    crate::check_verdict::CheckSpec {
                        id: "fault".into(),
                        required: false,
                    },
                    crate::check_verdict::CheckSpec {
                        id: "later".into(),
                        required: true,
                    },
                ])
                .unwrap();
                ledger.record("fault", result).unwrap();
                assert!(!ledger.may_continue());
                assert!(!ledger.finish().checks_passed);
            }
        }
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
