//! Provider-neutral Decision Kernel runtime helpers.
//!
//! This slice is deliberately offline: it validates the pinned canonical
//! contracts, fingerprints explicit state, and constructs a TypeSafe Jev
//! HTTP payload without performing a network call or reading credentials.
use crate::check_inputs;
use include_dir::{Dir, include_dir};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

static SCHEMAS: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/schema");

const LIMIT: usize = 2_000_000;
const JEV_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const DECISION_KINDS: &[&str] = &[
    "boolean",
    "choice",
    "ordinal",
    "ranking",
    "estimate",
    "distribution",
    "extraction",
    "constraint",
    "optimization",
];

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = check_inputs::read(path, LIMIT)
        .map_err(|error| format!("decisions: {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("decisions: {} is not valid JSON: {error}", path.display()))
}

fn object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("decisions: {context} must be an object"))
}

fn array<'a>(value: &'a Value, context: &str, min: usize) -> Result<&'a Vec<Value>, String> {
    let values = value
        .as_array()
        .ok_or_else(|| format!("decisions: {context} must be an array"))?;
    if values.len() < min {
        return Err(format!("decisions: {context} must contain at least {min} item(s)"));
    }
    Ok(values)
}

fn required<'a>(
    value: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a Value, String> {
    value
        .get(key)
        .ok_or_else(|| format!("decisions: {context}.{key} is required"))
}

fn text<'a>(value: &'a Value, context: &str) -> Result<&'a str, String> {
    let value = value
        .as_str()
        .ok_or_else(|| format!("decisions: {context} must be a string"))?;
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(format!("decisions: {context} is empty, oversized or contains control characters"));
    }
    Ok(value)
}

fn positive_revision(value: &Value, context: &str) -> Result<u64, String> {
    let value = value
        .as_u64()
        .ok_or_else(|| format!("decisions: {context} must be a positive integer"))?;
    if value == 0 {
        return Err(format!("decisions: {context} must be a positive integer"));
    }
    Ok(value)
}

fn probability(value: &Value, context: &str) -> Result<f64, String> {
    let value = value
        .as_f64()
        .ok_or_else(|| format!("decisions: {context} must be a number"))?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(format!("decisions: {context} must be between 0 and 1"));
    }
    Ok(value)
}

fn optional_probability(value: &Value, context: &str) -> Result<Option<f64>, String> {
    if value.is_null() {
        Ok(None)
    } else {
        probability(value, context).map(Some)
    }
}

fn enum_text<'a>(
    value: &'a Value,
    context: &str,
    allowed: &[&str],
) -> Result<&'a str, String> {
    let value = text(value, context)?;
    if !allowed.contains(&value) {
        return Err(format!("decisions: unsupported {context}: {value}"));
    }
    Ok(value)
}

fn version(value: &Value, context: &str) -> Result<(), String> {
    if value.as_u64().is_some_and(|revision| revision > 0)
        || value.as_str().is_some_and(|revision| !revision.is_empty())
    {
        Ok(())
    } else {
        Err(format!("decisions: {context} must be a non-empty version"))
    }
}

fn base<'a>(value: &'a Value, expected_kind: &str) -> Result<&'a Map<String, Value>, String> {
    let object = object(value, expected_kind)?;
    if required(object, "format_version", expected_kind)?.as_u64() != Some(1) {
        return Err(format!("decisions: unsupported {expected_kind} format_version"));
    }
    if required(object, "kind", expected_kind)?.as_str() != Some(expected_kind) {
        return Err(format!("decisions: expected kind {expected_kind}"));
    }
    Ok(object)
}

fn unique_text_array(value: &Value, context: &str, min: usize) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in array(value, context, min)? {
        let item = text(item, context)?.to_string();
        if !seen.insert(item.clone()) {
            return Err(format!("decisions: duplicate value in {context}: {item}"));
        }
        out.push(item);
    }
    Ok(out)
}

