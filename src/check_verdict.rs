//! Pure, fail-closed bookkeeping for a reviewed sequence of check outcomes.
//!
//! No processes, filesystem reads, approval decisions or evidence authentication.
//! Callers must validate the full artifact schema and bind the plan to trusted,
//! current inputs separately. This experimental API never verifies completion.
use serde_json::Value;
use std::collections::BTreeSet;

/// A check's required flag comes from the reviewed plan, never its result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckSpec {
    pub id: String,
    pub required: bool,
}

/// Stable reasons for stopping the sequence. The first cause is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HaltReason {
    SupervisorIntegrity,
    InputRevalidation,
    BudgetExhausted,
    InvalidSequence,
    MissingResults,
}

/// Invalid plans or out-of-order results never echo untrusted content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerError {
    InvalidPlan,
    UnexpectedCheck,
    ResultAfterHalt,
}

/// Original accepted observations are retained even when they cause a halt.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedCheck {
    pub spec: CheckSpec,
    pub outcome: Option<Value>,
    pub skipped_because: Option<HaltReason>,
}

/// Bookkeeping facts, not an authenticated execution report or readiness score.
#[derive(Clone, Debug, PartialEq)]
pub struct RunVerdict {
    pub checks_passed: bool,
    pub checks_executed: bool,
    pub inputs_current: bool,
    pub completion_verified: bool,
    pub halt_reason: Option<HaltReason>,
    pub records: Vec<RecordedCheck>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disposition {
    Passed,
    Unsuccessful,
    NotExecuted,
    SupervisorFault,
}

fn valid_stream(stream: &Value, require_complete: bool) -> bool {
    let Some(complete) = stream.get("complete").and_then(Value::as_bool) else {
        return false;
    };
    let scope = if complete {
        "complete-stream"
    } else {
        "observed-prefix"
    };
    let valid_hash = stream
        .get("sha256")
        .and_then(Value::as_str)
        .and_then(|value| value.strip_prefix("sha256:"))
        .is_some_and(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
    (!require_complete || complete)
        && stream.get("hash_scope").and_then(Value::as_str) == Some(scope)
        && stream
            .get("observed_bytes")
            .and_then(Value::as_u64)
            .is_some_and(|count| count <= 1_048_577)
        && valid_hash
}

fn null_field(outcome: &Value, key: &str) -> bool {
    outcome.get(key).is_some_and(Value::is_null)
}

fn disposition(outcome: &Value) -> Disposition {
    let status = outcome.get("status").and_then(Value::as_str);
    if outcome.get("output_disclosure").and_then(Value::as_str) != Some("omitted") {
        return Disposition::SupervisorFault;
    }
    if matches!(status, Some("skipped" | "unsupported")) {
        return if outcome.get("spawned").and_then(Value::as_bool) == Some(false)
            && outcome.get("direct_child_reaped").and_then(Value::as_bool) == Some(false)
            && outcome.get("process_group_cleanup").and_then(Value::as_str) == Some("not-attempted")
            && ["exit_code", "signal", "stdout", "stderr"]
                .iter()
                .all(|key| null_field(outcome, key))
        {
            Disposition::NotExecuted
        } else {
            Disposition::SupervisorFault
        };
    }
    if !matches!(
        status,
        Some("passed" | "failed" | "timeout" | "output-limit")
    ) || outcome.get("spawned").and_then(Value::as_bool) != Some(true)
        || outcome.get("direct_child_reaped").and_then(Value::as_bool) != Some(true)
        || !matches!(
            outcome.get("process_group_cleanup").and_then(Value::as_str),
            Some("signal-sent-or-group-absent" | "no-live-group-members")
        )
    {
        return Disposition::SupervisorFault;
    }
    let complete = matches!(status, Some("passed" | "failed"));
    if !valid_stream(&outcome["stdout"], complete) || !valid_stream(&outcome["stderr"], complete) {
        return Disposition::SupervisorFault;
    }
    let code = outcome.get("exit_code").and_then(Value::as_i64);
    let signal = outcome.get("signal").and_then(Value::as_i64);
    let normal_exit =
        code.is_some_and(|value| (0..=255).contains(&value)) && null_field(outcome, "signal");
    let signaled_exit = signal.is_some_and(|value| value > 0) && null_field(outcome, "exit_code");
    if !normal_exit && !signaled_exit {
        return Disposition::SupervisorFault;
    }
    match status {
        Some("passed") if code == Some(0) && normal_exit => Disposition::Passed,
        Some("failed") if code.is_some_and(|value| value > 0) || signaled_exit => {
            Disposition::Unsuccessful
        }
        Some("timeout" | "output-limit") => Disposition::Unsuccessful,
        _ => Disposition::SupervisorFault,
    }
}

/// A sticky, ordered ledger. It never schedules or launches a check.
#[derive(Debug)]
pub struct RunLedger {
    plan: Vec<CheckSpec>,
    records: Vec<RecordedCheck>,
    halt: Option<HaltReason>,
    inputs_current: bool,
}

impl RunLedger {
    pub fn new(plan: Vec<CheckSpec>) -> Result<Self, LedgerError> {
        let mut ids = BTreeSet::new();
        if plan.is_empty()
            || plan.len() > 64
            || !plan.iter().any(|check| check.required)
            || plan.iter().any(|check| {
                check.id.is_empty()
                    || check.id.len() > 128
                    || !check
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
                    || !ids.insert(check.id.clone())
            })
        {
            return Err(LedgerError::InvalidPlan);
        }
        Ok(Self {
            plan,
            records: Vec::new(),
            halt: None,
            inputs_current: true,
        })
    }

    /// This says only that bookkeeping permits a next step, not that it is authorized.
    pub fn may_continue(&self) -> bool {
        self.halt.is_none() && self.records.len() < self.plan.len()
    }

    pub fn halt_reason(&self) -> Option<HaltReason> {
        self.halt
    }

    pub fn records(&self) -> &[RecordedCheck] {
        &self.records
    }

    /// An error after earlier work must stop the ledger rather than discard it.
    pub fn stop(&mut self, reason: HaltReason) {
        if reason == HaltReason::InputRevalidation {
            self.inputs_current = false;
        }
        self.halt.get_or_insert(reason);
    }

    /// Append exactly the next planned result; optional flags are never caller supplied.
    pub fn record(&mut self, id: &str, outcome: Value) -> Result<(), LedgerError> {
        let Some(spec) = self.plan.get(self.records.len()).cloned() else {
            self.stop(HaltReason::InvalidSequence);
            return Err(LedgerError::UnexpectedCheck);
        };
        if spec.id != id {
            self.stop(HaltReason::InvalidSequence);
            return Err(LedgerError::UnexpectedCheck);
        }
        if self.halt.is_some() {
            // Preserve a contradicting late observation instead of silently losing it.
            self.records.push(RecordedCheck {
                spec,
                outcome: Some(outcome),
                skipped_because: None,
            });
            return Err(LedgerError::ResultAfterHalt);
        }
        if disposition(&outcome) == Disposition::SupervisorFault {
            self.stop(HaltReason::SupervisorIntegrity);
        }
        self.records.push(RecordedCheck {
            spec,
            outcome: Some(outcome),
            skipped_because: None,
        });
        Ok(())
    }

    /// Missing results become explicit unexecuted entries and can never be a pass.
    pub fn finish(mut self) -> RunVerdict {
        if self.records.len() < self.plan.len() {
            self.stop(HaltReason::MissingResults);
            for spec in &self.plan[self.records.len()..] {
                self.records.push(RecordedCheck {
                    spec: spec.clone(),
                    outcome: None,
                    skipped_because: self.halt,
                });
            }
        }
        let checks_passed = self.halt.is_none()
            && self.inputs_current
            && self
                .records
                .iter()
                .filter(|r| r.spec.required)
                .all(|r| r.outcome.as_ref().map(disposition) == Some(Disposition::Passed));
        let checks_executed = self.records.iter().any(|r| {
            r.outcome
                .as_ref()
                .and_then(|value| value.get("spawned"))
                .and_then(Value::as_bool)
                == Some(true)
        });
        RunVerdict {
            checks_passed,
            checks_executed,
            inputs_current: self.inputs_current,
            completion_verified: false,
            halt_reason: self.halt,
            records: self.records,
        }
    }
}
