use agentic_harness_cli::check_verdict::{CheckSpec, HaltReason, LedgerError, RunLedger};
use serde_json::{Value, json};

fn spec(id: &str, required: bool) -> CheckSpec {
    CheckSpec {
        id: id.into(),
        required,
    }
}

fn outcome(status: &str) -> Value {
    let unexecuted = matches!(status, "skipped" | "unsupported");
    let mut stream = json!({
        "observed_bytes":0,"sha256":format!("sha256:{}", "a".repeat(64)),
        "complete":true,"hash_scope":"complete-stream"
    });
    if unexecuted {
        stream = Value::Null;
    }
    json!({
        "status":status,"spawned":!unexecuted,"direct_child_reaped":!unexecuted,
        "process_group_cleanup":if unexecuted {"not-attempted"} else {"signal-sent-or-group-absent"},
        "output_disclosure":"omitted","stdout":stream,"stderr":stream,
        "exit_code":if unexecuted {Value::Null} else {json!(if status == "passed" {0} else {1})},
        "signal":null
    })
}

fn optional_verdict(value: Value) -> agentic_harness_cli::check_verdict::RunVerdict {
    let mut ledger = RunLedger::new(vec![spec("required", true), spec("optional", false)]).unwrap();
    ledger.record("required", outcome("passed")).unwrap();
    ledger.record("optional", value).unwrap();
    ledger.finish()
}

#[test]
fn empty_all_optional_duplicate_and_invalid_plans_are_rejected() {
    for plan in [
        vec![],
        vec![spec("optional", false)],
        vec![spec("same", true), spec("same", false)],
        vec![spec("has spaces", true)],
        vec![spec("", true)],
        vec![spec(&"a".repeat(129), true)],
        (0..65).map(|n| spec(&format!("check-{n}"), true)).collect(),
    ] {
        assert_eq!(RunLedger::new(plan).unwrap_err(), LedgerError::InvalidPlan);
    }
}

#[test]
fn original_optional_cleanup_counterexample_is_rejected() {
    let mut failed = outcome("failed");
    failed["process_group_cleanup"] = json!("failed");
    let verdict = optional_verdict(failed.clone());
    assert!(!verdict.checks_passed);
    assert_eq!(verdict.halt_reason, Some(HaltReason::SupervisorIntegrity));
    assert_eq!(verdict.records[1].outcome, Some(failed));
    assert_eq!(verdict.records[0].outcome, Some(outcome("passed")));
}

#[test]
fn optional_unreaped_child_is_fatal() {
    let mut failed = outcome("timeout");
    failed["direct_child_reaped"] = json!(false);
    assert!(!optional_verdict(failed).checks_passed);
}

#[test]
fn ordinary_optional_failures_and_clean_limits_remain_visible() {
    for status in ["failed", "timeout", "output-limit", "skipped", "unsupported"] {
        let verdict = optional_verdict(outcome(status));
        assert!(verdict.checks_passed, "{status}");
        assert_eq!(verdict.records[1].outcome.as_ref().unwrap()["status"], status);
        assert_eq!(verdict.halt_reason, None);
        assert!(!verdict.completion_verified);
    }
}

#[test]
fn required_failure_cannot_be_downgraded_by_result_metadata() {
    let mut ledger = RunLedger::new(vec![spec("required", true)]).unwrap();
    let mut failed = outcome("failed");
    failed["required"] = json!(false);
    ledger.record("required", failed).unwrap();
    assert!(!ledger.finish().checks_passed);
}

#[test]
fn clean_required_nonpass_always_fails_without_inventing_supervisor_failure() {
    for status in ["failed", "timeout", "output-limit", "skipped", "unsupported"] {
        let mut ledger = RunLedger::new(vec![spec("required", true)]).unwrap();
        ledger.record("required", outcome(status)).unwrap();
        let verdict = ledger.finish();
        assert!(!verdict.checks_passed);
        assert_eq!(verdict.halt_reason, None);
    }
}

#[test]
fn optional_execution_error_and_capture_setup_errors_always_halt() {
    for spawned in [true, false] {
        let mut value = outcome("execution-error");
        value["spawned"] = json!(spawned);
        let verdict = optional_verdict(value);
        assert_eq!(verdict.halt_reason, Some(HaltReason::SupervisorIntegrity));
    }
}