fn validate_spec(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-spec")?;
    text(required(root, "id", "decision-spec")?, "decision-spec.id")?;
    positive_revision(required(root, "revision", "decision-spec")?, "decision-spec.revision")?;
    let decision_kind = enum_text(
        required(root, "decision_kind", "decision-spec")?,
        "decision-spec.decision_kind",
        DECISION_KINDS,
    )?;
    text(
        required(root, "description", "decision-spec")?,
        "decision-spec.description",
    )?;

    let input = object(required(root, "input", "decision-spec")?, "decision-spec.input")?;
    text(required(input, "schema_id", "decision-spec.input")?, "decision-spec.input.schema_id")?;
    version(
        required(input, "schema_version", "decision-spec.input")?,
        "decision-spec.input.schema_version",
    )?;
    if let Some(required_snapshot) = input.get("immutable_snapshot_required") {
        required_snapshot
            .as_bool()
            .ok_or("decisions: decision-spec.input.immutable_snapshot_required must be boolean")?;
    }

    match decision_kind {
        "choice" => {
            unique_text_array(
                required(root, "options", "decision-spec")?,
                "decision-spec.options",
                2,
            )?;
        }
        "ordinal" => {
            unique_text_array(
                required(root, "levels", "decision-spec")?,
                "decision-spec.levels",
                2,
            )?;
        }
        _ => {}
    }

    let evidence = object(
        required(root, "evidence", "decision-spec")?,
        "decision-spec.evidence",
    )?;
    let mut evidence_ids = BTreeSet::new();
    for requirement in array(
        required(evidence, "requirements", "decision-spec.evidence")?,
        "decision-spec.evidence.requirements",
        0,
    )? {
        let requirement = object(requirement, "decision-spec.evidence.requirement")?;
        let id = text(
            required(requirement, "id", "decision-spec.evidence.requirement")?,
            "decision-spec.evidence.requirement.id",
        )?;
        if !evidence_ids.insert(id.to_string()) {
            return Err(format!("decisions: duplicate evidence requirement id: {id}"));
        }
        required(requirement, "required", "decision-spec.evidence.requirement")?
            .as_bool()
            .ok_or("decisions: evidence requirement required must be boolean")?;
        text(
            required(requirement, "description", "decision-spec.evidence.requirement")?,
            "decision-spec.evidence.requirement.description",
        )?;
    }

    if let Some(uncertainty) = root.get("uncertainty") {
        let uncertainty = object(uncertainty, "decision-spec.uncertainty")?;
        if let Some(value) = uncertainty.get("minimum_evidence_coverage") {
            probability(value, "decision-spec.uncertainty.minimum_evidence_coverage")?;
        }
        for key in ["allow_abstain", "require_calibrated_confidence"] {
            if let Some(value) = uncertainty.get(key) {
                value
                    .as_bool()
                    .ok_or_else(|| format!("decisions: decision-spec.uncertainty.{key} must be boolean"))?;
            }
        }
    }

    let policy = object(required(root, "policy", "decision-spec")?, "decision-spec.policy")?;
    enum_text(
        required(policy, "risk", "decision-spec.policy")?,
        "decision-spec.policy.risk",
        &["low", "medium", "high", "critical"],
    )?;
    enum_text(
        required(policy, "consequential_action", "decision-spec.policy")?,
        "decision-spec.policy.consequential_action",
        &["forbidden", "review-required", "policy-gated"],
    )?;
    Ok(())
}

