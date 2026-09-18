//! Provider-neutral Decision Kernel runtime helpers.
//!
//! This slice is deliberately offline: it validates the pinned canonical
//! contracts, fingerprints explicit state, and constructs a TypeSafe Jev
//! HTTP payload without performing a network call or reading credentials.
use crate::{
    check_inputs,
    jev_transport::{self, JEV_ENDPOINT, ProviderError, TransportOptions},
};
use include_dir::{Dir, include_dir};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

static SCHEMAS: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/schema");

const LIMIT: usize = 2_000_000;
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

fn provider_fail(error: ProviderError) -> ! {
    eprintln!(
        "{}",
        json!({
            "format_version": 1,
            "kind": "diagnostic",
            "code": error.code,
            "message": error.message,
            "provider": "typesafe-jev",
            "http_status": error.status,
            "attempts": error.attempts,
            "credential_source": "TYPESAFE_API_KEY",
            "authorization_header": "Bearer <redacted>"
        })
    );
    crate::finish(3)
}

fn provider_fail_code(code: &'static str, message: &str) -> ! {
    provider_fail(ProviderError {
        code,
        message: message.to_string(),
        status: None,
        attempts: 0,
    })
}

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
        return Err(format!(
            "decisions: {context} must contain at least {min} item(s)"
        ));
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
        return Err(format!(
            "decisions: {context} is empty, oversized or contains control characters"
        ));
    }
    Ok(value)
}

