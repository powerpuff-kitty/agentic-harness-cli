//! Explicit local execution. Imported reports and global completion are not trusted here.
use crate::{check_inputs, execution_review, process_check};
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
    if !allow_unsandboxed {
        return Err("checks: execution requires explicit --allow-unsandboxed acknowledgment".into());
    }
    let review = execution_review::prepare(target, config, settings)?;
    if review["approval_digest"].as_str() != Some(approval) {
        return Err("checks: execution review is stale or does not match; prepare and review again".into());
    }
    let root = check_inputs::root(target)?;
    let start = Instant::now();
    let started_at_ms = process_check::now_ms();
    let mut inputs_current = true;
    let mut halted = false;
    let mut results = Vec::new();
    let budget = review["max_total_ms"].as_u64().unwrap();
    let checks = review["plan"]["policy"]["checks"].as_array().unwrap();
    let matches_review = || {
        execution_review::prepare(&root, config, settings)
            .is_ok_and(|current| current["approval_digest"].as_str() == Some(approval))
    };
    for check in checks {
        let mut reason = None;
        let outcome = if !execution_review::supported() {
            reason = Some("execution backend unavailable on this platform");
            process_check::empty("unsupported")
        } else if halted {
            reason = Some("execution stopped after invalidated inputs or exhausted budget");
            process_check::empty("skipped")
        } else if !matches_review() {
            inputs_current = false;
            halted = true;
            reason = Some("source, policy, tool or settings identity changed before execution");
            process_check::empty("skipped")
        } else {
            let elapsed = start.elapsed().as_millis() as u64;
            if elapsed >= budget {
                halted = true;
                reason = Some("total execution budget exhausted before this check");
                process_check::empty("skipped")
            } else {
                let cwd = check_inputs::resolve(&root, check["cwd"].as_str().unwrap(), true)?;
                let name = check["argv"][0].as_str().unwrap();
                let executable = review["tools"][name]["path"].as_str().unwrap();
                let args: Vec<String> = check["argv"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .skip(1)
                    .map(|arg| arg.as_str().unwrap().to_string())
                    .collect();
                let outcome = process_check::execute(
                    executable,
                    &args,
                    &cwd,
                    review["environment"].as_object().unwrap(),
                    check["timeout_ms"].as_u64().unwrap().min(budget - elapsed),
                    check["max_output_bytes"].as_u64().unwrap() as usize,
                );
                if !matches_review() {
                    inputs_current = false;
                    halted = true;
                }
                outcome
            }
        };
        results.push(json!({
            "id":check["id"],"required":check["required"],"argv":check["argv"],
            "cwd":check["cwd"],"reason":reason,"outcome":outcome
        }));
    }
    inputs_current &= matches_review();
    let checks_passed = execution_review::supported()
        && inputs_current
        && results.iter().filter(|result| result["required"] == true)
            .all(|result| result["outcome"]["status"] == "passed");
    let executed = results.iter().any(|result| result["outcome"]["spawned"] == true);
    Ok(json!({
        "format_version":1,"kind":"check-run","review":review,
        "approval_matched":true,"unsandboxed_acknowledged":true,
        "checks_executed":executed,"checks_passed":checks_passed,
        "inputs_current":inputs_current,"completion_verified":false,
        "started_at_ms":started_at_ms,"ended_at_ms":process_check::now_ms(),
        "duration_ms":start.elapsed().as_millis() as u64,"results":results,
        "governance_controls":review["plan"]["control_requirements"],
        "not_checked":["trusted imported evidence","freshness after this invocation",
            "tool runtime versions","transitive executable identity","whole-project readiness",
            "filesystem or network sandboxing","descendants that deliberately escape the process group"]
    }))
}