fn validate_graph(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-graph")?;
    text(required(root, "id", "decision-graph")?, "decision-graph.id")?;
    positive_revision(required(root, "revision", "decision-graph")?, "decision-graph.revision")?;
    let nodes = array(required(root, "nodes", "decision-graph")?, "decision-graph.nodes", 1)?;
    let mut ids = BTreeSet::new();
    let mut dependencies = BTreeMap::<String, Vec<String>>::new();

    for node in nodes {
        let node = object(node, "decision-graph.node")?;
        let id = text(required(node, "id", "decision-graph.node")?, "decision-graph.node.id")?;
        if !ids.insert(id.to_string()) {
            return Err(format!("decisions: duplicate decision graph node id: {id}"));
        }
        text(required(node, "spec_id", "decision-graph.node")?, "decision-graph.node.spec_id")?;
        positive_revision(
            required(node, "spec_revision", "decision-graph.node")?,
            "decision-graph.node.spec_revision",
        )?;
        dependencies.insert(
            id.to_string(),
            unique_text_array(
                required(node, "depends_on", "decision-graph.node")?,
                "decision-graph.node.depends_on",
                0,
            )?,
        );
    }

    for (node, deps) in &dependencies {
        for dep in deps {
            if dep == node {
                return Err(format!("decisions: decision graph self dependency: {node}"));
            }
            if !ids.contains(dep) {
                return Err(format!("decisions: decision graph dependency is unknown: {dep}"));
            }
        }
    }

    fn visit(
        node: &str,
        dependencies: &BTreeMap<String, Vec<String>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if visiting.contains(node) {
            return Err(format!("decisions: decision graph cycle detected at {node}"));
        }
        if visited.contains(node) {
            return Ok(());
        }
        visiting.insert(node.to_string());
        if let Some(deps) = dependencies.get(node) {
            for dep in deps {
                visit(dep, dependencies, visiting, visited)?;
            }
        }
        visiting.remove(node);
        visited.insert(node.to_string());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in &ids {
        visit(node, &dependencies, &mut visiting, &mut visited)?;
    }

    if let Some(reducers) = root.get("reducers") {
        let mut reducer_ids = BTreeSet::new();
        for reducer in array(reducers, "decision-graph.reducers", 0)? {
            let reducer = object(reducer, "decision-graph.reducer")?;
            let id = text(
                required(reducer, "id", "decision-graph.reducer")?,
                "decision-graph.reducer.id",
            )?;
            if !reducer_ids.insert(id.to_string()) {
                return Err(format!("decisions: duplicate reducer id: {id}"));
            }
            if required(reducer, "type", "decision-graph.reducer")?.as_str()
                != Some("deterministic")
            {
                return Err("decisions: decision graph reducers must be deterministic".into());
            }
            for input in unique_text_array(
                required(reducer, "inputs", "decision-graph.reducer")?,
                "decision-graph.reducer.inputs",
                1,
            )? {
                if !ids.contains(&input) {
                    return Err(format!("decisions: reducer input is not a decision node: {input}"));
                }
            }
        }
    }
    Ok(())
}

fn validate_request(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-request")?;
    text(required(root, "request_id", "decision-request")?, "decision-request.request_id")?;
    enum_text(
        required(root, "mode", "decision-request")?,
        "decision-request.mode",
        &["live", "shadow", "replay", "evaluation"],
    )?;
    let state = object(required(root, "state", "decision-request")?, "decision-request.state")?;
    text(required(state, "schema_id", "decision-request.state")?, "decision-request.state.schema_id")?;
    version(
        required(state, "schema_version", "decision-request.state")?,
        "decision-request.state.schema_version",
    )?;
    if text(
        required(state, "fingerprint", "decision-request.state")?,
        "decision-request.state.fingerprint",
    )?
    .len()
        < 8
    {
        return Err("decisions: decision-request.state.fingerprint is too short".into());
    }
    if let Some(payload) = state.get("payload") {
        object(payload, "decision-request.state.payload")?;
    }
    for question in array(
        required(root, "questions", "decision-request")?,
        "decision-request.questions",
        1,
    )? {
        let question = object(question, "decision-request.question")?;
        text(
            required(question, "spec_id", "decision-request.question")?,
            "decision-request.question.spec_id",
        )?;
        positive_revision(
            required(question, "spec_revision", "decision-request.question")?,
            "decision-request.question.spec_revision",
        )?;
    }
    Ok(())
}

fn validate_provider(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-provider-profile")?;
    text(required(root, "id", "decision-provider-profile")?, "decision-provider-profile.id")?;
    positive_revision(
        required(root, "revision", "decision-provider-profile")?,
        "decision-provider-profile.revision",
    )?;
    enum_text(
        required(root, "provider_type", "decision-provider-profile")?,
        "decision-provider-profile.provider_type",
        &["deterministic", "jev", "llm", "ml", "human", "custom"],
    )?;
    let kinds = unique_text_array(
        required(root, "decision_kinds", "decision-provider-profile")?,
        "decision-provider-profile.decision_kinds",
        1,
    )?;
    for kind in kinds {
        if !DECISION_KINDS.contains(&kind.as_str()) {
            return Err(format!("decisions: unsupported provider decision kind: {kind}"));
        }
    }
    let confidence = object(
        required(root, "confidence", "decision-provider-profile")?,
        "decision-provider-profile.confidence",
    )?;
    enum_text(
        required(confidence, "semantics", "decision-provider-profile.confidence")?,
        "decision-provider-profile.confidence.semantics",
        &[
            "calibrated-probability",
            "uncalibrated-score",
            "self-report",
            "none",
            "provider-defined",
        ],
    )?;
    required(
        confidence,
        "supports_distribution",
        "decision-provider-profile.confidence",
    )?
    .as_bool()
    .ok_or("decisions: supports_distribution must be boolean")?;
    for key in ["external_network", "requires_credentials"] {
        required(root, key, "decision-provider-profile")?
            .as_bool()
            .ok_or_else(|| format!("decisions: decision-provider-profile.{key} must be boolean"))?;
    }
    if required(root, "consequence_authority", "decision-provider-profile")?.as_bool()
        != Some(false)
    {
        return Err("decisions: providers cannot have consequence authority".into());
    }
    Ok(())
}

fn validate_policy(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-policy")?;
    text(required(root, "id", "decision-policy")?, "decision-policy.id")?;
    positive_revision(required(root, "revision", "decision-policy")?, "decision-policy.revision")?;
    unique_text_array(
        required(root, "applies_to", "decision-policy")?,
        "decision-policy.applies_to",
        1,
    )?;
    enum_text(
        required(root, "risk", "decision-policy")?,
        "decision-policy.risk",
        &["low", "medium", "high", "critical"],
    )?;
    let evidence = object(required(root, "evidence", "decision-policy")?, "decision-policy.evidence")?;
    probability(
        required(evidence, "minimum_coverage", "decision-policy.evidence")?,
        "decision-policy.evidence.minimum_coverage",
    )?;
    enum_text(
        required(evidence, "on_missing_required", "decision-policy.evidence")?,
        "decision-policy.evidence.on_missing_required",
        &["review", "abstain", "reject"],
    )?;

    let confidence = object(
        required(root, "confidence", "decision-policy")?,
        "decision-policy.confidence",
    )?;
    required(confidence, "calibration_required", "decision-policy.confidence")?
        .as_bool()
        .ok_or("decisions: decision-policy.confidence.calibration_required must be boolean")?;
    optional_probability(
        required(
            confidence,
            "minimum_provider_confidence",
            "decision-policy.confidence",
        )?,
        "decision-policy.confidence.minimum_provider_confidence",
    )?;
    optional_probability(
        required(
            confidence,
            "minimum_decision_certainty",
            "decision-policy.confidence",
        )?,
        "decision-policy.confidence.minimum_decision_certainty",
    )?;

    let dispositions = object(
        required(root, "dispositions", "decision-policy")?,
        "decision-policy.dispositions",
    )?;
    enum_text(
        required(dispositions, "success", "decision-policy.dispositions")?,
        "decision-policy.dispositions.success",
        &["accepted", "review", "abstained", "rejected"],
    )?;
    for key in [
        "low_confidence",
        "insufficient_evidence",
        "conflict",
        "provider_failure",
    ] {
        enum_text(
            required(dispositions, key, "decision-policy.dispositions")?,
            &format!("decision-policy.dispositions.{key}"),
            &["review", "abstained", "rejected"],
        )?;
    }
    enum_text(
        required(root, "consequential_action", "decision-policy")?,
        "decision-policy.consequential_action",
        &[
            "forbidden",
            "review-required",
            "separate-authorization-required",
        ],
    )?;

    let mut invariant_ids = BTreeSet::new();
    for invariant in array(
        required(root, "invariants", "decision-policy")?,
        "decision-policy.invariants",
        1,
    )? {
        let invariant = object(invariant, "decision-policy.invariant")?;
        let id = text(
            required(invariant, "id", "decision-policy.invariant")?,
            "decision-policy.invariant.id",
        )?;
        if !invariant_ids.insert(id.to_string()) {
            return Err(format!("decisions: duplicate invariant id: {id}"));
        }
        text(
            required(invariant, "description", "decision-policy.invariant")?,
            "decision-policy.invariant.description",
        )?;
        if required(invariant, "enforcement", "decision-policy.invariant")?.as_str()
            != Some("deterministic")
        {
            return Err("decisions: policy invariants must declare deterministic enforcement".into());
        }
    }
    Ok(())
}

fn validate_distribution(value: &Value, context: &str) -> Result<(), String> {
    let values = object(value, context)?;
    if values.is_empty() {
        return Err(format!("decisions: {context} cannot be empty"));
    }
    let mut total = 0.0;
    for (key, value) in values {
        total += probability(value, &format!("{context}.{key}"))?;
    }
    if (total - 1.0).abs() > 1e-6 {
        return Err(format!("decisions: {context} probabilities must sum to 1"));
    }
    Ok(())
}

fn validate_receipt(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-receipt")?;
    if root.contains_key("outcome_probability") {
        return Err("decisions: outcome_probability is not a DecisionReceipt field".into());
    }
    text(required(root, "id", "decision-receipt")?, "decision-receipt.id")?;
    let spec = object(required(root, "spec", "decision-receipt")?, "decision-receipt.spec")?;
    text(required(spec, "id", "decision-receipt.spec")?, "decision-receipt.spec.id")?;
    positive_revision(
        required(spec, "revision", "decision-receipt.spec")?,
        "decision-receipt.spec.revision",
    )?;
    let state = object(required(root, "state", "decision-receipt")?, "decision-receipt.state")?;
    text(required(state, "schema_id", "decision-receipt.state")?, "decision-receipt.state.schema_id")?;
    version(
        required(state, "schema_version", "decision-receipt.state")?,
        "decision-receipt.state.schema_version",
    )?;
    text(
        required(state, "fingerprint", "decision-receipt.state")?,
        "decision-receipt.state.fingerprint",
    )?;
    let status = enum_text(
        required(root, "status", "decision-receipt")?,
        "decision-receipt.status",
        &[
            "produced",
            "unknown",
            "insufficient-evidence",
            "conflicting-evidence",
            "out-of-distribution",
            "abstained",
            "provider-failure",
        ],
    )?;
    if status == "produced" && root.get("result").is_none() {
        return Err("decisions: produced receipt requires result".into());
    }
    if let Some(result) = root.get("result") {
        let result = object(result, "decision-receipt.result")?;
        if result.contains_key("outcome_probability") {
            return Err("decisions: outcome_probability cannot be stored inside result".into());
        }
        if let Some(distribution) = result.get("distribution") {
            validate_distribution(distribution, "decision-receipt.result.distribution")?;
        }
    }

    let provider = object(
        required(root, "provider", "decision-receipt")?,
        "decision-receipt.provider",
    )?;
    enum_text(
        required(provider, "type", "decision-receipt.provider")?,
        "decision-receipt.provider.type",
        &["deterministic", "jev", "llm", "ml", "human", "custom"],
    )?;
    text(required(provider, "id", "decision-receipt.provider")?, "decision-receipt.provider.id")?;

    let uncertainty = object(
        required(root, "uncertainty", "decision-receipt")?,
        "decision-receipt.uncertainty",
    )?;
    if uncertainty.contains_key("outcome_probability") {
        return Err("decisions: outcome_probability cannot be stored as uncertainty".into());
    }
    optional_probability(
        required(uncertainty, "provider_confidence", "decision-receipt.uncertainty")?,
        "decision-receipt.uncertainty.provider_confidence",
    )?;
    optional_probability(
        required(uncertainty, "evidence_reliability", "decision-receipt.uncertainty")?,
        "decision-receipt.uncertainty.evidence_reliability",
    )?;
    optional_probability(
        required(uncertainty, "decision_certainty", "decision-receipt.uncertainty")?,
        "decision-receipt.uncertainty.decision_certainty",
    )?;
    let calibration = object(
        required(uncertainty, "calibration", "decision-receipt.uncertainty")?,
        "decision-receipt.uncertainty.calibration",
    )?;
    enum_text(
        required(calibration, "status", "decision-receipt.uncertainty.calibration")?,
        "decision-receipt.uncertainty.calibration.status",
        &["calibrated", "uncalibrated", "unknown", "not-applicable"],
    )?;

    let coverage = object(
        required(
            uncertainty,
            "evidence_coverage",
            "decision-receipt.uncertainty",
        )?,
        "decision-receipt.uncertainty.evidence_coverage",
    )?;
    let value = probability(
        required(
            coverage,
            "value",
            "decision-receipt.uncertainty.evidence_coverage",
        )?,
        "decision-receipt.uncertainty.evidence_coverage.value",
    )?;
    let present = required(
        coverage,
        "required_present",
        "decision-receipt.uncertainty.evidence_coverage",
    )?
    .as_u64()
    .ok_or("decisions: required_present must be an unsigned integer")?;
    let total = required(
        coverage,
        "required_total",
        "decision-receipt.uncertainty.evidence_coverage",
    )?
    .as_u64()
    .ok_or("decisions: required_total must be an unsigned integer")?;
    if present > total {
        return Err("decisions: required_present exceeds required_total".into());
    }
    let missing = unique_text_array(
        required(
            coverage,
            "missing",
            "decision-receipt.uncertainty.evidence_coverage",
        )?,
        "decision-receipt.uncertainty.evidence_coverage.missing",
        0,
    )?;
    if missing.len() as u64 != total - present {
        return Err("decisions: evidence coverage missing-count mismatch".into());
    }
    let expected = if total == 0 {
        1.0
    } else {
        present as f64 / total as f64
    };
    if (value - expected).abs() > 1e-9 {
        return Err("decisions: evidence coverage arithmetic mismatch".into());
    }

    let policy = object(required(root, "policy", "decision-receipt")?, "decision-receipt.policy")?;
    text(required(policy, "id", "decision-receipt.policy")?, "decision-receipt.policy.id")?;
    positive_revision(
        required(policy, "revision", "decision-receipt.policy")?,
        "decision-receipt.policy.revision",
    )?;
    enum_text(
        required(policy, "disposition", "decision-receipt.policy")?,
        "decision-receipt.policy.disposition",
        &["accepted", "review", "abstained", "rejected"],
    )?;
    Ok(())
}

fn validate_outcome(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-outcome")?;
    text(required(root, "id", "decision-outcome")?, "decision-outcome.id")?;
    text(
        required(root, "receipt_id", "decision-outcome")?,
        "decision-outcome.receipt_id",
    )?;
    text(
        required(root, "observed_at", "decision-outcome")?,
        "decision-outcome.observed_at",
    )?;
    let outcome = object(required(root, "outcome", "decision-outcome")?, "decision-outcome.outcome")?;
    text(required(outcome, "label", "decision-outcome.outcome")?, "decision-outcome.outcome.label")?;
    let verification = object(
        required(root, "verification", "decision-outcome")?,
        "decision-outcome.verification",
    )?;
    enum_text(
        required(verification, "type", "decision-outcome.verification")?,
        "decision-outcome.verification.type",
        &["human", "deterministic", "external-authority", "measurement", "unknown"],
    )?;
    let feedback = object(required(root, "feedback", "decision-outcome")?, "decision-outcome.feedback")?;
    required(feedback, "usable_for_evaluation", "decision-outcome.feedback")?
        .as_bool()
        .ok_or("decisions: decision-outcome.feedback.usable_for_evaluation must be boolean")?;
    Ok(())
}

fn validate_evaluation(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-evaluation")?;
    text(required(root, "id", "decision-evaluation")?, "decision-evaluation.id")?;
    let mode = enum_text(
        required(root, "mode", "decision-evaluation")?,
        "decision-evaluation.mode",
        &["offline", "replay", "shadow", "champion-challenger", "counterfactual"],
    )?;
    if required(root, "side_effects", "decision-evaluation")?.as_bool() != Some(false) {
        return Err("decisions: evaluation side effects are forbidden".into());
    }
    let dataset = object(
        required(root, "dataset", "decision-evaluation")?,
        "decision-evaluation.dataset",
    )?;
    text(required(dataset, "id", "decision-evaluation.dataset")?, "decision-evaluation.dataset.id")?;
    version(
        required(dataset, "revision", "decision-evaluation.dataset")?,
        "decision-evaluation.dataset.revision",
    )?;
    let candidate = object(
        required(root, "candidate", "decision-evaluation")?,
        "decision-evaluation.candidate",
    )?;
    text(
        required(candidate, "provider_id", "decision-evaluation.candidate")?,
        "decision-evaluation.candidate.provider_id",
    )?;
    let coverage = object(
        required(root, "coverage", "decision-evaluation")?,
        "decision-evaluation.coverage",
    )?;
    let mut counts = [0_u64; 4];
    for (index, key) in ["total", "evaluated", "abstained", "failed"]
        .iter()
        .enumerate()
    {
        counts[index] = required(coverage, key, "decision-evaluation.coverage")?
            .as_u64()
            .ok_or_else(|| format!("decisions: decision-evaluation.coverage.{key} must be unsigned"))?;
    }
    if counts[1] + counts[2] + counts[3] != counts[0] {
        return Err("decisions: evaluation coverage arithmetic mismatch".into());
    }
    if mode == "counterfactual" {
        unique_text_array(
            required(root, "changed_dimensions", "decision-evaluation")?,
            "decision-evaluation.changed_dimensions",
            1,
        )?;
    }
    text(
        required(root, "generated_at", "decision-evaluation")?,
        "decision-evaluation.generated_at",
    )?;
    Ok(())
}

fn schema_name(kind: &str) -> Option<&'static str> {
    match kind {
        "decision-spec" => Some("decision-spec.v1.schema.json"),
        "decision-graph" => Some("decision-graph.v1.schema.json"),
        "decision-request" => Some("decision-request.v1.schema.json"),
        "decision-provider-profile" => Some("decision-provider-profile.v1.schema.json"),
        "decision-policy" => Some("decision-policy.v1.schema.json"),
        "decision-receipt" => Some("decision-receipt.v1.schema.json"),
        "decision-outcome" => Some("decision-outcome.v1.schema.json"),
        "decision-evaluation" => Some("decision-evaluation.v1.schema.json"),
        _ => None,
    }
}