fn bounded_text<'a>(value: &'a Value, context: &str, max: usize) -> Result<&'a str, String> {
    let value = text(value, context)?;
    if value.len() > max {
        return Err(format!("decisions: {context} exceeds {max} bytes"));
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

fn enum_text<'a>(value: &'a Value, context: &str, allowed: &[&str]) -> Result<&'a str, String> {
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
        return Err(format!(
            "decisions: unsupported {expected_kind} format_version"
        ));
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
    positive_revision(
        required(root, "revision", "decision-spec")?,
        "decision-spec.revision",
    )?;
    let decision_kind = enum_text(
        required(root, "decision_kind", "decision-spec")?,
        "decision-spec.decision_kind",
        DECISION_KINDS,
    )?;
    text(
        required(root, "description", "decision-spec")?,
        "decision-spec.description",
    )?;

    let input = object(
        required(root, "input", "decision-spec")?,
        "decision-spec.input",
    )?;
    text(
        required(input, "schema_id", "decision-spec.input")?,
        "decision-spec.input.schema_id",
    )?;
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
            return Err(format!(
                "decisions: duplicate evidence requirement id: {id}"
            ));
        }
        required(
            requirement,
            "required",
            "decision-spec.evidence.requirement",
        )?
        .as_bool()
        .ok_or("decisions: evidence requirement required must be boolean")?;
        text(
            required(
                requirement,
                "description",
                "decision-spec.evidence.requirement",
            )?,
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
                value.as_bool().ok_or_else(|| {
                    format!("decisions: decision-spec.uncertainty.{key} must be boolean")
                })?;
            }
        }
    }

    let policy = object(
        required(root, "policy", "decision-spec")?,
        "decision-spec.policy",
    )?;
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
    positive_revision(
        required(root, "revision", "decision-graph")?,
        "decision-graph.revision",
    )?;
    let nodes = array(
        required(root, "nodes", "decision-graph")?,
        "decision-graph.nodes",
        1,
    )?;
    let mut ids = BTreeSet::new();
    let mut dependencies = BTreeMap::<String, Vec<String>>::new();

    for node in nodes {
        let node = object(node, "decision-graph.node")?;
        let id = text(
            required(node, "id", "decision-graph.node")?,
            "decision-graph.node.id",
        )?;
        if !ids.insert(id.to_string()) {
            return Err(format!("decisions: duplicate decision graph node id: {id}"));
        }
        text(
            required(node, "spec_id", "decision-graph.node")?,
            "decision-graph.node.spec_id",
        )?;
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
                return Err(format!(
                    "decisions: decision graph dependency is unknown: {dep}"
                ));
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
            return Err(format!(
                "decisions: decision graph cycle detected at {node}"
            ));
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
                    return Err(format!(
                        "decisions: reducer input is not a decision node: {input}"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_request(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-request")?;
    text(
        required(root, "request_id", "decision-request")?,
        "decision-request.request_id",
    )?;
    enum_text(
        required(root, "mode", "decision-request")?,
        "decision-request.mode",
        &["live", "shadow", "replay", "evaluation"],
    )?;
    let state = object(
        required(root, "state", "decision-request")?,
        "decision-request.state",
    )?;
    text(
        required(state, "schema_id", "decision-request.state")?,
        "decision-request.state.schema_id",
    )?;
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
    text(
        required(root, "id", "decision-provider-profile")?,
        "decision-provider-profile.id",
    )?;
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
            return Err(format!(
                "decisions: unsupported provider decision kind: {kind}"
            ));
        }
    }
    let confidence = object(
        required(root, "confidence", "decision-provider-profile")?,
        "decision-provider-profile.confidence",
    )?;
    enum_text(
        required(
            confidence,
            "semantics",
            "decision-provider-profile.confidence",
        )?,
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
    text(
        required(root, "id", "decision-policy")?,
        "decision-policy.id",
    )?;
    positive_revision(
        required(root, "revision", "decision-policy")?,
        "decision-policy.revision",
    )?;
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
    let evidence = object(
        required(root, "evidence", "decision-policy")?,
        "decision-policy.evidence",
    )?;
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
    required(
        confidence,
        "calibration_required",
        "decision-policy.confidence",
    )?
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
            return Err(
                "decisions: policy invariants must declare deterministic enforcement".into(),
            );
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

pub(crate) fn validate_receipt(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-receipt")?;
    if root.contains_key("outcome_probability") {
        return Err("decisions: outcome_probability is not a DecisionReceipt field".into());
    }
    text(
        required(root, "id", "decision-receipt")?,
        "decision-receipt.id",
    )?;
    let spec = object(
        required(root, "spec", "decision-receipt")?,
        "decision-receipt.spec",
    )?;
    text(
        required(spec, "id", "decision-receipt.spec")?,
        "decision-receipt.spec.id",
    )?;
    positive_revision(
        required(spec, "revision", "decision-receipt.spec")?,
        "decision-receipt.spec.revision",
    )?;
    let state = object(
        required(root, "state", "decision-receipt")?,
        "decision-receipt.state",
    )?;
    text(
        required(state, "schema_id", "decision-receipt.state")?,
        "decision-receipt.state.schema_id",
    )?;
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
    text(
        required(provider, "id", "decision-receipt.provider")?,
        "decision-receipt.provider.id",
    )?;

    let uncertainty = object(
        required(root, "uncertainty", "decision-receipt")?,
        "decision-receipt.uncertainty",
    )?;
    if uncertainty.contains_key("outcome_probability") {
        return Err("decisions: outcome_probability cannot be stored as uncertainty".into());
    }
    optional_probability(
        required(
            uncertainty,
            "provider_confidence",
            "decision-receipt.uncertainty",
        )?,
        "decision-receipt.uncertainty.provider_confidence",
    )?;
    optional_probability(
        required(
            uncertainty,
            "evidence_reliability",
            "decision-receipt.uncertainty",
        )?,
        "decision-receipt.uncertainty.evidence_reliability",
    )?;
    optional_probability(
        required(
            uncertainty,
            "decision_certainty",
            "decision-receipt.uncertainty",
        )?,
        "decision-receipt.uncertainty.decision_certainty",
    )?;
    let calibration = object(
        required(uncertainty, "calibration", "decision-receipt.uncertainty")?,
        "decision-receipt.uncertainty.calibration",
    )?;
    enum_text(
        required(
            calibration,
            "status",
            "decision-receipt.uncertainty.calibration",
        )?,
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

    let policy = object(
        required(root, "policy", "decision-receipt")?,
        "decision-receipt.policy",
    )?;
    text(
        required(policy, "id", "decision-receipt.policy")?,
        "decision-receipt.policy.id",
    )?;
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
    text(
        required(root, "id", "decision-outcome")?,
        "decision-outcome.id",
    )?;
    text(
        required(root, "receipt_id", "decision-outcome")?,
        "decision-outcome.receipt_id",
    )?;
    text(
        required(root, "observed_at", "decision-outcome")?,
        "decision-outcome.observed_at",
    )?;
    let outcome = object(
        required(root, "outcome", "decision-outcome")?,
        "decision-outcome.outcome",
    )?;
    text(
        required(outcome, "label", "decision-outcome.outcome")?,
        "decision-outcome.outcome.label",
    )?;
    let verification = object(
        required(root, "verification", "decision-outcome")?,
        "decision-outcome.verification",
    )?;
    enum_text(
        required(verification, "type", "decision-outcome.verification")?,
        "decision-outcome.verification.type",
        &[
            "human",
            "deterministic",
            "external-authority",
            "measurement",
            "unknown",
        ],
    )?;
    let feedback = object(
        required(root, "feedback", "decision-outcome")?,
        "decision-outcome.feedback",
    )?;
    required(
        feedback,
        "usable_for_evaluation",
        "decision-outcome.feedback",
    )?
    .as_bool()
    .ok_or("decisions: decision-outcome.feedback.usable_for_evaluation must be boolean")?;
    Ok(())
}

fn validate_evaluation(value: &Value) -> Result<(), String> {
    let root = base(value, "decision-evaluation")?;
    text(
        required(root, "id", "decision-evaluation")?,
        "decision-evaluation.id",
    )?;
    let mode = enum_text(
        required(root, "mode", "decision-evaluation")?,
        "decision-evaluation.mode",
        &[
            "offline",
            "replay",
            "shadow",
            "champion-challenger",
            "counterfactual",
        ],
    )?;
    if required(root, "side_effects", "decision-evaluation")?.as_bool() != Some(false) {
        return Err("decisions: evaluation side effects are forbidden".into());
    }
    let dataset = object(
        required(root, "dataset", "decision-evaluation")?,
        "decision-evaluation.dataset",
    )?;
    text(
        required(dataset, "id", "decision-evaluation.dataset")?,
        "decision-evaluation.dataset.id",
    )?;
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
            .ok_or_else(|| {
                format!("decisions: decision-evaluation.coverage.{key} must be unsigned")
            })?;
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
        "decision-eval-dataset" => Some("decision-eval-dataset.v1.schema.json"),
        "decision-calibration" => Some("decision-calibration.v1.schema.json"),
        "decision-regression" => Some("decision-regression.v1.schema.json"),
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
        "decision-eval-dataset" => crate::decision_calibration::validate_dataset(value)?,
        "decision-calibration" => crate::decision_calibration::validate_calibration(value)?,
        "decision-regression" => crate::decision_calibration::validate_regression(value)?,
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
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("decisions: state serialization failed: {error}"))?;
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
                criteria.insert(option.as_str().unwrap().to_string(), Value::Null);
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
            return Err(format!(
                "decisions: duplicate spec revision: {id}@{revision}"
            ));
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
                validate_distribution(probabilities, &format!("Jev answer {id}.probabilities"))?;
                if !probabilities.as_object().unwrap().contains_key(choice) {
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
    let usage = object(
        required(root, "usage", "Jev response")?,
        "Jev response.usage",
    )?;
    for key in ["input_tokens", "output_tokens"] {
        required(usage, key, "Jev response.usage")?
            .as_u64()
            .ok_or_else(|| format!("decisions: Jev usage {key} must be unsigned"))?;
    }
    Ok(())
}

fn evidence_for_spec(
    manifest: Option<&Value>,
    spec_id: &str,
    required_ids: &[String],
) -> Result<(Vec<String>, Vec<String>), String> {
    let Some(manifest) = manifest else {
        return Ok((Vec::new(), required_ids.to_vec()));
    };
    let root = object(manifest, "decision evidence manifest")?;
    let Some(entry) = root.get(spec_id) else {
        return Ok((Vec::new(), required_ids.to_vec()));
    };
    let entry = object(entry, "decision evidence entry")?;
    let used = entry
        .get("used")
        .map(|value| unique_text_array(value, "decision evidence used", 0))
        .transpose()?
        .unwrap_or_default();
    let satisfied = entry
        .get("satisfied_requirements")
        .map(|value| unique_text_array(value, "decision evidence satisfied_requirements", 0))
        .transpose()?
        .unwrap_or_default();
    let required: BTreeSet<_> = required_ids.iter().cloned().collect();
    for item in &satisfied {
        if !required.contains(item) {
            return Err(format!(
                "decisions: evidence manifest marks unknown requirement as satisfied: {item}"
            ));
        }
    }
    let satisfied: BTreeSet<_> = satisfied.into_iter().collect();
    let missing = required_ids
        .iter()
        .filter(|id| !satisfied.contains(*id))
        .cloned()
        .collect();
    Ok((used, missing))
}

fn normalize_jev_receipts(
    request: &Value,
    specs: &Value,
    response: &Value,
    evidence_manifest: Option<&Value>,
    decided_at: &str,
) -> Result<Value, String> {
    validate_request(request)?;
    validate_jev_response(response)?;
    if decided_at.is_empty() || !decided_at.contains('T') {
        return Err("decisions: --decided-at must be an RFC3339 timestamp".into());
    }

    let mut registry = BTreeMap::<(String, u64), Map<String, Value>>::new();
    for spec in array(specs, "Jev specs", 1)? {
        validate_spec(spec)?;
        let spec = object(spec, "decision-spec")?.clone();
        let id = spec["id"].as_str().unwrap().to_string();
        let revision = spec["revision"].as_u64().unwrap();
        if registry.insert((id.clone(), revision), spec).is_some() {
            return Err(format!(
                "decisions: duplicate spec revision: {id}@{revision}"
            ));
        }
    }

    let request_object = object(request, "decision-request")?;
    let state = object(&request_object["state"], "decision-request.state")?;
    let response_object = object(response, "Jev response")?;
    let answers = object(&response_object["answers"], "Jev response.answers")?;
    let provider_model = response_object["model"].as_str().unwrap();
    let usage = response_object["usage"].clone();
    let mut receipts = Vec::new();

    for question in request_object["questions"].as_array().unwrap() {
        let question = question.as_object().unwrap();
        let spec_id = question["spec_id"].as_str().unwrap();
        let spec_revision = question["spec_revision"].as_u64().unwrap();
        let spec = registry
            .get(&(spec_id.to_string(), spec_revision))
            .ok_or_else(|| {
                format!("decisions: requested spec not found: {spec_id}@{spec_revision}")
            })?;
        let answer = answers
            .get(spec_id)
            .ok_or_else(|| format!("decisions: Jev response is missing answer for {spec_id}"))?;
        let answer = object(answer, "Jev answer")?;
        let answer_type = answer["type"].as_str().unwrap();
        let decision_kind = spec["decision_kind"].as_str().unwrap();

        let (result, provider_confidence) = match (decision_kind, answer_type) {
            ("boolean", "noul") => {
                let p = probability(&answer["noul"], "Jev noul answer")?;
                (
                    json!({
                        "value": p >= 0.5,
                        "distribution": {"false": 1.0 - p, "true": p}
                    }),
                    p.max(1.0 - p),
                )
            }
            ("choice", "choice") => {
                let choice = answer["choice"].as_str().unwrap();
                let allowed: BTreeSet<_> = spec["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .collect();
                if !allowed.contains(choice) {
                    return Err(format!(
                        "decisions: Jev answer selected undeclared option {choice} for {spec_id}"
                    ));
                }
                let probabilities = answer["probabilities"].clone();
                let keys: BTreeSet<_> = probabilities
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect();
                if keys != allowed {
                    return Err(format!(
                        "decisions: Jev probability labels do not match Choice options for {spec_id}"
                    ));
                }
                (
                    json!({"value": choice, "distribution": probabilities}),
                    probability(&answer["confidence"], "Jev choice confidence")?,
                )
            }
            ("ordinal", "score") => {
                let levels = spec["levels"].as_array().unwrap();
                let legend = object(
                    answer.get("legend").ok_or_else(|| {
                        format!("decisions: Jev Score legend missing for {spec_id}")
                    })?,
                    "Jev score legend",
                )?;
                if legend.len() != levels.len() {
                    return Err(format!(
                        "decisions: Jev Score legend length does not match levels for {spec_id}"
                    ));
                }
                for (index, level) in levels.iter().enumerate() {
                    if legend.get(&index.to_string()).and_then(Value::as_str) != level.as_str() {
                        return Err(format!(
                            "decisions: Jev Score legend does not match declared levels for {spec_id}"
                        ));
                    }
                }
                (
                    json!({
                        "value": answer["score"],
                        "alternatives": answer["legend"],
                        "distribution": answer["probabilities"]
                    }),
                    probability(&answer["confidence"], "Jev score confidence")?,
                )
            }
            _ => {
                return Err(format!(
                    "decisions: Jev answer type {answer_type} does not match DecisionSpec kind {decision_kind} for {spec_id}"
                ));
            }
        };

        let required_ids: Vec<String> = spec["evidence"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|requirement| requirement["required"].as_bool() == Some(true))
            .filter_map(|requirement| requirement["id"].as_str().map(str::to_string))
            .collect();
        let (used, missing) = evidence_for_spec(evidence_manifest, spec_id, &required_ids)?;
        let total = required_ids.len() as u64;
        let present = total - missing.len() as u64;
        let coverage = if total == 0 {
            1.0
        } else {
            present as f64 / total as f64
        };

        let receipt_identity = json!({
            "request_id": request_object["request_id"],
            "spec_id": spec_id,
            "spec_revision": spec_revision,
            "state_fingerprint": state["fingerprint"],
            "provider_model": provider_model,
            "answer": answer,
            "decided_at": decided_at,
        });
        let receipt_id =
            canonical_fingerprint(&receipt_identity)?
                .0
                .replacen("sha256:", "decision-", 1);
        let mut receipt = json!({
            "format_version": 1,
            "kind": "decision-receipt",
            "id": receipt_id,
            "spec": {"id": spec_id, "revision": spec_revision},
            "state": {
                "schema_id": state["schema_id"],
                "schema_version": state["schema_version"],
                "fingerprint": state["fingerprint"]
            },
            "status": "produced",
            "result": result,
            "provider": {
                "type": "jev",
                "id": "typesafe-jev",
                "model": provider_model
            },
            "uncertainty": {
                "provider_confidence": provider_confidence,
                "calibration": {"status": "unknown"},
                "evidence_coverage": {
                    "value": coverage,
                    "required_present": present,
                    "required_total": total,
                    "missing": missing
                },
                "evidence_reliability": Value::Null,
                "decision_certainty": Value::Null
            },
            "evidence": {"used": used, "missing": missing},
            "policy": {
                "id": "unapplied",
                "revision": 1,
                "disposition": "review",
                "reasons": [
                    "provider output has not passed project policy and deterministic invariants"
                ]
            },
            "timing": {"decided_at": decided_at},
            "usage": usage
        });
        if let Some(latency) = response_object.get("latency_ms").and_then(Value::as_u64) {
            receipt["timing"]["latency_ms"] = json!(latency);
        }
        validate_receipt(&receipt)?;
        receipts.push(receipt);
    }

    if answers.len() != receipts.len() {
        return Err(
            "decisions: Jev response contains answers not present in the DecisionRequest".into(),
        );
    }

    Ok(json!({
        "format_version": 1,
        "kind": "decision-receipt-set",
        "request_id": request_object["request_id"],
        "provider": {"type": "jev", "id": "typesafe-jev", "model": provider_model},
        "receipts": receipts,
        "policy_applied": false,
        "consequence_authorized": false
    }))
}

fn graph_plan(
    graph: &Value,
    specs: &Value,
    state: &Value,
    provider: Option<&str>,
    mode: &str,
) -> Result<Value, String> {
    validate_graph(graph)?;
    if !state.is_object() {
        return Err("decisions: plan state must be a JSON object".into());
    }
    if !["live", "shadow", "replay", "evaluation"].contains(&mode) {
        return Err("decisions: unsupported plan mode".into());
    }

    let mut registry = BTreeMap::<(String, u64), Map<String, Value>>::new();
    for spec in array(specs, "decision specs", 1)? {
        validate_spec(spec)?;
        let spec = object(spec, "decision-spec")?.clone();
        let key = (
            spec["id"].as_str().unwrap().to_string(),
            spec["revision"].as_u64().unwrap(),
        );
        if registry.insert(key.clone(), spec).is_some() {
            return Err(format!(
                "decisions: duplicate spec revision: {}@{}",
                key.0, key.1
            ));
        }
    }

    let graph_object = object(graph, "decision-graph")?;
    let nodes = graph_object["nodes"].as_array().unwrap();
    let first = nodes.first().unwrap().as_object().unwrap();
    let first_spec = registry
        .get(&(
            first["spec_id"].as_str().unwrap().to_string(),
            first["spec_revision"].as_u64().unwrap(),
        ))
        .ok_or("decisions: graph references a DecisionSpec not present in the registry")?;
    let schema_id = first_spec["input"]["schema_id"].clone();
    let schema_version = first_spec["input"]["schema_version"].clone();

    let mut pending = BTreeMap::<String, (String, u64, Vec<String>)>::new();
    for node in nodes {
        let node = node.as_object().unwrap();
        let node_id = node["id"].as_str().unwrap().to_string();
        let spec_id = node["spec_id"].as_str().unwrap().to_string();
        let revision = node["spec_revision"].as_u64().unwrap();
        let spec = registry.get(&(spec_id.clone(), revision)).ok_or_else(|| {
            format!("decisions: graph references missing spec {spec_id}@{revision}")
        })?;
        if spec["input"]["schema_id"] != schema_id
            || spec["input"]["schema_version"] != schema_version
        {
            return Err(
                "decisions: all specs in one DecisionGraph must share a state schema/version"
                    .into(),
            );
        }
        let deps = node["depends_on"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        pending.insert(node_id, (spec_id, revision, deps));
    }

    let state_fingerprint = canonical_fingerprint(state)?.0;
    let mut completed = BTreeSet::new();
    let mut stages = Vec::new();
    while !pending.is_empty() {
        let ready: Vec<String> = pending
            .iter()
            .filter(|(_, (_, _, deps))| deps.iter().all(|dep| completed.contains(dep)))
            .map(|(id, _)| id.clone())
            .collect();
        if ready.is_empty() {
            return Err(
                "decisions: graph could not be scheduled; dependency cycle or missing node".into(),
            );
        }
        let mut stage = Vec::new();
        for node_id in ready {
            let (spec_id, revision, deps) = pending.remove(&node_id).unwrap();
            let cache_identity = json!({
                "state_fingerprint": state_fingerprint,
                "spec_id": spec_id,
                "spec_revision": revision,
                "provider": provider,
                "mode": mode
            });
            stage.push(json!({
                "node_id": node_id,
                "spec_id": spec_id,
                "spec_revision": revision,
                "depends_on": deps,
                "cache_key": canonical_fingerprint(&cache_identity)?.0
            }));
            completed.insert(node_id);
        }
        stages.push(json!({
            "parallel": true,
            "nodes": stage
        }));
    }

    Ok(json!({
        "format_version": 1,
        "kind": "decision-plan",
        "graph": {"id": graph_object["id"], "revision": graph_object["revision"]},
        "state": {
            "schema_id": schema_id,
            "schema_version": schema_version,
            "fingerprint": state_fingerprint
        },
        "provider_hint": provider,
        "mode": mode,
        "stages": stages,
        "provider_calls_performed": false,
        "side_effects": false,
        "consequence_authorized": false
    }))
}

fn replay_receipts(graph: &Value, receipt_set: &Value) -> Result<Value, String> {
    validate_graph(graph)?;
    let receipts =
        if receipt_set.get("kind").and_then(Value::as_str) == Some("decision-receipt-set") {
            receipt_set
                .get("receipts")
                .and_then(Value::as_array)
                .ok_or("decisions: decision-receipt-set.receipts must be an array")?
        } else {
            receipt_set
                .as_array()
                .ok_or("decisions: replay receipts must be an array or decision-receipt-set")?
        };
    let mut by_spec = BTreeMap::<(String, u64), &Value>::new();
    let mut state_fingerprint: Option<String> = None;
    for receipt in receipts {
        validate_receipt(receipt)?;
        let receipt_state = receipt["state"]["fingerprint"]
            .as_str()
            .unwrap()
            .to_string();
        if state_fingerprint
            .as_ref()
            .is_some_and(|known| known != &receipt_state)
        {
            return Err(
                "decisions: replay receipts do not share one immutable state fingerprint".into(),
            );
        }
        state_fingerprint.get_or_insert(receipt_state);
        let key = (
            receipt["spec"]["id"].as_str().unwrap().to_string(),
            receipt["spec"]["revision"].as_u64().unwrap(),
        );
        if by_spec.insert(key.clone(), receipt).is_some() {
            return Err(format!(
                "decisions: replay has multiple receipts for {}@{}",
                key.0, key.1
            ));
        }
    }

    let graph_object = object(graph, "decision-graph")?;
    let mut nodes = Vec::new();
    let mut node_receipts = BTreeMap::<String, String>::new();
    let mut unresolved = Vec::new();
    for node in graph_object["nodes"].as_array().unwrap() {
        let node = node.as_object().unwrap();
        let node_id = node["id"].as_str().unwrap();
        let key = (
            node["spec_id"].as_str().unwrap().to_string(),
            node["spec_revision"].as_u64().unwrap(),
        );
        if let Some(receipt) = by_spec.get(&key) {
            let receipt_id = receipt["id"].as_str().unwrap().to_string();
            node_receipts.insert(node_id.to_string(), receipt_id.clone());
            nodes.push(json!({
                "node_id": node_id,
                "receipt_id": receipt_id,
                "status": receipt["status"],
                "result": receipt.get("result").cloned().unwrap_or(Value::Null)
            }));
        } else {
            unresolved.push(node_id.to_string());
            nodes.push(json!({
                "node_id": node_id,
                "receipt_id": Value::Null,
                "status": "missing",
                "result": Value::Null
            }));
        }
    }

    let reducers = graph_object
        .get("reducers")
        .and_then(Value::as_array)
        .map(|reducers| {
            reducers
                .iter()
                .map(|reducer| {
                    let reducer = reducer.as_object().unwrap();
                    let inputs = reducer["inputs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|node_id| {
                            json!({
                                "node_id": node_id,
                                "receipt_id": node_receipts.get(node_id)
                            })
                        })
                        .collect::<Vec<_>>();
                    json!({
                        "id": reducer["id"],
                        "type": "deterministic",
                        "output": reducer.get("output").cloned().unwrap_or(Value::Null),
                        "inputs": inputs,
                        "executed": false
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(json!({
        "format_version": 1,
        "kind": "decision-replay",
        "graph": {"id": graph_object["id"], "revision": graph_object["revision"]},
        "state_fingerprint": state_fingerprint,
        "complete": unresolved.is_empty(),
        "unresolved_nodes": unresolved,
        "nodes": nodes,
        "reducers": reducers,
        "provider_calls_performed": false,
        "side_effects": false,
        "consequence_authorized": false,
        "not_checked": [
            "domain reducer implementation",
            "external evidence freshness",
            "historical provider availability"
        ]
    }))
}

struct OutcomeOptions<'a> {
    id: &'a str,
    observed_at: &'a str,
    label: &'a str,
    verification_type: &'a str,
    verification_ref: Option<&'a str>,
    action_ref: Option<&'a str>,
    usable_for_evaluation: bool,
    success: Option<bool>,
}

fn outcome_from_receipt(receipt: &Value, options: OutcomeOptions<'_>) -> Result<Value, String> {
    let OutcomeOptions {
        id,
        observed_at,
        label,
        verification_type,
        verification_ref,
        action_ref,
        usable_for_evaluation,
        success,
    } = options;
    validate_receipt(receipt)?;
    bounded_text(&Value::String(id.to_string()), "outcome id", 512)?;
    bounded_text(
        &Value::String(observed_at.to_string()),
        "outcome observed_at",
        128,
    )?;
    if !observed_at.contains('T') {
        return Err("decisions: --observed-at must be an RFC3339 timestamp".into());
    }
    bounded_text(&Value::String(label.to_string()), "outcome label", 512)?;
    if ![
        "human",
        "deterministic",
        "external-authority",
        "measurement",
        "unknown",
    ]
    .contains(&verification_type)
    {
        return Err("decisions: unsupported outcome verification type".into());
    }

    let mut verification = json!({"type": verification_type});
    if let Some(reference) = verification_ref {
        bounded_text(
            &Value::String(reference.to_string()),
            "verification ref",
            2048,
        )?;
        verification["ref"] = json!(reference);
    }

    let mut outcome_value = json!({"label": label});
    if let Some(success) = success {
        outcome_value["success"] = json!(success);
    }

    let mut outcome = json!({
        "format_version": 1,
        "kind": "decision-outcome",
        "id": id,
        "receipt_id": receipt["id"],
        "observed_at": observed_at,
        "outcome": outcome_value,
        "verification": verification,
        "feedback": {"usable_for_evaluation": usable_for_evaluation}
    });
    if let Some(action_ref) = action_ref {
        bounded_text(
            &Value::String(action_ref.to_string()),
            "outcome action_ref",
            2048,
        )?;
        outcome["action_ref"] = json!(action_ref);
    }
    validate_outcome(&outcome)?;
    Ok(outcome)
}

fn receipt_provider_id(receipt: &Value) -> &str {
    receipt["provider"]["id"].as_str().unwrap_or("unknown")
}

fn receipt_disposition(receipt: &Value) -> &str {
    receipt["policy"]["disposition"]
        .as_str()
        .unwrap_or("unknown")
}

fn receipt_confidence(receipt: &Value) -> Option<f64> {
    receipt["uncertainty"]["provider_confidence"].as_f64()
}

fn compare_receipts(
    champion: &Value,
    candidate: &Value,
    mode: &str,
    dataset_id: &str,
    dataset_revision: &str,
    generated_at: &str,
    changed_dimensions: &[String],
) -> Result<Value, String> {
    validate_receipt(champion)?;
    validate_receipt(candidate)?;
    if !["shadow", "champion-challenger", "counterfactual"].contains(&mode) {
        return Err("decisions: unsupported comparison mode".into());
    }
    if dataset_id.is_empty() || dataset_revision.is_empty() {
        return Err("decisions: dataset id/revision must be non-empty".into());
    }
    if !generated_at.contains('T') {
        return Err("decisions: --generated-at must be an RFC3339 timestamp".into());
    }

    let changed: BTreeSet<_> = changed_dimensions.iter().map(String::as_str).collect();
    for dimension in &changed {
        if ![
            "provider",
            "model",
            "spec",
            "policy",
            "evidence",
            "threshold",
            "state",
        ]
        .contains(dimension)
        {
            return Err(format!(
                "decisions: unsupported changed dimension {dimension}"
            ));
        }
    }
    if mode == "counterfactual" && changed.is_empty() {
        return Err("decisions: counterfactual comparison requires --changed".into());
    }

    let same_spec = champion["spec"] == candidate["spec"];
    let same_state = champion["state"]["fingerprint"] == candidate["state"]["fingerprint"];
    if !same_spec && !changed.contains("spec") {
        return Err(
            "decisions: compared receipts use different specs without declaring changed dimension spec"
                .into(),
        );
    }
    if !same_state && !changed.contains("state") && !changed.contains("evidence") {
        return Err(
            "decisions: compared receipts use different states without declaring state/evidence change"
                .into(),
        );
    }

    let result_match = champion["status"] == candidate["status"]
        && champion.get("result").cloned().unwrap_or(Value::Null)
            == candidate.get("result").cloned().unwrap_or(Value::Null);
    let disposition_match = receipt_disposition(champion) == receipt_disposition(candidate);
    let confidence_delta = match (receipt_confidence(champion), receipt_confidence(candidate)) {
        (Some(left), Some(right)) => Some(right - left),
        _ => None,
    };

    let mut metrics = vec![
        json!({"name":"result_match","value": if result_match {1.0} else {0.0},"unit":"ratio"}),
        json!({"name":"disposition_match","value": if disposition_match {1.0} else {0.0},"unit":"ratio"}),
    ];
    if let Some(delta) = confidence_delta {
        metrics.push(json!({
            "name":"provider_confidence_delta",
            "value":delta,
            "unit":"absolute"
        }));
    }

    let evaluation = json!({
        "format_version": 1,
        "kind": "decision-evaluation",
        "id": canonical_fingerprint(&json!({
            "champion": champion["id"],
            "candidate": candidate["id"],
            "mode": mode,
            "dataset": dataset_id,
            "revision": dataset_revision,
            "generated_at": generated_at,
            "changed": changed_dimensions
        }))?.0.replacen("sha256:", "eval-", 1),
        "mode": mode,
        "dataset": {
            "id": dataset_id,
            "revision": dataset_revision
        },
        "champion": {
            "provider_id": receipt_provider_id(champion),
            "policy_id": champion["policy"]["id"],
            "revision": champion["policy"]["revision"]
        },
        "candidate": {
            "provider_id": receipt_provider_id(candidate),
            "policy_id": candidate["policy"]["id"],
            "revision": candidate["policy"]["revision"]
        },
        "side_effects": false,
        "changed_dimensions": changed_dimensions,
        "source_receipts": [champion["id"], candidate["id"]],
        "metrics": metrics,
        "coverage": {"total":1,"evaluated":1,"abstained":0,"failed":0},
        "generated_at": generated_at,
        "notes": "Receipt comparison only; no provider call, side effect, authorization, or outcome prediction was performed."
    });
    validate_evaluation(&evaluation)?;
    Ok(evaluation)
}

fn usage(program: &str) {
    println!(
        "Agentic Harness Decisions\n\nusage:\n  {program} validate ARTIFACT.json\n  {program} fingerprint STATE.json\n  {program} plan GRAPH.json SPECS.json STATE.json [--provider ID] [--mode MODE]\n  {program} replay GRAPH.json RECEIPTS.json\n  {program} outcome RECEIPT.json --id ID --observed-at RFC3339 --label LABEL --verification TYPE [--verification-ref REF] [--action-ref REF] [--usable true|false] [--success true|false]\n  {program} compare-receipts CHAMPION.json CANDIDATE.json --mode shadow|champion-challenger|counterfactual --dataset ID --revision REV --generated-at RFC3339 [--changed provider,model,...]\n  {program} calibration-report DATASET.json --generated-at RFC3339 [--bins 10] [--target-accuracy N --min-coverage N --min-samples N]\n  {program} calibration-compare BASELINE.json CANDIDATE.json --generated-at RFC3339 [--max-accuracy-drop N] [--max-coverage-drop N] [--max-brier-increase N] [--max-ece-increase N] [--max-ordinal-mae-increase N] [--max-latency-increase-ms N] [--max-cost-increase-usd N]\n  {program} jev-payload REQUEST.json SPECS.json [--model MODEL]\n  {program} jev-evaluate REQUEST.json SPECS.json --allow-network --decided-at RFC3339 [--evidence EVIDENCE.json] [--model MODEL] [--timeout-ms 10000] [--max-retries 2]\n  {program} jev-receipts REQUEST.json SPECS.json RESPONSE.json --decided-at RFC3339 [--evidence EVIDENCE.json]\n\ncommands:\n  validate      Validate a Decision Kernel v1 artifact plus semantic invariants\n  fingerprint   Canonicalize explicit JSON state and emit its SHA-256 identity\n  plan          Topologically stage a DecisionGraph into parallel fan-out batches and cache identities\n  replay        Reconstruct graph inputs from recorded receipts without provider calls\n  outcome       Append an observed outcome/verification artifact without rewriting its receipt\n  compare-receipts  Build side-effect-free shadow/challenger/counterfactual evaluation evidence\n  calibration-report  Compute offline quality/calibration metrics and optionally tune a threshold on calibration split\n  calibration-compare  Apply deterministic regression budgets to two calibration reports\n  jev-payload   Construct the documented TypeSafe Jev request without making a network call\n  jev-evaluate  Opt in to hosted TypeSafe evaluation and normalize the live response into review-required receipts\n  jev-receipts  Normalize a recorded Jev response into canonical review-required DecisionReceipts"
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
                        "calibration datasets preserve one exact spec/state-schema/provider identity",
                        "threshold tuning is calibration-split only",
                        "regression pass/fail matches explicit failure evidence",
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
        "plan" => {
            let graph_path = PathBuf::from(&args[2]);
            let specs_path = PathBuf::from(&args[3]);
            let state_path = PathBuf::from(&args[4]);
            let mut provider: Option<String> = None;
            let mut mode = "live".to_string();
            let mut index = 5;
            while index < args.len() {
                match args[index].as_str() {
                    "--provider" => {
                        index += 1;
                        provider = Some(args[index].clone());
                    }
                    "--mode" => {
                        index += 1;
                        mode = args[index].clone();
                    }
                    option => crate::fail(format!("decisions: unknown plan option: {option}")),
                }
                index += 1;
            }
            let graph = read_json(&graph_path).unwrap_or_else(|error| crate::fail(error));
            let specs = read_json(&specs_path).unwrap_or_else(|error| crate::fail(error));
            let state = read_json(&state_path).unwrap_or_else(|error| crate::fail(error));
            let plan = graph_plan(&graph, &specs, &state, provider.as_deref(), &mode)
                .unwrap_or_else(|error| crate::fail(error));
            println!("{}", serde_json::to_string_pretty(&plan).unwrap());
        }
        "replay" => {
            let graph = read_json(Path::new(&args[2])).unwrap_or_else(|error| crate::fail(error));
            let receipts =
                read_json(Path::new(&args[3])).unwrap_or_else(|error| crate::fail(error));
            let replay =
                replay_receipts(&graph, &receipts).unwrap_or_else(|error| crate::fail(error));
            println!("{}", serde_json::to_string_pretty(&replay).unwrap());
        }
        "outcome" => {
            let receipt = read_json(Path::new(&args[2])).unwrap_or_else(|error| crate::fail(error));
            let mut id: Option<String> = None;
            let mut observed_at: Option<String> = None;
            let mut label: Option<String> = None;
            let mut verification = "unknown".to_string();
            let mut verification_ref: Option<String> = None;
            let mut action_ref: Option<String> = None;
            let mut usable = true;
            let mut success: Option<bool> = None;
            let mut index = 3;
            while index < args.len() {
                let flag = args[index].as_str();
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| crate::fail(format!("decisions: {flag} requires a value")));
                match flag {
                    "--id" => id = Some(value),
                    "--observed-at" => observed_at = Some(value),
                    "--label" => label = Some(value),
                    "--verification" => verification = value,
                    "--verification-ref" => verification_ref = Some(value),
                    "--action-ref" => action_ref = Some(value),
                    "--usable" => {
                        usable = value.parse::<bool>().unwrap_or_else(|_| {
                            crate::fail("decisions: --usable expects true|false")
                        });
                    }
                    "--success" => {
                        success = Some(value.parse::<bool>().unwrap_or_else(|_| {
                            crate::fail("decisions: --success expects true|false")
                        }));
                    }
                    option => crate::fail(format!("decisions: unknown outcome option: {option}")),
                }
                index += 1;
            }
            let outcome = outcome_from_receipt(
                &receipt,
                OutcomeOptions {
                    id: id
                        .as_deref()
                        .unwrap_or_else(|| crate::fail("decisions: --id is required")),
                    observed_at: observed_at
                        .as_deref()
                        .unwrap_or_else(|| crate::fail("decisions: --observed-at is required")),
                    label: label
                        .as_deref()
                        .unwrap_or_else(|| crate::fail("decisions: --label is required")),
                    verification_type: &verification,
                    verification_ref: verification_ref.as_deref(),
                    action_ref: action_ref.as_deref(),
                    usable_for_evaluation: usable,
                    success,
                },
            )
            .unwrap_or_else(|error| crate::fail(error));
            println!("{}", serde_json::to_string_pretty(&outcome).unwrap());
        }
        "compare-receipts" => {
            let champion =
                read_json(Path::new(&args[2])).unwrap_or_else(|error| crate::fail(error));
            let candidate =
                read_json(Path::new(&args[3])).unwrap_or_else(|error| crate::fail(error));
            let mut mode: Option<String> = None;
            let mut dataset: Option<String> = None;
            let mut revision: Option<String> = None;
            let mut generated_at: Option<String> = None;
            let mut changed: Vec<String> = Vec::new();
            let mut index = 4;
            while index < args.len() {
                let flag = args[index].as_str();
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| crate::fail(format!("decisions: {flag} requires a value")));
                match flag {
                    "--mode" => mode = Some(value),
                    "--dataset" => dataset = Some(value),
                    "--revision" => revision = Some(value),
                    "--generated-at" => generated_at = Some(value),
                    "--changed" => {
                        changed = value
                            .split(',')
                            .filter(|item| !item.is_empty())
                            .map(str::to_string)
                            .collect();
                    }
                    option => crate::fail(format!(
                        "decisions: unknown compare-receipts option: {option}"
                    )),
                }
                index += 1;
            }
            let evaluation = compare_receipts(
                &champion,
                &candidate,
                mode.as_deref()
                    .unwrap_or_else(|| crate::fail("decisions: --mode is required")),
                dataset
                    .as_deref()
                    .unwrap_or_else(|| crate::fail("decisions: --dataset is required")),
                revision
                    .as_deref()
                    .unwrap_or_else(|| crate::fail("decisions: --revision is required")),
                generated_at
                    .as_deref()
                    .unwrap_or_else(|| crate::fail("decisions: --generated-at is required")),
                &changed,
            )
            .unwrap_or_else(|error| crate::fail(error));
            println!("{}", serde_json::to_string_pretty(&evaluation).unwrap());
        }
        "calibration-report" => {
            let dataset = read_json(Path::new(&args[2])).unwrap_or_else(|error| crate::fail(error));
            let mut bins = 10_usize;
            let mut target_accuracy: Option<f64> = None;
            let mut minimum_coverage: Option<f64> = None;
            let mut minimum_samples = 30_usize;
            let mut generated_at: Option<String> = None;
            let mut index = 3;
            while index < args.len() {
                let flag = args[index].as_str();
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| crate::fail(format!("decisions: {flag} requires a value")));
                match flag {
                    "--bins" => {
                        bins = value.parse::<usize>().unwrap_or_else(|_| {
                            crate::fail("decisions: --bins must be an integer")
                        });
                    }
                    "--target-accuracy" => {
                        target_accuracy = Some(value.parse::<f64>().unwrap_or_else(|_| {
                            crate::fail("decisions: --target-accuracy must be numeric")
                        }));
                    }
                    "--min-coverage" => {
                        minimum_coverage = Some(value.parse::<f64>().unwrap_or_else(|_| {
                            crate::fail("decisions: --min-coverage must be numeric")
                        }));
                    }
                    "--min-samples" => {
                        minimum_samples = value.parse::<usize>().unwrap_or_else(|_| {
                            crate::fail("decisions: --min-samples must be an integer")
                        });
                    }
                    "--generated-at" => generated_at = Some(value),
                    option => crate::fail(format!(
                        "decisions: unknown calibration-report option: {option}"
                    )),
                }
                index += 1;
            }
            let report = crate::decision_calibration::calibrate_dataset(
                &dataset,
                &crate::decision_calibration::CalibrationOptions {
                    bins,
                    target_accuracy,
                    minimum_coverage,
                    minimum_samples,
                    generated_at: generated_at
                        .unwrap_or_else(|| crate::fail("decisions: --generated-at is required")),
                },
            )
            .unwrap_or_else(|error| crate::fail(error));
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
        "calibration-compare" => {
            let baseline =
                read_json(Path::new(&args[2])).unwrap_or_else(|error| crate::fail(error));
            let candidate =
                read_json(Path::new(&args[3])).unwrap_or_else(|error| crate::fail(error));
            let mut max_accuracy_drop = 0.0_f64;
            let mut max_coverage_drop = 0.0_f64;
            let mut max_brier_increase = 0.0_f64;
            let mut max_ece_increase = 0.0_f64;
            let mut max_ordinal_mae_increase = 0.0_f64;
            let mut max_mean_latency_increase_ms: Option<f64> = None;
            let mut max_total_cost_increase_usd: Option<f64> = None;
            let mut generated_at: Option<String> = None;
            let mut index = 4;
            while index < args.len() {
                let flag = args[index].as_str();
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| crate::fail(format!("decisions: {flag} requires a value")));
                let numeric = || {
                    value.parse::<f64>().unwrap_or_else(|_| {
                        crate::fail(format!("decisions: {flag} must be numeric"))
                    })
                };
                match flag {
                    "--max-accuracy-drop" => max_accuracy_drop = numeric(),
                    "--max-coverage-drop" => max_coverage_drop = numeric(),
                    "--max-brier-increase" => max_brier_increase = numeric(),
                    "--max-ece-increase" => max_ece_increase = numeric(),
                    "--max-ordinal-mae-increase" => max_ordinal_mae_increase = numeric(),
                    "--max-latency-increase-ms" => max_mean_latency_increase_ms = Some(numeric()),
                    "--max-cost-increase-usd" => max_total_cost_increase_usd = Some(numeric()),
                    "--generated-at" => generated_at = Some(value),
                    option => crate::fail(format!(
                        "decisions: unknown calibration-compare option: {option}"
                    )),
                }
                index += 1;
            }
            let report = crate::decision_calibration::compare_calibrations(
                &baseline,
                &candidate,
                &crate::decision_calibration::RegressionBudgets {
                    max_accuracy_drop,
                    max_coverage_drop,
                    max_brier_increase,
                    max_ece_increase,
                    max_ordinal_mae_increase,
                    max_mean_latency_increase_ms,
                    max_total_cost_increase_usd,
                    generated_at: generated_at.unwrap_or_else(|| {
                        crate::fail("decisions: --generated-at is required")
                    }),
                },
            )
            .unwrap_or_else(|error| crate::fail(error));
            let passed = report["passed"].as_bool().unwrap_or(false);
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            if !passed {
                crate::finish(1);
            }
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
                    option => {
                        crate::fail(format!("decisions: unknown jev-payload option: {option}"))
                    }
                }
                index += 1;
            }
            let request = read_json(&request_path).unwrap_or_else(|error| crate::fail(error));
            let specs = read_json(&specs_path).unwrap_or_else(|error| crate::fail(error));
            let payload =
                jev_payload(&request, &specs, &model).unwrap_or_else(|error| crate::fail(error));
            println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        }
        "jev-evaluate" => {
            let request = read_json(Path::new(&args[2])).unwrap_or_else(|error| crate::fail(error));
            let specs = read_json(Path::new(&args[3])).unwrap_or_else(|error| crate::fail(error));
            let mut model = "jev-latest".to_string();
            let mut timeout_ms = 10_000_u64;
            let mut max_retries = 2_u32;
            let mut decided_at: Option<String> = None;
            let mut evidence: Option<Value> = None;
            let mut allow_network = false;
            let mut index = 4;
            while index < args.len() {
                match args[index].as_str() {
                    "--model" => {
                        index += 1;
                        model = args[index].clone();
                    }
                    "--timeout-ms" => {
                        index += 1;
                        timeout_ms = args[index].parse::<u64>().unwrap_or_else(|_| {
                            crate::fail("decisions: --timeout-ms must be an unsigned integer")
                        });
                    }
                    "--max-retries" => {
                        index += 1;
                        max_retries = args[index].parse::<u32>().unwrap_or_else(|_| {
                            crate::fail("decisions: --max-retries must be an unsigned integer")
                        });
                    }
                    "--decided-at" => {
                        index += 1;
                        decided_at = Some(args[index].clone());
                    }
                    "--evidence" => {
                        index += 1;
                        evidence = Some(
                            read_json(Path::new(&args[index]))
                                .unwrap_or_else(|error| crate::fail(error)),
                        );
                    }
                    "--allow-network" => {
                        allow_network = true;
                    }
                    option => {
                        crate::fail(format!("decisions: unknown jev-evaluate option: {option}"))
                    }
                }
                index += 1;
            }

            if !allow_network {
                provider_fail_code(
                    "provider-network-disabled",
                    "hosted Jev evaluation is disabled unless --allow-network is explicitly supplied",
                );
            }

            let decided_at = decided_at
                .as_deref()
                .unwrap_or_else(|| crate::fail("decisions: --decided-at is required"));
            let transport = TransportOptions {
                timeout_ms,
                max_retries,
            }
            .validate()
            .unwrap_or_else(|error| provider_fail(error));
            let payload =
                jev_payload(&request, &specs, &model).unwrap_or_else(|error| crate::fail(error));
            let provider_request = payload.get("request").cloned().unwrap_or_else(|| {
                crate::fail("decisions: Jev provider payload is missing request")
            });
            let api_key =
                jev_transport::api_key_from_env().unwrap_or_else(|error| provider_fail(error));
            let run = jev_transport::execute(&provider_request, &api_key, transport)
                .unwrap_or_else(|error| provider_fail(error));

            if let Err(error) = validate_jev_response(&run.response) {
                provider_fail(ProviderError {
                    code: "provider-malformed-response",
                    message: error,
                    status: Some(200),
                    attempts: run.attempts,
                });
            }

            let mut receipts = normalize_jev_receipts(
                &request,
                &specs,
                &run.response,
                evidence.as_ref(),
                decided_at,
            )
            .unwrap_or_else(|error| {
                provider_fail(ProviderError {
                    code: "provider-malformed-response",
                    message: error,
                    status: Some(200),
                    attempts: run.attempts,
                })
            });
            receipts["network_call_performed"] = json!(true);
            receipts["transport"] = json!({
                "endpoint": JEV_ENDPOINT,
                "https_only": true,
                "redirects_followed": false,
                "proxy_from_environment": false,
                "attempts": run.attempts,
                "timeout_ms": timeout_ms,
                "max_retries": max_retries,
                "elapsed_ms": run.elapsed_ms,
                "credential_source": "TYPESAFE_API_KEY",
                "authorization_header": "Bearer <redacted>",
                "cost_usd": Value::Null
            });
            receipts["usage"] = run.response.get("usage").cloned().unwrap_or(Value::Null);
            receipts["not_checked"] = json!([
                "provider calibration on this decision class",
                "billing cost when the provider does not return cost",
                "authorization for consequential application actions"
            ]);
            println!("{}", serde_json::to_string_pretty(&receipts).unwrap());
        }
        "jev-receipts" => {
            let request = read_json(Path::new(&args[2])).unwrap_or_else(|error| crate::fail(error));
            let specs = read_json(Path::new(&args[3])).unwrap_or_else(|error| crate::fail(error));
            let response =
                read_json(Path::new(&args[4])).unwrap_or_else(|error| crate::fail(error));
            let mut decided_at: Option<String> = None;
            let mut evidence: Option<Value> = None;
            let mut index = 5;
            while index < args.len() {
                match args[index].as_str() {
                    "--decided-at" => {
                        index += 1;
                        decided_at = Some(args[index].clone());
                    }
                    "--evidence" => {
                        index += 1;
                        evidence = Some(
                            read_json(Path::new(&args[index]))
                                .unwrap_or_else(|error| crate::fail(error)),
                        );
                    }
                    option => {
                        crate::fail(format!("decisions: unknown jev-receipts option: {option}"))
                    }
                }
                index += 1;
            }
            let decided_at =
                decided_at.unwrap_or_else(|| crate::fail("decisions: --decided-at is required"));
            let receipts =
                normalize_jev_receipts(&request, &specs, &response, evidence.as_ref(), &decided_at)
                    .unwrap_or_else(|error| crate::fail(error));
            println!("{}", serde_json::to_string_pretty(&receipts).unwrap());
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
        assert!(
            validate_graph(&graph)
                .unwrap_err()
                .contains("not a decision node")
        );
    }

    #[test]
    fn provider_and_evaluation_cannot_grant_side_effect_authority() {
        let provider = json!({
            "format_version":1,"kind":"decision-provider-profile","id":"jev","revision":1,
            "provider_type":"jev","decision_kinds":["boolean"],
            "confidence":{"semantics":"provider-defined","supports_distribution":true},
            "external_network":true,"requires_credentials":true,"consequence_authority":true
        });
        assert!(
            validate_provider(&provider)
                .unwrap_err()
                .contains("consequence authority")
        );

        let evaluation = json!({
            "format_version":1,"kind":"decision-evaluation","id":"eval","mode":"shadow",
            "dataset":{"id":"d","revision":1},"candidate":{"provider_id":"jev"},
            "side_effects":true,"metrics":[],"coverage":{"total":0,"evaluated":0,"abstained":0,"failed":0},
            "generated_at":"2026-09-18T00:00:00Z"
        });
        assert!(
            validate_evaluation(&evaluation)
                .unwrap_err()
                .contains("side effects")
        );
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
        assert!(
            validate_receipt(&receipt)
                .unwrap_err()
                .contains("outcome_probability")
        );
        receipt
            .as_object_mut()
            .unwrap()
            .remove("outcome_probability");
        receipt["uncertainty"]["evidence_coverage"]["value"] = json!(1.0);
        assert!(
            validate_receipt(&receipt)
                .unwrap_err()
                .contains("coverage arithmetic")
        );
    }

    #[test]
    fn fingerprint_is_stable_across_object_key_order() {
        let a: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
        assert_eq!(
            canonical_fingerprint(&a).unwrap(),
            canonical_fingerprint(&b).unwrap()
        );
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
        let payload =
            jev_payload(&request, &json!([boolean, choice, ordinal]), "jev-latest").unwrap();
        assert_eq!(payload["request"]["questions"]["is-risky"]["type"], "noul");
        assert_eq!(
            payload["request"]["questions"]["task-type"]["type"],
            "choice"
        );
        assert_eq!(
            payload["request"]["questions"]["complexity"]["type"],
            "score"
        );
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
        assert!(
            validate_jev_response(&bad)
                .unwrap_err()
                .contains("sum to 1")
        );
    }

    #[test]
    fn graph_plan_batches_independent_nodes_and_stabilizes_cache_keys() {
        let mut risk = spec("boolean");
        risk["id"] = json!("task.risk");
        let mut route = spec("choice");
        route["id"] = json!("task.route");
        let mut complexity = spec("ordinal");
        complexity["id"] = json!("task.complexity");
        let specs = json!([risk, route, complexity]);
        let graph = json!({
            "format_version":1,"kind":"decision-graph","id":"task.plan","revision":1,
            "nodes":[
                {"id":"risk","spec_id":"task.risk","spec_revision":1,"depends_on":[]},
                {"id":"complexity","spec_id":"task.complexity","spec_revision":1,"depends_on":[]},
                {"id":"route","spec_id":"task.route","spec_revision":1,"depends_on":["risk","complexity"]}
            ],
            "reducers":[]
        });
        let state = json!({"task":"review this change"});
        let first = graph_plan(&graph, &specs, &state, Some("typesafe-jev"), "shadow").unwrap();
        let second = graph_plan(&graph, &specs, &state, Some("typesafe-jev"), "shadow").unwrap();
        assert_eq!(first["stages"].as_array().unwrap().len(), 2);
        assert_eq!(first["stages"][0]["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(first["stages"][1]["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(
            first["stages"][0]["nodes"][0]["cache_key"],
            second["stages"][0]["nodes"][0]["cache_key"]
        );
        assert_eq!(first["provider_calls_performed"], false);
        assert_eq!(first["side_effects"], false);
    }

    #[test]
    fn jev_receipts_are_review_required_and_preserve_evidence_coverage() {
        let mut boolean = spec("boolean");
        boolean["id"] = json!("task.risk");
        boolean["evidence"]["requirements"] = json!([
            {"id":"ticket","required":true,"description":"The original task or ticket"},
            {"id":"diff","required":true,"description":"The candidate change"}
        ]);
        let specs = json!([boolean]);
        let request = json!({
            "format_version":1,"kind":"decision-request","request_id":"request-1",
            "state":{"schema_id":"task-state","schema_version":1,"fingerprint":"sha256:12345678","payload":{"task":"review"}},
            "questions":[{"spec_id":"task.risk","spec_revision":1}],
            "mode":"shadow"
        });
        let response = json!({
            "model":"jev-1.13.0",
            "answers":{"task.risk":{"type":"noul","noul":0.92}},
            "usage":{"input_tokens":100,"output_tokens":5}
        });
        let evidence = json!({
            "task.risk":{
                "used":["evidence:ticket:1"],
                "satisfied_requirements":["ticket"]
            }
        });
        let set = normalize_jev_receipts(
            &request,
            &specs,
            &response,
            Some(&evidence),
            "2026-09-18T19:30:00Z",
        )
        .unwrap();
        let receipt = &set["receipts"][0];
        assert_eq!(receipt["provider"]["model"], "jev-1.13.0");
        assert_eq!(receipt["policy"]["disposition"], "review");
        assert_eq!(receipt["uncertainty"]["evidence_coverage"]["value"], 0.5);
        assert_eq!(receipt["uncertainty"]["calibration"]["status"], "unknown");
        assert_eq!(set["consequence_authorized"], false);
    }

    #[test]
    fn replay_uses_recorded_receipts_without_provider_calls() {
        let graph = json!({
            "format_version":1,"kind":"decision-graph","id":"task.graph","revision":1,
            "nodes":[{"id":"risk","spec_id":"task.risk","spec_revision":1,"depends_on":[]}],
            "reducers":[{"id":"summary","type":"deterministic","inputs":["risk"],"output":"task.summary"}]
        });
        let receipt = json!({
            "format_version":1,"kind":"decision-receipt","id":"decision-1",
            "spec":{"id":"task.risk","revision":1},
            "state":{"schema_id":"task-state","schema_version":1,"fingerprint":"sha256:12345678"},
            "status":"produced","result":{"value":true,"distribution":{"false":0.1,"true":0.9}},
            "provider":{"type":"jev","id":"typesafe-jev","model":"jev-1.13.0"},
            "uncertainty":{
                "provider_confidence":0.9,"calibration":{"status":"unknown"},
                "evidence_coverage":{"value":1.0,"required_present":0,"required_total":0,"missing":[]},
                "evidence_reliability":null,"decision_certainty":null
            },
            "evidence":{"used":[],"missing":[]},
            "policy":{"id":"unapplied","revision":1,"disposition":"review","reasons":[]},
            "timing":{"decided_at":"2026-09-18T19:30:00Z"}
        });
        let replay = replay_receipts(&graph, &json!([receipt])).unwrap();
        assert_eq!(replay["complete"], true);
        assert_eq!(replay["provider_calls_performed"], false);
        assert_eq!(replay["reducers"][0]["executed"], false);
        assert_eq!(
            replay["reducers"][0]["inputs"][0]["receipt_id"],
            "decision-1"
        );
    }

    fn comparison_receipt(id: &str, provider: &str, value: bool, confidence: f64) -> Value {
        json!({
            "format_version":1,"kind":"decision-receipt","id":id,
            "spec":{"id":"task.risk","revision":1},
            "state":{"schema_id":"task-state","schema_version":1,"fingerprint":"sha256:12345678"},
            "status":"produced",
            "result":{"value":value,"distribution":{"false":1.0-confidence,"true":confidence}},
            "provider":{"type":"custom","id":provider},
            "uncertainty":{
                "provider_confidence":confidence,
                "calibration":{"status":"unknown"},
                "evidence_coverage":{"value":1.0,"required_present":0,"required_total":0,"missing":[]},
                "evidence_reliability":null,
                "decision_certainty":null
            },
            "evidence":{"used":[],"missing":[]},
            "policy":{"id":"policy:test","revision":1,"disposition":"review","reasons":[]},
            "timing":{"decided_at":"2026-09-18T20:00:00Z"}
        })
    }

    #[test]
    fn outcomes_append_feedback_without_rewriting_receipts() {
        let receipt = comparison_receipt("decision-one", "provider-a", true, 0.9);
        let original = receipt.clone();
        let outcome = outcome_from_receipt(
            &receipt,
            OutcomeOptions {
                id: "outcome-one",
                observed_at: "2026-09-19T10:00:00Z",
                label: "confirmed-regression",
                verification_type: "human",
                verification_ref: Some("review:42"),
                action_ref: Some("issue:99"),
                usable_for_evaluation: true,
                success: Some(true),
            },
        )
        .unwrap();
        assert_eq!(receipt, original);
        assert_eq!(outcome["receipt_id"], "decision-one");
        assert_eq!(outcome["verification"]["type"], "human");
        assert_eq!(outcome["feedback"]["usable_for_evaluation"], true);
        assert_eq!(outcome["outcome"]["success"], true);
        assert!(validate_outcome(&outcome).is_ok());
    }

    #[test]
    fn shadow_and_counterfactual_comparisons_are_side_effect_free() {
        let champion = comparison_receipt("decision-a", "provider-a", false, 0.8);
        let candidate = comparison_receipt("decision-b", "provider-b", true, 0.7);
        let shadow = compare_receipts(
            &champion,
            &candidate,
            "shadow",
            "dataset:test",
            "1",
            "2026-09-18T20:30:00Z",
            &["provider".to_string()],
        )
        .unwrap();
        assert_eq!(shadow["side_effects"], false);
        assert_eq!(shadow["metrics"][0]["name"], "result_match");
        assert_eq!(shadow["metrics"][0]["value"], 0.0);
        assert_eq!(shadow["source_receipts"][0], "decision-a");
        assert_eq!(shadow["source_receipts"][1], "decision-b");

        assert!(
            compare_receipts(
                &champion,
                &candidate,
                "counterfactual",
                "dataset:test",
                "1",
                "2026-09-18T20:30:00Z",
                &[],
            )
            .unwrap_err()
            .contains("--changed")
        );
        let counterfactual = compare_receipts(
            &champion,
            &candidate,
            "counterfactual",
            "dataset:test",
            "1",
            "2026-09-18T20:30:00Z",
            &["provider".to_string(), "threshold".to_string()],
        )
        .unwrap();
        assert_eq!(counterfactual["mode"], "counterfactual");
        assert_eq!(counterfactual["side_effects"], false);
    }
}
