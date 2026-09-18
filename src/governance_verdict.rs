//! Pure semantic evaluation of governance evidence. No I/O or producer authentication.
//! Trust decisions must come from the caller, outside the imported report.
use crate::{check_inputs, checks, strict_json};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Independently established current input identity, sampled before and after evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentIdentity {
    pub policy_digest: String,
    pub source_digest: String,
    pub scope: Vec<String>,
}

/// Exact bytes accepted by an external trust mechanism, bound to producer identity.
/// Constructing this value is a caller assertion, not cryptographic authentication.
pub struct TrustedReport {
    pub digest: String,
    pub producer_id: String,
    pub producer_version: String,
}

/// Caller-resolved immutable reference bytes. Names are opaque and never fetched.
pub struct Reference {
    pub name: String,
    pub digest: String,
    pub bytes: Vec<u8>,
}

/// Trusted evaluation inputs; none may be taken from the imported report itself.
pub struct Context {
    pub before: CurrentIdentity,
    pub after: CurrentIdentity,
    pub required: Vec<(String, String)>,
    pub max_age_ms: u64,
    pub now_ms: u64,
    pub trusted_reports: Vec<TrustedReport>,
    pub references: Vec<Reference>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    InvalidContext,
    InvalidArtifact,
    UntrustedReport,
    IdentityMismatch,
    InputsChanged,
    StaleOrFuture,
    DuplicateClaim,
    InvalidReference,
    MissingRequired,
    RequiredNotVerified,
}

/// Acceptance covers only the requested governance assertions under caller trust.
/// It never certifies command execution, host enforcement or whole-project completion.
#[derive(Debug, PartialEq, Eq)]
pub struct Verdict {
    pub required_controls_satisfied: bool,
    pub completion_verified: bool,
    pub reports_evaluated: usize,
}

fn digest(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}
fn capability(s: &str) -> bool {
    matches!(s, "declared" | "delivered" | "checked" | "enforced")
}
fn text(v: &Value) -> Result<&str, Rejection> {
    checks::text(v, 4096).map_err(|_| Rejection::InvalidArtifact)
}
fn shape(v: &Value, fields: &[&str]) -> Result<(), Rejection> {
    checks::shape(v, fields)
        .map(|_| ())
        .map_err(|_| Rejection::InvalidArtifact)
}
fn array(v: &Value, min: usize, max: usize) -> Result<&Vec<Value>, Rejection> {
    v.as_array()
        .filter(|v| (min..=max).contains(&v.len()))
        .ok_or(Rejection::InvalidArtifact)
}
fn identity(v: &Value) -> Result<(&str, &str), Rejection> {
    shape(v, &["id", "version"])?;
    let name = text(&v["id"])?;
    if !id(name) {
        return Err(Rejection::InvalidArtifact);
    }
    Ok((name, text(&v["version"])?))
}
fn scope_valid(scope: &[String]) -> bool {
    (1..=64).contains(&scope.len())
        && scope
            .iter()
            .all(|s| s.len() <= 512 && check_inputs::valid_path(s, true))
        && scope.iter().collect::<BTreeSet<_>>().len() == scope.len()
}