fn validate_artifact(value: &Value) -> Result<&str, String> {
    let kind = object(value, "artifact")?
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("decisions: artifact.kind is required")?;
    match kind {
        "decision-spec" => validate_spec(value)?,
        "decision-graph" => validate_graph(value)?,
        "decision-request" => validate_request(value)?,
        "decision-provider-profile" => validate_provider(value)?,
        "decision-policy" => validate_policy(value)?,
        "decision-receipt" => validate_receipt(value)?,
        "decision-outcome" => validate_outcome(value)?,
        "decision-evaluation" => validate_evaluation(value)?,
        _ => return Err(format!("decisions: unsupported artifact kind: {kind}")),
    }
    Ok(kind)
}

fn schema_digest(kind: &str) -> Result<(String, String), String> {
    let name = schema_name(kind).ok_or_else(|| format!("decisions: no schema for {kind}"))?;
    let file = SCHEMAS
        .get_file(name)
        .ok_or_else(|| format!("decisions: pinned schema is missing: {name}"))?;
    Ok((name.to_string(), check_inputs::hash(file.contents())))
}

fn canonical_fingerprint(value: &Value) -> Result<(String, usize), String> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| format!("decisions: state serialization failed: {error}"))?;
    Ok((check_inputs::hash(&bytes), bytes.len()))
}

