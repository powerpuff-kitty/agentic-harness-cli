use agentic_harness_cli::governance_verdict::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn hash(b: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(b))
}
fn fixture() -> (Context, Value) {
    let identity = CurrentIdentity {
        policy_digest: hash(b"policy"),
        source_digest: hash(b"source"),
        scope: vec!["src".into()],
    };
    let report = json!({"format_version":1,"kind":"governance-evidence","policy_digest":identity.policy_digest,"source_digest":identity.source_digest,
 "revision":null,"producer":{"id":"synthetic","version":"1"},"observed_at_ms":1000,"host":null,"adapter":null,"scope":["src"],
 "claims":[{"rule_id":"boundary","capability":"checked","status":"verified","mechanism":"synthetic detector","evidence_refs":["result"]}],"not_checked":[]});
    (
        Context {
            before: identity.clone(),
            after: identity,
            required: vec![("boundary".into(), "checked".into())],
            max_age_ms: 100,
            now_ms: 1100,
            trusted_reports: vec![],
            references: vec![Reference {
                name: "result".into(),
                digest: hash(b"result bytes"),
                bytes: b"result bytes".to_vec(),
            }],
        },
        report,
    )
}
fn trust(c: &mut Context, bytes: &[u8]) {
    c.trusted_reports.push(TrustedReport {
        digest: hash(bytes),
        producer_id: "synthetic".into(),
        producer_version: "1".into(),
    });
}
fn run(c: &mut Context, v: &Value) -> Result<Verdict, Rejection> {
    let b = serde_json::to_vec(v).unwrap();
    trust(c, &b);
    evaluate(c, &[b])
}
#[test]
fn exact_trusted_current_evidence_satisfies_only_requested_controls() {
    let (mut c, v) = fixture();
    let out = run(&mut c, &v).unwrap();
    assert!(out.required_controls_satisfied);
    assert!(!out.completion_verified);
    assert_eq!(out.reports_evaluated, 1);
}
#[test]
fn report_cannot_grant_itself_trust() {
    let (c, v) = fixture();
    assert_eq!(
        evaluate(&c, &[serde_json::to_vec(&v).unwrap()]),
        Err(Rejection::UntrustedReport)
    );
}
#[test]
fn trusted_digest_does_not_authorize_different_producer_or_version() {
    for field in ["id", "version"] {
        let (mut c, mut v) = fixture();
        v["producer"][field] = json!("other");
        assert_eq!(run(&mut c, &v), Err(Rejection::UntrustedReport));
    }
}
#[test]
fn modifying_authenticated_bytes_revokes_trust() {
    let (mut c, mut v) = fixture();
    trust(&mut c, &serde_json::to_vec(&v).unwrap());
    v["observed_at_ms"] = json!(1099);
    assert_eq!(
        evaluate(&c, &[serde_json::to_vec(&v).unwrap()]),
        Err(Rejection::UntrustedReport)
    );
}
#[test]
fn stale_future_and_invalid_timestamps_fail() {
    for (time, error) in [
        (json!(999), Rejection::StaleOrFuture),
        (json!(1101), Rejection::StaleOrFuture),
        (json!(-1), Rejection::InvalidArtifact),
        (json!(1000.0), Rejection::InvalidArtifact),
        (json!(9007199254740992u64), Rejection::InvalidArtifact),
    ] {
        let (mut c, mut v) = fixture();
        v["observed_at_ms"] = time;
        assert_eq!(run(&mut c, &v), Err(error));
    }
}
#[test]
fn source_policy_and_scope_mismatches_fail() {
    for field in ["source_digest", "policy_digest", "scope"] {
        let (mut c, mut v) = fixture();
        v[field] = if field == "scope" {
            json!(["other"])
        } else {
            json!(hash(b"changed"))
        };
        assert_eq!(run(&mut c, &v), Err(Rejection::IdentityMismatch));
    }
}
#[test]
fn changes_during_evaluation_fail_even_with_valid_reports() {
    let (mut c, v) = fixture();
    c.after.source_digest = hash(b"changed");
    assert_eq!(run(&mut c, &v), Err(Rejection::InputsChanged));
}
#[test]
fn required_capabilities_never_substitute_for_each_other() {
    for cap in ["declared", "delivered", "enforced"] {
        let (mut c, mut v) = fixture();
        v["claims"][0]["capability"] = json!(cap);
        v["host"] = json!({"id":"host","version":"1"});
        assert_eq!(run(&mut c, &v), Err(Rejection::MissingRequired));
    }
}
#[test]
fn nonverified_required_states_fail() {
    for status in ["failed", "unverified", "unsupported"] {
        let (mut c, mut v) = fixture();
        v["claims"][0]["status"] = json!(status);
        assert_eq!(run(&mut c, &v), Err(Rejection::RequiredNotVerified));
    }
}
#[test]
fn duplicate_and_conflicting_claims_fail_within_and_across_reports() {
    for across in [false, true] {
        for status in ["verified", "failed"] {
            let (mut c, mut v) = fixture();
            let mut second = v["claims"][0].clone();
            second["status"] = json!(status);
            if across {
                let mut other = v.clone();
                other["claims"][0] = second;
                other["observed_at_ms"] = json!(1001);
                let a = serde_json::to_vec(&v).unwrap();
                let b = serde_json::to_vec(&other).unwrap();
                trust(&mut c, &a);
                trust(&mut c, &b);
                assert_eq!(evaluate(&c, &[a, b]), Err(Rejection::DuplicateClaim));
            } else {
                v["claims"].as_array_mut().unwrap().push(second);
                assert_eq!(run(&mut c, &v), Err(Rejection::DuplicateClaim));
            }
        }
    }
}
#[test]
fn missing_duplicate_and_changed_reference_bytes_fail() {
    for case in 0..4 {
        let (mut c, mut v) = fixture();
        match case {
            0 => c.references.clear(),
            1 => c.references[0].bytes.push(0),
            2 => v["claims"][0]["evidence_refs"] = json!(["result", "result"]),
            _ => v["claims"][0]["evidence_refs"] = json!(["https://unresolved.invalid/result"]),
        };
        assert_eq!(run(&mut c, &v), Err(Rejection::InvalidReference));
    }
}
#[test]
fn verified_claims_require_mechanism_references_and_host_when_applicable() {
    for case in 0..4 {
        let (mut c, mut v) = fixture();
        match case {
            0 => v["claims"][0]["mechanism"] = Value::Null,
            1 => v["claims"][0]["evidence_refs"] = json!([]),
            2 => v["claims"][0]["capability"] = json!("delivered"),
            _ => v["claims"][0]["capability"] = json!("enforced"),
        };
        assert_eq!(run(&mut c, &v), Err(Rejection::InvalidArtifact));
    }
}
#[test]
fn unknown_fields_and_duplicate_json_keys_fail_even_if_trusted() {
    let (mut c, mut v) = fixture();
    v["trust_me"] = json!(true);
    assert_eq!(run(&mut c, &v), Err(Rejection::InvalidArtifact));
    let (mut c, v) = fixture();
    let b = serde_json::to_string(&v)
        .unwrap()
        .replacen('{', "{\"kind\":\"governance-evidence\",", 1)
        .into_bytes();
    trust(&mut c, &b);
    assert_eq!(evaluate(&c, &[b]), Err(Rejection::InvalidArtifact));
}
#[test]
fn empty_evidence_cannot_satisfy_required_controls() {
    let (c, _) = fixture();
    assert_eq!(evaluate(&c, &[]), Err(Rejection::MissingRequired));
}
#[test]
fn invalid_trust_context_and_resource_limits_fail_closed() {
    for case in 0..5 {
        let (mut c, v) = fixture();
        match case {
            0 => c.max_age_ms = 0,
            1 => c.now_ms = u64::MAX,
            2 => c.required.push(c.required[0].clone()),
            3 => {
                c.before.scope = vec!["../outside".into()];
                c.after = c.before.clone();
            }
            _ => c.required = vec![("x".into(), "unknown".into())],
        };
        assert_eq!(run(&mut c, &v), Err(Rejection::InvalidContext));
    }
    let (mut c, _) = fixture();
    let bytes = vec![b' '; 1_048_577];
    trust(&mut c, &bytes);
    assert_eq!(evaluate(&c, &[bytes]), Err(Rejection::InvalidArtifact));
}
#[test]
fn scope_order_is_not_semantic_but_duplicates_are_invalid() {
    let (mut c, mut v) = fixture();
    c.before.scope.push("tests".into());
    c.after = c.before.clone();
    v["scope"] = json!(["tests", "src"]);
    assert!(run(&mut c, &v).is_ok());
    let (mut c, mut v) = fixture();
    v["scope"] = json!(["src", "src"]);
    assert_eq!(run(&mut c, &v), Err(Rejection::InvalidArtifact));
}
