//! Explicit local execution. Imported reports and global completion are not trusted here.
use crate::check_verdict::{CheckSpec, HaltReason, RunLedger};
use crate::{check_inputs, execution_budget::Budget, execution_review, process_check};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Instant;

pub(crate) fn run(
    target: &Path,
    config: &str,
    settings: &str,
    approval: &str,
    allow_unsandboxed: bool,
) -> Result<Value, String> {
    let budget = Budget::new();
    let started_at_ms = process_check::now_ms();
    if !allow_unsandboxed {
        return Err(
            "checks: execution requires explicit --allow-unsandboxed acknowledgment".into(),
        );
    }
    let _cancellation = crate::execution_cancel::Guard::install()?;
    let review = execution_review::prepare_with_budget(target, config, settings, &budget)?;
    if review["approval_digest"].as_str() != Some(approval) {
        return Err(
            "checks: execution review is stale or does not match; prepare and review again".into(),
        );
    }
    let root = check_inputs::root(target)?;
    let review_ms = budget.elapsed_ms();
    sequence(
        &review,
        &budget,
        started_at_ms,
        review_ms,
        || {
            let current = execution_review::prepare_with_budget(&root, config, settings, &budget)?;
            if current["approval_digest"].as_str() != Some(approval) {
                return Err("checks: reviewed inputs changed".into());
            }
            Ok(())
        },
        |check, remaining| {
            let cwd = check_inputs::resolve(&root, check["cwd"].as_str().unwrap(), true)?;
            if !cwd.is_dir() {
                return Err("checks: working directory changed".into());
            }
            budget.check()?;
            let name = check["argv"][0].as_str().unwrap();
            let executable = review["tools"][name]["path"].as_str().unwrap();
            let args: Vec<String> = check["argv"]
                .as_array()
                .unwrap()
                .iter()
                .skip(1)
                .map(|arg| arg.as_str().unwrap().to_string())
                .collect();
            Ok(process_check::execute(
                executable,
                &args,
                &cwd,
                review["environment"].as_object().unwrap(),
                check["timeout_ms"]
                    .as_u64()
                    .unwrap()
                    .min(remaining)
                    .min(budget.remaining_ms()),
                check["max_output_bytes"].as_u64().unwrap() as usize,
            ))
        },
    )
}

fn deadline_reason() -> HaltReason {
    if crate::execution_cancel::cancelled() {
        HaltReason::SupervisorIntegrity
    } else {
        HaltReason::BudgetExhausted
    }
}