fn jev_question(spec: &Map<String, Value>) -> Result<Value, String> {
    let kind = spec["decision_kind"]
        .as_str()
        .ok_or("decisions: spec decision_kind is missing")?;
    let instructions = spec["description"].clone();
    match kind {
        "boolean" => Ok(json!({"type":"noul","instructions":instructions})),
        "choice" => {
            let mut criteria = Map::new();
            for option in spec["options"]
                .as_array()
                .ok_or("decisions: choice spec options are missing")?
            {
                criteria.insert(
                    option.as_str().unwrap().to_string(),
                    Value::Null,
                );
            }
            Ok(json!({"type":"choice","instructions":instructions,"criteria":criteria}))
        }
        "ordinal" => Ok(json!({
            "type":"score",
            "instructions":instructions,
            "criteria":spec["levels"].clone()
        })),
        other => Err(format!(
            "decisions: Jev adapter supports boolean/choice/ordinal only, not {other}"
        )),
    }
}

fn jev_payload(request: &Value, specs: &Value, model: &str) -> Result<Value, String> {
    validate_request(request)?;
    if model.is_empty() || model.len() > 128 || model.chars().any(char::is_control) {
        return Err("decisions: invalid Jev model".into());
    }

    let mut registry = BTreeMap::<(String, u64), Map<String, Value>>::new();
    for spec in array(specs, "Jev specs", 1)? {
        validate_spec(spec)?;
        let spec = object(spec, "decision-spec")?.clone();
        let id = spec["id"].as_str().unwrap().to_string();
        let revision = spec["revision"].as_u64().unwrap();
        if registry.insert((id.clone(), revision), spec).is_some() {
            return Err(format!("decisions: duplicate spec revision: {id}@{revision}"));
        }
    }

    let request_object = object(request, "decision-request")?;
    let state = object(&request_object["state"], "decision-request.state")?;
    let payload = state
        .get("payload")
        .ok_or("decisions: Jev payload construction requires decision-request.state.payload")?
        .clone();

    let mut questions = Map::new();
    let mut sources = Vec::new();
    for question in request_object["questions"].as_array().unwrap() {
        let question = question.as_object().unwrap();
        let id = question["spec_id"].as_str().unwrap();
        let revision = question["spec_revision"].as_u64().unwrap();
        let spec = registry
            .get(&(id.to_string(), revision))
            .ok_or_else(|| format!("decisions: requested spec not found: {id}@{revision}"))?;
        if questions
            .insert(id.to_string(), jev_question(spec)?)
            .is_some()
        {
            return Err(format!(
                "decisions: Jev question IDs must be unique within one request: {id}"
            ));
        }
        sources.push(json!({"id":id,"revision":revision}));
    }

    Ok(json!({
        "format_version":1,
        "kind":"decision-provider-payload",
        "provider":"typesafe-jev",
        "endpoint":JEV_ENDPOINT,
        "request":{
            "state":payload,
            "model":model,
            "questions":questions
        },
        "source":{
            "request_id":request_object["request_id"],
            "state_fingerprint":state["fingerprint"],
            "specs":sources
        },
        "network_call_performed":false,
        "credential_source":"TYPESAFE_API_KEY environment variable",
        "authorization_header":"Bearer <redacted>",
        "not_checked":[
            "provider availability",
            "credential validity",
            "quota and billing",
            "live response",
            "provider calibration on this decision class"
        ]
    }))
}