/// Evaluate up to 64 reports (8 MiB total), with 64 resolved references (8 MiB total).
/// Rejects the entire set on any malformed, untrusted, stale or conflicting artifact.
pub fn evaluate(context: &Context, reports: &[Vec<u8>]) -> Result<Verdict, Rejection> {
    if context.before != context.after {
        return Err(Rejection::InputsChanged);
    }
    let current = &context.before;
    if !digest(&current.policy_digest)
        || !digest(&current.source_digest)
        || !scope_valid(&current.scope)
        || !(1..=86_400_000).contains(&context.max_age_ms)
        || context.now_ms > 9_007_199_254_740_991
        || context.required.len() > 64
        || context.trusted_reports.len() > 64
        || context.references.len() > 64
        || reports.len() > 64
        || reports.iter().map(Vec::len).sum::<usize>() > 8_388_608
        || context
            .references
            .iter()
            .map(|r| r.bytes.len())
            .sum::<usize>()
            > 8_388_608
    {
        return Err(Rejection::InvalidContext);
    }
    let mut required = BTreeSet::new();
    for (rule, cap) in &context.required {
        if !id(rule) || !capability(cap) || !required.insert((rule.as_str(), cap.as_str())) {
            return Err(Rejection::InvalidContext);
        }
    }
    let mut trusted = BTreeMap::new();
    for trust in &context.trusted_reports {
        if !digest(&trust.digest)
            || !id(&trust.producer_id)
            || checks::text(&Value::String(trust.producer_version.clone()), 4096).is_err()
            || trusted.insert(trust.digest.as_str(), trust).is_some()
        {
            return Err(Rejection::InvalidContext);
        }
    }
    let mut references = BTreeMap::new();
    for reference in &context.references {
        if checks::text(&Value::String(reference.name.clone()), 4096).is_err()
            || !digest(&reference.digest)
            || check_inputs::hash(&reference.bytes) != reference.digest
            || references
                .insert(reference.name.as_str(), reference)
                .is_some()
        {
            return Err(Rejection::InvalidReference);
        }
    }
    let mut claims = BTreeMap::new();
    let mut seen_reports = BTreeSet::new();
    for bytes in reports {
        if bytes.len() > 1_048_576 {
            return Err(Rejection::InvalidArtifact);
        }
        let report_digest = check_inputs::hash(bytes);
        if !seen_reports.insert(report_digest.clone()) {
            return Err(Rejection::DuplicateClaim);
        }
        let trust = trusted
            .get(report_digest.as_str())
            .ok_or(Rejection::UntrustedReport)?;
        let v = strict_json::decode(bytes).map_err(|_| Rejection::InvalidArtifact)?;
        shape(
            &v,
            &[
                "format_version",
                "kind",
                "policy_digest",
                "source_digest",
                "revision",
                "producer",
                "observed_at_ms",
                "host",
                "adapter",
                "scope",
                "claims",
                "not_checked",
            ],
        )?;
        if v["format_version"].as_u64() != Some(1) || v["kind"] != "governance-evidence" {
            return Err(Rejection::InvalidArtifact);
        }
        let (producer, version) = identity(&v["producer"])?;
        if producer != trust.producer_id || version != trust.producer_version {
            return Err(Rejection::UntrustedReport);
        }
        for key in ["host", "adapter"] {
            if !v[key].is_null() {
                identity(&v[key])?;
            }
        }
        if !v["revision"].is_null() {
            text(&v["revision"])?;
        }
        for note in array(&v["not_checked"], 0, 64)? {
            text(note)?;
        }
        let scope: Vec<String> = array(&v["scope"], 1, 64)?
            .iter()
            .map(|x| text(x).map(str::to_owned))
            .collect::<Result<_, _>>()?;
        if !scope_valid(&scope) {
            return Err(Rejection::InvalidArtifact);
        }
        if v["policy_digest"] != current.policy_digest
            || v["source_digest"] != current.source_digest
            || scope.iter().collect::<BTreeSet<_>>()
                != current.scope.iter().collect::<BTreeSet<_>>()
        {
            return Err(Rejection::IdentityMismatch);
        }
        let observed = v["observed_at_ms"]
            .as_u64()
            .filter(|x| *x <= 9_007_199_254_740_991)
            .ok_or(Rejection::InvalidArtifact)?;
        if observed > context.now_ms || context.now_ms - observed > context.max_age_ms {
            return Err(Rejection::StaleOrFuture);
        }
        for claim in array(&v["claims"], 1, 256)? {
            shape(
                claim,
                &[
                    "rule_id",
                    "capability",
                    "status",
                    "mechanism",
                    "evidence_refs",
                ],
            )?;
            let rule = text(&claim["rule_id"])?;
            let cap = text(&claim["capability"])?;
            let status = text(&claim["status"])?;
            if !id(rule)
                || !capability(cap)
                || !matches!(status, "verified" | "unverified" | "unsupported" | "failed")
            {
                return Err(Rejection::InvalidArtifact);
            }
            if !claim["mechanism"].is_null() {
                text(&claim["mechanism"])?;
            }
            let refs = array(&claim["evidence_refs"], 0, 64)?;
            let mut seen = BTreeSet::new();
            for r in refs {
                let name = text(r)?;
                if !seen.insert(name) || !references.contains_key(name) {
                    return Err(Rejection::InvalidReference);
                }
            }
            if status == "verified"
                && (claim["mechanism"].is_null()
                    || refs.is_empty()
                    || (matches!(cap, "delivered" | "enforced") && v["host"].is_null()))
            {
                return Err(Rejection::InvalidArtifact);
            }
            if claims
                .insert((rule.to_owned(), cap.to_owned()), status.to_owned())
                .is_some()
            {
                return Err(Rejection::DuplicateClaim);
            }
        }
    }
    for (rule, cap) in required {
        match claims
            .get(&(rule.to_owned(), cap.to_owned()))
            .map(String::as_str)
        {
            None => return Err(Rejection::MissingRequired),
            Some("verified") => (),
            _ => return Err(Rejection::RequiredNotVerified),
        }
    }
    Ok(Verdict {
        required_controls_satisfied: true,
        completion_verified: false,
        reports_evaluated: reports.len(),
    })
}