#[test]
fn fault_stops_later_work_and_finishes_with_explicit_skips() {
    let mut ledger = RunLedger::new(vec![
        spec("required", true),
        spec("optional", false),
        spec("later", true),
    ])
    .unwrap();
    ledger.record("required", outcome("passed")).unwrap();
    ledger.record("optional", outcome("execution-error")).unwrap();
    assert!(!ledger.may_continue());
    let verdict = ledger.finish();
    assert_eq!(verdict.records.len(), 3);
    assert_eq!(verdict.records[2].outcome, None);
    assert_eq!(verdict.records[2].skipped_because, verdict.halt_reason);
    assert!(!verdict.checks_passed);
}

#[test]
fn successful_later_observation_cannot_clear_a_previous_halt() {
    let mut ledger = RunLedger::new(vec![spec("first", true), spec("later", true)]).unwrap();
    ledger.record("first", outcome("execution-error")).unwrap();
    assert_eq!(
        ledger.record("later", outcome("passed")),
        Err(LedgerError::ResultAfterHalt)
    );
    let verdict = ledger.finish();
    assert!(!verdict.checks_passed);
    assert_eq!(verdict.halt_reason, Some(HaltReason::SupervisorIntegrity));
    assert_eq!(verdict.records[1].outcome, Some(outcome("passed")));
}

#[test]
fn later_revalidation_error_keeps_earlier_results() {
    let mut ledger = RunLedger::new(vec![spec("first", true), spec("later", true)]).unwrap();
    ledger.record("first", outcome("passed")).unwrap();
    ledger.stop(HaltReason::InputRevalidation);
    let verdict = ledger.finish();
    assert!(!verdict.inputs_current);
    assert!(!verdict.checks_passed);
    assert_eq!(verdict.records[0].outcome, Some(outcome("passed")));
    assert_eq!(verdict.records[1].skipped_because, Some(HaltReason::InputRevalidation));
}

#[test]
fn first_failure_reason_is_sticky_but_input_invalidity_is_also_preserved() {
    let mut ledger = RunLedger::new(vec![spec("first", true)]).unwrap();
    ledger.stop(HaltReason::SupervisorIntegrity);
    ledger.stop(HaltReason::InputRevalidation);
    ledger.stop(HaltReason::BudgetExhausted);
    let verdict = ledger.finish();
    assert_eq!(verdict.halt_reason, Some(HaltReason::SupervisorIntegrity));
    assert!(!verdict.inputs_current);
    assert!(!verdict.checks_passed);
}

#[test]
fn missing_optional_result_is_not_silently_treated_as_a_pass() {
    let mut ledger = RunLedger::new(vec![spec("first", true), spec("optional", false)]).unwrap();
    ledger.record("first", outcome("passed")).unwrap();
    let verdict = ledger.finish();
    assert_eq!(verdict.halt_reason, Some(HaltReason::MissingResults));
    assert!(!verdict.checks_passed);
}

#[test]
fn empty_results_never_pass() {
    let verdict = RunLedger::new(vec![spec("first", true)]).unwrap().finish();
    assert!(!verdict.checks_passed);
    assert!(!verdict.checks_executed);
    assert_eq!(verdict.records[0].outcome, None);
}

#[test]
fn out_of_order_duplicate_and_extra_results_are_rejected() {
    for wrong in ["later", "unknown"] {
        let mut ledger = RunLedger::new(vec![spec("first", true), spec("later", false)]).unwrap();
        assert_eq!(
            ledger.record(wrong, outcome("passed")),
            Err(LedgerError::UnexpectedCheck)
        );
        assert!(!ledger.finish().checks_passed);
    }
    let mut ledger = RunLedger::new(vec![spec("first", true)]).unwrap();
    ledger.record("first", outcome("passed")).unwrap();
    assert_eq!(
        ledger.record("first", outcome("passed")),
        Err(LedgerError::UnexpectedCheck)
    );
    assert!(!ledger.finish().checks_passed);
}

