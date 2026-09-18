//! Cooperative monotonic deadline, covering review through finalization.
use std::cell::Cell;
use std::time::{Duration, Instant};

pub(crate) struct Budget {
    start: Instant,
    deadline: Cell<Instant>,
}

impl Budget {
    pub(crate) fn new() -> Self {
        let start = Instant::now();
        Self {
            start,
            deadline: Cell::new(start + Duration::from_millis(900_000)),
        }
    }
    pub(crate) fn limit(&self, ms: u64) -> Result<(), String> {
        self.deadline.set(
            self.deadline
                .get()
                .min(self.start + Duration::from_millis(ms)),
        );
        self.check()
    }
    pub(crate) fn check(&self) -> Result<(), String> {
        if crate::execution_cancel::cancelled() {
            return Err("checks: invocation cancelled".into());
        }
        self.check_at(Instant::now())
    }
    fn check_at(&self, now: Instant) -> Result<(), String> {
        if now >= self.deadline.get() {
            Err("checks: invocation deadline exhausted".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn remaining_ms(&self) -> u64 {
        self.deadline
            .get()
            .saturating_duration_since(Instant::now())
            .as_millis() as u64
    }
    pub(crate) fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_observation_and_revalidation_cannot_extend_budget() {
        let b = Budget::new();
        b.limit(1000).unwrap();
        let deadline = b.deadline.get();
        b.limit(900_000).unwrap();
        assert_eq!(b.deadline.get(), deadline);
        assert!(b.check_at(deadline - Duration::from_nanos(1)).is_ok());
        assert!(b.check_at(deadline).is_err());
        assert!(b.check_at(deadline + Duration::from_secs(1)).is_err());
    }
}
