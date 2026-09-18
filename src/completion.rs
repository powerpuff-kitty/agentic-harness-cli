//! Caller-approved, read-only completion evaluation. Never executes imported commands.
use crate::check_verdict::{CheckSpec, RunLedger};
use crate::{
    check_inputs, checks, execution_budget::Budget, execution_review,
    governance_verdict as governance, strict_json,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn error() -> String {
    "checks: evidence is malformed, incomplete, stale or does not match current inputs (contents omitted)".into()
}
fn number(v: &Value) -> Result<u64, String> {
    v.as_u64()
        .filter(|n| *n <= 9_007_199_254_740_991)
        .ok_or_else(error)
}
fn array(v: &Value, max: usize) -> Result<&Vec<Value>, String> {
    v.as_array().filter(|a| a.len() <= max).ok_or_else(error)
}
fn shape(v: &Value, required: &[&str], optional: &[&str]) -> Result<(), String> {
    let obj = v.as_object().ok_or_else(error)?;
    if required.iter().any(|k| !obj.contains_key(*k))
        || obj
            .keys()
            .any(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err(error());
    }
    Ok(())
}
fn clock() -> Result<u64, String> {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| error())?
        .as_millis();
    u64::try_from(n).map_err(|_| error())
}
fn identity(review: &Value) -> governance::CurrentIdentity {
    governance::CurrentIdentity {
        policy_digest: review["plan"]["policy_digest"].as_str().unwrap().into(),
        source_digest: review["plan"]["source_digest"].as_str().unwrap().into(),
        scope: review["plan"]["policy"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().into())
            .collect(),
    }
}
fn artifact(root: &Path, entry: &Value, budget: &Budget, limit: usize) -> Result<Vec<u8>, String> {
    let path = check_inputs::resolve(root, checks::text(&entry["path"], 512)?, false)?;
    let bytes = check_inputs::read_with_budget(&path, limit, budget)?;
    if entry["digest"] != check_inputs::hash(&bytes) {
        return Err(error());
    }
    Ok(bytes)
}
fn run_valid(run: &Value, review: &Value, now: u64) -> Result<(), String> {
    shape(
        run,
        &[
            "format_version",
            "kind",
            "review",
            "approval_matched",
            "unsandboxed_acknowledged",
            "checks_executed",
            "checks_passed",
            "inputs_current",
            "completion_verified",
            "started_at_ms",
            "ended_at_ms",
            "duration_ms",
            "results",
            "governance_controls",
            "not_checked",
        ],
        &["timing", "halt_reason"],
    )?;
    if run["format_version"].as_u64() != Some(1)
        || run["kind"] != "check-run"
        || run["review"] != *review
        || [
            "approval_matched",
            "unsandboxed_acknowledged",
            "checks_executed",
            "checks_passed",
            "inputs_current",
        ]
        .iter()
        .any(|k| run[*k] != true)
        || run["completion_verified"] != false
        || !run["halt_reason"].is_null()
        || review["execution_supported"] != true
        || run["governance_controls"] != review["plan"]["control_requirements"]
    {
        return Err(error());
    }
    let start = number(&run["started_at_ms"])?;
    let end = number(&run["ended_at_ms"])?;
    let age = review["plan"]["policy"]["max_age_ms"].as_u64().unwrap();
    if start > end
        || end > now
        || now - start > age
        || number(&run["duration_ms"])? > review["max_total_ms"].as_u64().unwrap()
    {
        return Err(error());
    }
    let notes = array(&run["not_checked"], 64)?;
    if notes.is_empty() {
        return Err(error());
    }
    for note in notes {
        checks::text(note, 4096)?;
    }
    if let Some(t) = run.get("timing") {
        shape(
            t,
            &[
                "review_ms",
                "scope",
                "cleanup_grace_ms",
                "recovery_grace_ms",
            ],
            &[],
        )?;
        number(&t["review_ms"])?;
        if t["scope"] != "invocation-through-final-revalidation"
            || t["cleanup_grace_ms"] != 250
            || t["recovery_grace_ms"] != 250
        {
            return Err(error());
        }
    }
    let checks = review["plan"]["policy"]["checks"].as_array().unwrap();
    let results = array(&run["results"], 64)?;
    if checks.len() != results.len() {
        return Err(error());
    }
    let mut ledger = RunLedger::new(
        checks
            .iter()
            .map(|c| CheckSpec {
                id: c["id"].as_str().unwrap().into(),
                required: c["required"].as_bool().unwrap(),
            })
            .collect(),
    )
    .map_err(|_| error())?;
    let mut previous = start;
    for (check, result) in checks.iter().zip(results) {
        shape(
            result,
            &["id", "required", "argv", "cwd", "reason", "outcome"],
            &[],
        )?;
        if ["id", "required", "argv", "cwd"]
            .iter()
            .any(|k| result[*k] != check[*k])
            || !result["reason"].is_null()
        {
            return Err(error());
        }
        let o = &result["outcome"];
        shape(
            o,
            &[
                "status",
                "started_at_ms",
                "ended_at_ms",
                "duration_ms",
                "exit_code",
                "signal",
                "spawned",
                "direct_child_reaped",
                "process_group_cleanup",
                "output_disclosure",
                "stdout",
                "stderr",
            ],
            &["timing"],
        )?;
        let s = number(&o["started_at_ms"])?;
        let e = number(&o["ended_at_ms"])?;
        if s < previous || e < s || e > end {
            return Err(error());
        }
        previous = e;
        let duration = number(&o["duration_ms"])?;
        if o["status"] == "passed" && duration > check["timeout_ms"].as_u64().unwrap() {
            return Err(error());
        }
        if let Some(t) = o.get("timing") {
            shape(t, &["execution_ms", "cleanup_ms", "recovery_ms"], &[])?;
            for k in ["execution_ms", "cleanup_ms", "recovery_ms"] {
                number(&t[k])?;
            }
        }
        for stream in ["stdout", "stderr"] {
            shape(
                &o[stream],
                &["observed_bytes", "sha256", "complete", "hash_scope"],
                &[],
            )?;
        }
        let observed = number(&o["stdout"]["observed_bytes"])?
            .checked_add(number(&o["stderr"]["observed_bytes"])?)
            .ok_or_else(error)?;
        if observed > check["max_output_bytes"].as_u64().unwrap() + 1 {
            return Err(error());
        }
        ledger
            .record(check["id"].as_str().unwrap(), o.clone())
            .map_err(|_| error())?;
    }
    if !ledger.finish().checks_passed {
        return Err(error());
    }
    Ok(())
}