#[test]
fn budget_failure_after_recording_prevents_success() {
    let mut ledger = RunLedger::new(vec![spec("first", true)]).unwrap();
    ledger.record("first", outcome("passed")).unwrap();
    ledger.stop(HaltReason::BudgetExhausted);
    assert!(!ledger.finish().checks_passed);
}

#[test]
fn missing_or_invalid_supervision_fields_fail_closed() {
    for key in ["status", "spawned", "direct_child_reaped", "process_group_cleanup", "output_disclosure"] {
        let mut value = outcome("passed");
        value.as_object_mut().unwrap().remove(key);
        assert!(!optional_verdict(value).checks_passed, "missing {key}");
        let mut value = outcome("passed");
        value[key] = json!("unknown");
        assert!(!optional_verdict(value).checks_passed, "invalid {key}");
    }
    for value in [Value::Null, json!([]), json!(false), json!({})] {
        assert!(!optional_verdict(value).checks_passed);
    }
}

#[test]
fn incomplete_or_invalid_capture_cannot_claim_normal_success_or_failure() {
    for side in ["stdout", "stderr"] {
        for status in ["passed", "failed"] {
            let mut value = outcome(status);
            value[side]["complete"] = json!(false);
            value[side]["hash_scope"] = json!("observed-prefix");
            assert!(!optional_verdict(value).checks_passed);
        }
        for (field, bad) in [
            ("complete", json!("yes")),
            ("hash_scope", json!("observed-prefix")),
            ("observed_bytes", json!(-1)),
            ("observed_bytes", json!(1_048_578)),
            ("sha256", json!("unknown")),
        ] {
            let mut value = outcome("passed");
            value[side][field] = bad;
            assert!(!optional_verdict(value).checks_passed);
        }
    }
}

#[test]
fn incomplete_capture_can_be_honest_for_a_clean_timeout() {
    let mut value = outcome("timeout");
    for side in ["stdout", "stderr"] {
        value[side]["complete"] = json!(false);
        value[side]["hash_scope"] = json!("observed-prefix");
    }
    assert!(optional_verdict(value).checks_passed);
}

#[test]
fn conflicting_exit_status_is_not_accepted() {
    for (status, code, signal) in [
        ("passed", json!(1), Value::Null),
        ("passed", json!(0), json!(9)),
        ("failed", json!(0), Value::Null),
        ("failed", Value::Null, Value::Null),
        ("failed", json!(-1), Value::Null),
        ("failed", json!(256), Value::Null),
    ] {
        let mut value = outcome(status);
        value["exit_code"] = code;
        value["signal"] = signal;
        assert!(!optional_verdict(value).checks_passed);
    }
    let mut signaled = outcome("failed");
    signaled["exit_code"] = Value::Null;
    signaled["signal"] = json!(9);
    assert!(optional_verdict(signaled).checks_passed);
}

#[test]
fn no_optional_spawned_result_survives_missing_cleanup_or_reaping() {
    for status in ["passed", "failed", "timeout", "output-limit", "execution-error", "skipped", "unsupported"] {
        for reaped in [false, true] {
            for cleanup in ["not-attempted", "failed", "signal-sent-or-group-absent"] {
                if reaped && cleanup == "signal-sent-or-group-absent" {
                    continue;
                }
                let mut value = outcome(status);
                value["spawned"] = json!(true);
                value["direct_child_reaped"] = json!(reaped);
                value["process_group_cleanup"] = json!(cleanup);
                let verdict = optional_verdict(value);
                assert!(!verdict.checks_passed, "{status}/{reaped}/{cleanup}");
                assert_eq!(verdict.halt_reason, Some(HaltReason::SupervisorIntegrity));
            }
        }
    }
}

#[test]
fn successful_run_never_authenticates_global_completion() {
    let mut ledger = RunLedger::new(vec![spec("first", true)]).unwrap();
    assert!(ledger.may_continue());
    ledger.record("first", outcome("passed")).unwrap();
    assert!(!ledger.may_continue());
    assert_eq!(ledger.records().len(), 1);
    assert_eq!(ledger.halt_reason(), None);
    let verdict = ledger.finish();
    assert!(verdict.checks_passed);
    assert!(verdict.checks_executed);
    assert!(!verdict.completion_verified);
}