fn validate_jev_response(value: &Value) -> Result<(), String> {
    let root = object(value, "Jev response")?;
    text(
        required(root, "model", "Jev response")?,
        "Jev response.model",
    )?;
    let answers = object(
        required(root, "answers", "Jev response")?,
        "Jev response.answers",
    )?;
    if answers.is_empty() {
        return Err("decisions: Jev response answers cannot be empty".into());
    }
    for (id, answer) in answers {
        let answer = object(answer, "Jev answer")?;
        match enum_text(
            required(answer, "type", "Jev answer")?,
            &format!("Jev answer {id}.type"),
            &["noul", "choice", "score"],
        )? {
            "noul" => {
                probability(
                    required(answer, "noul", "Jev noul answer")?,
                    &format!("Jev answer {id}.noul"),
                )?;
            }
            "choice" => {
                let choice = text(
                    required(answer, "choice", "Jev choice answer")?,
                    &format!("Jev answer {id}.choice"),
                )?;
                let probabilities = required(answer, "probabilities", "Jev choice answer")?;
                validate_distribution(
                    probabilities,
                    &format!("Jev answer {id}.probabilities"),
                )?;
                if !probabilities
                    .as_object()
                    .unwrap()
                    .contains_key(choice)
                {
                    return Err(format!(
                        "decisions: Jev answer {id} choice is absent from probabilities"
                    ));
                }
                probability(
                    required(answer, "confidence", "Jev choice answer")?,
                    &format!("Jev answer {id}.confidence"),
                )?;
            }
            "score" => {
                let score = required(answer, "score", "Jev score answer")?
                    .as_f64()
                    .ok_or_else(|| format!("decisions: Jev answer {id}.score must be numeric"))?;
                if !score.is_finite() {
                    return Err(format!("decisions: Jev answer {id}.score must be finite"));
                }
                validate_distribution(
                    required(answer, "probabilities", "Jev score answer")?,
                    &format!("Jev answer {id}.probabilities"),
                )?;
                probability(
                    required(answer, "confidence", "Jev score answer")?,
                    &format!("Jev answer {id}.confidence"),
                )?;
            }
            _ => unreachable!(),
        }
    }
    let usage = object(required(root, "usage", "Jev response")?, "Jev response.usage")?;
    for key in ["input_tokens", "output_tokens"] {
        required(usage, key, "Jev response.usage")?
            .as_u64()
            .ok_or_else(|| format!("decisions: Jev usage {key} must be unsigned"))?;
    }
    Ok(())
}

