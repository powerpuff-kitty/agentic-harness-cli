//! Test-only consumer of the canonical exact-byte corpus, not a new validator.
use crate::{artifact, strict_json};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const CORPUS: &[u8] = include_bytes!("../fixtures/evidence-v1.json");
const SOURCE: &str = include_str!("../fixtures/conformance-source.json");
const SCHEMA: &[u8] =
    include_bytes!("../../upstream/agentic-harness/catalog/schema/codebase-audit.v2.schema.json");

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bytes(case: &Value) -> Vec<u8> {
    let text = case["input"].as_str().unwrap();
    match case["encoding"].as_str().unwrap() {
        "utf-8" => text.as_bytes().to_vec(),
        "hex" => {
            assert_eq!(text.len() % 2, 0);
            text.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }
        _ => panic!("unsupported corpus encoding"),
    }
}

fn observe(case: &Value) -> (bool, &'static str) {
    match strict_json::decode(&bytes(case)) {
        Err(_) => (false, "transport"),
        Ok(value) => match case["family"].as_str().unwrap() {
            "strict-json" => (true, "accepted"),
            "codebase-audit-v2" => {
                if artifact::validate(&value).is_ok() {
                    (true, "accepted")
                } else {
                    (false, "schema")
                }
            }
            _ => panic!("unsupported corpus family"),
        },
    }
}

fn matches(case: &Value) -> bool {
    let (accepted, stage) = observe(case);
    accepted == case["expected"].as_bool().unwrap()
        && (accepted || Some(stage) == case["category"].as_str())
}

#[test]
fn canonical_corpus_and_pinned_schema_are_exact() {
    let source: Value = serde_json::from_str(SOURCE).unwrap();
    let corpus = strict_json::decode(CORPUS).unwrap();
    assert_eq!(digest(CORPUS), source["sha256"].as_str().unwrap());
    assert_eq!(digest(SCHEMA), corpus["schema"]["sha256"].as_str().unwrap());
    assert_eq!(corpus["format_version"], 1);
    assert_eq!(corpus["kind"], "evidence-conformance-corpus");
    assert_eq!(corpus["scope"], "transport-and-audit-shape-only");
    assert_eq!(corpus["limits"]["max_input_bytes"], 4096);
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 48);
    let mut ids = BTreeSet::new();
    for case in cases {
        assert!(ids.insert(case["id"].as_str().unwrap()));
        let input = bytes(case);
        assert!(input.len() <= 4096);
        assert_eq!(digest(&input), case["sha256"].as_str().unwrap());
        assert!(case["expected"].is_boolean());
    }
}

#[test]
fn existing_native_decoder_and_audit_validator_match_all_shared_cases() {
    let corpus = strict_json::decode(CORPUS).unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    let mismatches: Vec<&str> = cases
        .iter()
        .filter(|case| !matches(case))
        .map(|case| case["id"].as_str().unwrap())
        .collect();
    assert!(
        mismatches.is_empty(),
        "conformance mismatches: {mismatches:?}"
    );
}

#[test]
fn changed_expectation_is_detected_without_changing_the_consumer() {
    let corpus = strict_json::decode(CORPUS).unwrap();
    let mut case = corpus["cases"][0].clone();
    assert!(matches(&case));
    case["expected"] = Value::Bool(!case["expected"].as_bool().unwrap());
    assert!(!matches(&case));
}
