//! SIGINT/SIGTERM only set an atomic flag. Cleanup runs in ordinary Rust code.
use std::sync::atomic::{AtomicBool, Ordering};
static CANCELLED: AtomicBool = AtomicBool::new(false);
pub(crate) fn cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::*;
    use std::sync::{Mutex, MutexGuard};
    static OWNER: Mutex<()> = Mutex::new(());
    extern "C" fn notify(_: libc::c_int) {
        CANCELLED.store(true, Ordering::Relaxed);
    }
    pub(crate) struct Guard {
        previous: Vec<(libc::c_int, libc::sigaction)>,
        _owner: MutexGuard<'static, ()>,
    }
    impl Guard {
        pub(crate) fn install() -> Result<Self, String> {
            let owner = OWNER
                .try_lock()
                .map_err(|_| "checks: execution supervisor already active")?;
            CANCELLED.store(false, Ordering::Relaxed);
            let mut guard = Self {
                previous: Vec::new(),
                _owner: owner,
            };
            for signal in [libc::SIGINT, libc::SIGTERM] {
                // SAFETY: initialized libc ABI values; handler touches only a lock-free atomic.
                let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
                let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
                action.sa_sigaction = notify as *const () as usize;
                unsafe {
                    libc::sigemptyset(&mut action.sa_mask);
                }
                if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
                    return Err("checks: cannot install cancellation handler".into());
                }
                guard.previous.push((signal, previous));
            }
            Ok(guard)
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            for (signal, previous) in self.previous.iter().rev() {
                // SAFETY: restores the exact disposition saved by this exclusively owned guard.
                unsafe {
                    libc::sigaction(*signal, previous, std::ptr::null_mut());
                }
            }
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use native::Guard;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) struct Guard;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Guard {
    pub(crate) fn install() -> Result<Self, String> {
        Ok(Self)
    }
}