fn usage(program: &str) {
    println!(
        "Agentic Harness Decisions\n\nusage:\n  {program} validate ARTIFACT.json\n  {program} fingerprint STATE.json\n  {program} jev-payload REQUEST.json SPECS.json [--model MODEL]\n\ncommands:\n  validate      Validate a Decision Kernel v1 artifact plus semantic invariants\n  fingerprint   Canonicalize explicit JSON state and emit its SHA-256 identity\n  jev-payload   Construct the documented TypeSafe Jev request without making a network call"
    );
}

pub(crate) fn run(args: Vec<String>) {
    let program = Path::new(&args[0])
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ah decisions");

    if args.len() < 2 || matches!(args[1].as_str(), "-h" | "--help") {
        usage(program);
        return;
    }

    match args[1].as_str() {
        "validate" => {
            let path = PathBuf::from(&args[2]);
            let value = read_json(&path).unwrap_or_else(|error| crate::fail(error));
            let kind = validate_artifact(&value).unwrap_or_else(|error| crate::fail(error));
            let (schema, schema_digest) =
                schema_digest(kind).unwrap_or_else(|error| crate::fail(error));
            let artifact_bytes = serde_json::to_vec(&value).unwrap();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "format_version":1,
                    "kind":"decision-validation",
                    "valid":true,
                    "artifact_kind":kind,
                    "artifact_digest":check_inputs::hash(&artifact_bytes),
                    "schema":schema,
                    "schema_digest":schema_digest,
                    "canonical_source":crate::version()["sources"]["canonical"],
                    "semantic_checks":[
                        "decision graph referential integrity and acyclicity where applicable",
                        "evidence coverage arithmetic where applicable",
                        "probability distribution normalization where applicable",
                        "provider consequence authority is forbidden",
                        "evaluation side effects are forbidden",
                        "semantic confidence is not domain outcome probability"
                    ],
                    "not_checked":[
                        "factual correctness",
                        "provider quality or calibration",
                        "authorization outside the Decision Kernel",
                        "external evidence availability"
                    ]
                }))
                .unwrap()
            );
        }
        "fingerprint" => {
            let path = PathBuf::from(&args[2]);
            let value = read_json(&path).unwrap_or_else(|error| crate::fail(error));
            let (fingerprint, bytes) =
                canonical_fingerprint(&value).unwrap_or_else(|error| crate::fail(error));
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "format_version":1,
                    "kind":"decision-state-fingerprint",
                    "algorithm":"ah-json-sha256-v1",
                    "fingerprint":fingerprint,
                    "canonical_json_bytes":bytes
                }))
                .unwrap()
            );
        }
        "jev-payload" => {
            let request_path = PathBuf::from(&args[2]);
            let specs_path = PathBuf::from(&args[3]);
            let mut model = "jev-latest".to_string();
            let mut index = 4;
            while index < args.len() {
                match args[index].as_str() {
                    "--model" => {
                        index += 1;
                        model = args[index].clone();
                    }
                    option => crate::fail(format!("decisions: unknown jev-payload option: {option}")),
                }
                index += 1;
            }
            let request = read_json(&request_path).unwrap_or_else(|error| crate::fail(error));
            let specs = read_json(&specs_path).unwrap_or_else(|error| crate::fail(error));
            let payload =
                jev_payload(&request, &specs, &model).unwrap_or_else(|error| crate::fail(error));
            println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        }
        _ => {
            usage(program);
            crate::finish(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: &str) -> Value {
        let mut value = json!({
            "format_version":1,
            "kind":"decision-spec",
            "id":"task.route",
            "revision":1,
            "decision_kind":kind,
            "description":"Route the task",
            "input":{"schema_id":"task-state","schema_version":1,"immutable_snapshot_required":true},
            "evidence":{"requirements":[]},
            "policy":{"risk":"low","consequential_action":"forbidden"}
        });
        if kind == "choice" {
            value["options"] = json!(["code", "docs"]);
        }
        if kind == "ordinal" {
            value["levels"] = json!(["low", "medium", "high"]);
        }
        value
    }

    #[test]
    fn graph_rejects_cycles_and_unknown_reducer_inputs() {
        let mut graph = json!({
            "format_version":1,"kind":"decision-graph","id":"test.graph","revision":1,
            "nodes":[
                {"id":"a","spec_id":"a","spec_revision":1,"depends_on":[]},
                {"id":"b","spec_id":"b","spec_revision":1,"depends_on":["a"]}
            ],
            "reducers":[{"id":"r","type":"deterministic","inputs":["a","b"],"output":"summary"}]
        });
        assert!(validate_graph(&graph).is_ok());
        graph["nodes"][0]["depends_on"] = json!(["b"]);
        assert!(validate_graph(&graph).unwrap_err().contains("cycle"));
        graph["nodes"][0]["depends_on"] = json!([]);
        graph["reducers"][0]["inputs"] = json!(["missing"]);
        assert!(validate_graph(&graph).unwrap_err().contains("not a decision node"));
    }

    #[test]
    fn provider_and_evaluation_cannot_grant_side_effect_authority() {
        let provider = json!({
            "format_version":1,"kind":"decision-provider-profile","id":"jev","revision":1,
            "provider_type":"jev","decision_kinds":["boolean"],
            "confidence":{"semantics":"provider-defined","supports_distribution":true},
            "external_network":true,"requires_credentials":true,"consequence_authority":true
        });
        assert!(validate_provider(&provider).unwrap_err().contains("consequence authority"));

        let evaluation = json!({
            "format_version":1,"kind":"decision-evaluation","id":"eval","mode":"shadow",
            "dataset":{"id":"d","revision":1},"candidate":{"provider_id":"jev"},
            "side_effects":true,"metrics":[],"coverage":{"total":0,"evaluated":0,"abstained":0,"failed":0},
            "generated_at":"2026-09-18T00:00:00Z"
        });
        assert!(validate_evaluation(&evaluation).unwrap_err().contains("side effects"));
    }

    #[test]
    fn receipt_separates_confidence_from_outcome_probability_and_checks_coverage() {
        let mut receipt = json!({
            "format_version":1,"kind":"decision-receipt","id":"r",
            "spec":{"id":"task.route","revision":1},
            "state":{"schema_id":"task-state","schema_version":1,"fingerprint":"sha256:12345678"},
            "status":"produced","result":{"value":"code","distribution":{"code":0.8,"docs":0.2}},
            "provider":{"type":"jev","id":"jev"},
            "uncertainty":{
                "provider_confidence":0.8,"calibration":{"status":"uncalibrated"},
                "evidence_coverage":{"value":0.5,"required_present":1,"required_total":2,"missing":["b"]},
                "evidence_reliability":null,"decision_certainty":null
            },
            "evidence":{"used":["a"],"missing":["b"]},
            "policy":{"id":"p","revision":1,"disposition":"review","reasons":[]},
            "timing":{"decided_at":"2026-09-18T00:00:00Z"}
        });
        assert!(validate_receipt(&receipt).is_ok());
        receipt["outcome_probability"] = json!(0.8);
        assert!(validate_receipt(&receipt).unwrap_err().contains("outcome_probability"));
        receipt.as_object_mut().unwrap().remove("outcome_probability");
        receipt["uncertainty"]["evidence_coverage"]["value"] = json!(1.0);
        assert!(validate_receipt(&receipt).unwrap_err().contains("coverage arithmetic"));
    }

    #[test]
    fn fingerprint_is_stable_across_object_key_order() {
        let a: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
        assert_eq!(canonical_fingerprint(&a).unwrap(), canonical_fingerprint(&b).unwrap());
    }

    #[test]
    fn jev_payload_maps_boolean_choice_and_ordinal_without_network_or_secrets() {
        let request = json!({
            "format_version":1,"kind":"decision-request","request_id":"q",
            "state":{"schema_id":"task-state","schema_version":1,"fingerprint":"sha256:12345678","payload":{"task":"fix docs"}},
            "questions":[
                {"spec_id":"is-risky","spec_revision":1},
                {"spec_id":"task-type","spec_revision":1},
                {"spec_id":"complexity","spec_revision":1}
            ],
            "mode":"live"
        });
        let mut boolean = spec("boolean");
        boolean["id"] = json!("is-risky");
        let mut choice = spec("choice");
        choice["id"] = json!("task-type");
        let mut ordinal = spec("ordinal");
        ordinal["id"] = json!("complexity");
        let payload = jev_payload(&request, &json!([boolean, choice, ordinal]), "jev-latest").unwrap();
        assert_eq!(payload["request"]["questions"]["is-risky"]["type"], "noul");
        assert_eq!(payload["request"]["questions"]["task-type"]["type"], "choice");
        assert_eq!(payload["request"]["questions"]["complexity"]["type"], "score");
        assert_eq!(payload["network_call_performed"], false);
        assert_eq!(payload["authorization_header"], "Bearer <redacted>");
    }

    #[test]
    fn jev_response_parser_checks_probabilities_and_usage() {
        let response = json!({
            "model":"jev-latest",
            "answers":{
                "urgent":{"type":"noul","noul":0.9},
                "route":{"type":"choice","choice":"code","probabilities":{"code":0.8,"docs":0.2},"confidence":0.7},
                "complexity":{"type":"score","score":1.4,"probabilities":{"0":0.1,"1":0.4,"2":0.5},"confidence":0.6}
            },
            "usage":{"input_tokens":20,"output_tokens":3}
        });
        assert!(validate_jev_response(&response).is_ok());
        let mut bad = response.clone();
        bad["answers"]["route"]["probabilities"]["code"] = json!(0.9);
        assert!(validate_jev_response(&bad).unwrap_err().contains("sum to 1"));
    }
}