fn sequence(
    review: &Value,
    budget: &Budget,
    started_at_ms: u64,
    mut review_ms: u64,
    mut revalidate: impl FnMut() -> Result<(), String>,
    mut dispatch: impl FnMut(&Value, u64) -> Result<Value, String>,
) -> Result<Value, String> {
    let checks = review["plan"]["policy"]["checks"].as_array().unwrap();
    let specs = checks
        .iter()
        .map(|c| CheckSpec {
            id: c["id"].as_str().unwrap().to_string(),
            required: c["required"].as_bool().unwrap(),
        })
        .collect();
    let mut ledger = RunLedger::new(specs).map_err(|_| "checks: invalid reviewed sequence")?;
    for check in checks {
        if !ledger.may_continue() {
            break;
        }
        if budget.check().is_err() {
            ledger.stop(deadline_reason());
            break;
        }
        let t = Instant::now();
        let valid = revalidate();
        review_ms += t.elapsed().as_millis() as u64;
        if valid.is_err() {
            if budget.check().is_err() {
                ledger.stop(deadline_reason());
            }
            ledger.stop(HaltReason::InputRevalidation);
            break;
        }
        let remaining = budget.remaining_ms();
        if budget.check().is_err() || remaining == 0 {
            ledger.stop(deadline_reason());
            break;
        }
        let outcome = if !execution_review::supported() {
            process_check::empty("unsupported")
        } else {
            match dispatch(check, remaining) {
                Ok(outcome) => outcome,
                Err(_) => {
                    if budget.check().is_err() {
                        ledger.stop(deadline_reason());
                    }
                    ledger.stop(HaltReason::InputRevalidation);
                    break;
                }
            }
        };
        // Every owned outcome is retained BEFORE any later fallible revalidation.
        ledger
            .record(check["id"].as_str().unwrap(), outcome)
            .map_err(|_| "checks: internal outcome sequence error")?;
        if budget.check().is_err() {
            ledger.stop(deadline_reason());
        }
    }
    let t = Instant::now();
    let final_valid = budget
        .check()
        .and_then(|_| revalidate())
        .and_then(|_| budget.check());
    review_ms += t.elapsed().as_millis() as u64;
    if final_valid.is_err() {
        if budget.check().is_err() {
            ledger.stop(deadline_reason());
        }
        ledger.stop(HaltReason::InputRevalidation);
    }
    let verdict = ledger.finish();
    let results: Vec<Value> = checks.iter().zip(&verdict.records).map(|(check, record)| json!({
        "id":record.spec.id,"required":record.spec.required,"argv":check["argv"],"cwd":check["cwd"],
        "reason":record.skipped_because.map(|why| format!("{why:?}")),
        "outcome":record.outcome.clone().unwrap_or_else(|| process_check::empty("skipped"))
    })).collect();
    let late = budget.check().is_err();
    let halt = verdict.halt_reason.or_else(|| late.then(deadline_reason));
    Ok(json!({
        "format_version":1,"kind":"check-run","review":review,
        "approval_matched":true,"unsandboxed_acknowledged":true,
        "checks_executed":verdict.checks_executed,"checks_passed":verdict.checks_passed && !late,"halt_reason":halt.map(|why| format!("{why:?}")),
        "inputs_current":verdict.inputs_current,"completion_verified":false,
        "started_at_ms":started_at_ms,"ended_at_ms":process_check::now_ms(),
        "duration_ms":budget.elapsed_ms(),"results":results,
        "timing":{"review_ms":review_ms,"scope":"invocation-through-final-revalidation","cleanup_grace_ms":250,"recovery_grace_ms":250},
        "governance_controls":review["plan"]["control_requirements"],
        "not_checked":["trusted imported evidence","freshness after this invocation",
            "tool runtime versions","transitive executable identity","whole-project readiness",
            "filesystem or network sandboxing","descendants that deliberately escape the process group",
            "hard interruption of blocking OS I/O and scheduling","cleanup after SIGKILL or supervisor crash"]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn review() -> Value {
        json!({"plan":{"policy":{"checks":[
            {"id":"required","required":true,"argv":["fixture"],"cwd":"."},
            {"id":"optional","required":false,"argv":["fixture"],"cwd":"."},
            {"id":"later","required":false,"argv":["fixture"],"cwd":"."}
        ]},"control_requirements":[]}})
    }
    fn passed() -> Value {
        let stream = json!({"observed_bytes":0,"sha256":check_inputs::hash(b""),"complete":true,"hash_scope":"complete-stream"});
        let mut result = process_check::empty("passed");
        result["spawned"] = json!(true);
        result["direct_child_reaped"] = json!(true);
        result["process_group_cleanup"] = json!("signal-sent-or-group-absent");
        result["exit_code"] = json!(0);
        result["stdout"] = stream.clone();
        result["stderr"] = stream;
        result
    }
    #[test]
    fn optional_supervisor_fault_halts_real_dispatch_loop() {
        if !execution_review::supported() {
            return;
        }
        for fault in ["cleanup", "reaping", "capture", "setup"] {
            let budget = Budget::new();
            let mut calls = 0;
            let result = sequence(
                &review(),
                &budget,
                0,
                0,
                || Ok(()),
                |_, _| {
                    calls += 1;
                    let mut value = passed();
                    if calls == 2 {
                        match fault {
                            "cleanup" => value["process_group_cleanup"] = json!("failed"),
                            "reaping" => value["direct_child_reaped"] = json!(false),
                            "capture" => value["stdout"] = Value::Null,
                            _ => value = process_check::empty("execution-error"),
                        }
                    }
                    Ok(value)
                },
            )
            .unwrap();
            assert_eq!(calls, 2, "{fault}");
            assert_eq!(result["checks_passed"], false);
            assert_eq!(result["results"][0]["outcome"]["status"], "passed");
            assert_eq!(result["results"][2]["reason"], "SupervisorIntegrity");
        }
    }
    #[test]
    fn later_setup_error_preserves_prior_observations() {
        if !execution_review::supported() {
            return;
        }
        let mut calls = 0;
        let result = sequence(
            &review(),
            &Budget::new(),
            0,
            0,
            || Ok(()),
            |_, _| {
                calls += 1;
                if calls == 2 {
                    Err("synthetic cwd error".into())
                } else {
                    Ok(passed())
                }
            },
        )
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(result["checks_passed"], false);
        assert_eq!(result["results"][0]["outcome"]["status"], "passed");
        assert_eq!(result["results"][1]["reason"], "InputRevalidation");
    }
    #[test]
    fn slow_review_and_final_revalidation_cannot_pass_or_dispatch_late() {
        if !execution_review::supported() {
            return;
        }
        for expire_at in [1, 4] {
            let budget = Budget::new();
            let mut reviews = 0;
            let mut calls = 0;
            let result = sequence(
                &review(),
                &budget,
                0,
                0,
                || {
                    reviews += 1;
                    if reviews == expire_at {
                        budget.limit(0)?;
                    }
                    Ok(())
                },
                |_, _| {
                    calls += 1;
                    Ok(passed())
                },
            )
            .unwrap();
            assert_eq!(calls, if expire_at == 1 { 0 } else { 3 });
            assert_eq!(result["checks_passed"], false);
            assert_eq!(result["inputs_current"], false);
        }
    }
}