pub(crate) fn evaluate(
    target: &Path,
    config: &str,
    settings: &str,
    bundle_path: &str,
    approval: &str,
) -> Result<Value, String> {
    let budget = Budget::new();
    let started = clock()?;
    let root = check_inputs::root(target)?;
    let path = check_inputs::resolve(&root, bundle_path, false)?;
    let bytes = check_inputs::read_with_budget(&path, 262_144, &budget)?;
    let bundle_digest = check_inputs::hash(&bytes);
    // Approval precedes following any report or reference path. There is no stored/default trust.
    if bundle_digest != approval {
        return Err(
            "checks: --approve-evidence must match the independently reviewed evidence manifest"
                .into(),
        );
    }
    let bundle = strict_json::decode(&bytes)?;
    shape(
        &bundle,
        &["format_version", "kind", "run", "governance", "references"],
        &[],
    )?;
    if bundle["format_version"].as_u64() != Some(1) || bundle["kind"] != "check-evidence-manifest" {
        return Err(error());
    }
    shape(&bundle["run"], &["path", "digest"], &[])?;
    let governance = array(&bundle["governance"], 64)?;
    let references = array(&bundle["references"], 64)?;
    let before = execution_review::prepare_with_budget(&root, config, settings, &budget)?;
    let run_bytes = artifact(&root, &bundle["run"], &budget, 2_000_000)?;
    let run = strict_json::decode(&run_bytes)?;
    let mut reports = Vec::new();
    let mut trust = Vec::new();
    let mut refs = Vec::new();
    let mut total = run_bytes.len();
    let mut paths = BTreeSet::new();
    paths.insert(bundle_path.to_string());
    let mut snapshots = vec![(bundle["run"].clone(), run_bytes)];
    for report in governance {
        shape(report, &["path", "digest", "producer"], &[])?;
        shape(&report["producer"], &["id", "version"], &[])?;
        let bytes = artifact(&root, report, &budget, 1_048_576)?;
        total += bytes.len();
        if total > 10_388_608 {
            return Err(error());
        }
        trust.push(governance::TrustedReport {
            digest: checks::text(&report["digest"], 71)?.into(),
            producer_id: checks::text(&report["producer"]["id"], 128)?.into(),
            producer_version: checks::text(&report["producer"]["version"], 4096)?.into(),
        });
        reports.push(bytes.clone());
        snapshots.push((report.clone(), bytes));
    }
    let mut ref_bytes = 0;
    for reference in references {
        shape(reference, &["name", "path", "digest"], &[])?;
        let bytes = artifact(&root, reference, &budget, 2_000_000)?;
        ref_bytes += bytes.len();
        if ref_bytes > 8_388_608 {
            return Err(error());
        }
        refs.push(governance::Reference {
            name: checks::text(&reference["name"], 4096)?.into(),
            digest: checks::text(&reference["digest"], 71)?.into(),
            bytes: bytes.clone(),
        });
        snapshots.push((reference.clone(), bytes));
    }
    for (entry, _) in &snapshots {
        if !paths.insert(checks::text(&entry["path"], 512)?.to_string()) {
            return Err(error());
        }
    }
    for (entry, original) in &snapshots {
        if artifact(&root, entry, &budget, 2_000_000)? != *original {
            return Err(error());
        }
    }
    if check_inputs::read_with_budget(&path, 262_144, &budget)? != bytes {
        return Err(error());
    }
    let after = execution_review::prepare_with_budget(&root, config, settings, &budget)?;
    if before != after {
        return Err(error());
    }
    let now = clock()?;
    if now < started {
        return Err(error());
    }
    run_valid(&run, &after, now)?;
    let context = governance::Context {
        before: identity(&before),
        after: identity(&after),
        required: after["plan"]["policy"]["required_controls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["rule_id"].as_str().unwrap().into(),
                    r["capability"].as_str().unwrap().into(),
                )
            })
            .collect(),
        max_age_ms: after["plan"]["policy"]["max_age_ms"].as_u64().unwrap(),
        now_ms: now,
        trusted_reports: trust,
        references: refs,
    };
    governance::evaluate(&context, &reports).map_err(|_| error())?;
    budget.check()?;
    Ok(
        json!({"format_version":1,"kind":"check-completion","completion_verified":true,"scope":"declared-checks-and-required-controls","trust":"caller-approved-exact-evidence","evidence_digest":bundle_digest,"run_digest":bundle["run"]["digest"],"policy_digest":context.after.policy_digest,"source_digest":context.after.source_digest,"evaluated_at_ms":now,"checks_passed":true,"required_controls_satisfied":true,"producer_authenticated":false,"not_checked":["signed producer authentication","whole-project readiness","undeclared inputs and transitive dependencies","mutations reverted between snapshots","freshness after this evaluation","independent verification of producer assertions"]}),
    )
}
