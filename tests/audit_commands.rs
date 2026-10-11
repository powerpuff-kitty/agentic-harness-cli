//! Real command dispatch, using the same synthetic corpus as the copied-binary probe.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CORPUS: &str = include_str!("fixtures/audit-command-v1.json");

fn bytes(record: &Value) -> Vec<u8> {
    let text = record["input"].as_str().unwrap();
    let data = match record["encoding"].as_str().unwrap() {
        "utf-8" => text.as_bytes().to_vec(),
        "hex" => text
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect(),
        _ => panic!("unsupported fixture encoding"),
    };
    assert!(data.len() <= 4096);
    assert_eq!(
        format!("{:x}", Sha256::digest(&data)),
        record["sha256"].as_str().unwrap()
    );
    data
}

fn field<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn check(case: &Value, output: Output) {
    let id = case["id"].as_str().unwrap();
    let expected = &case["expected"];
    let exit = expected["exit_code"].as_i64().unwrap() as i32;
    assert_eq!(output.status.code(), Some(exit), "{id}");
    let (payload, silent) = if exit == 2 {
        (&output.stderr, &output.stdout)
    } else {
        (&output.stdout, &output.stderr)
    };
    assert!(silent.is_empty(), "{id}: unexpected output channel");
    let result: Value = serde_json::from_slice(payload).expect("structured result");
    assert_eq!(result["format_version"], 1, "{id}");
    assert_eq!(result["kind"], expected["kind"], "{id}");
    for (path, wanted) in expected["fields"].as_object().unwrap() {
        let actual = field(&result, path).expect("expected field must exist");
        if wanted.is_number() {
            assert_eq!(actual.as_f64(), wanted.as_f64(), "{id}: {path}");
        } else {
            assert_eq!(actual, wanted, "{id}: {path}");
        }
    }
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("private-fixture-marker"),
        "{id}: fixture content was echoed"
    );
}

fn command(binary: &Path, root: &Path) -> Command {
    let mut cmd = Command::new(binary);
    cmd.current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("TMPDIR", root)
        .env("TMP", root)
        .env("TEMP", root);
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd
}

fn group(name: &str, count: usize) {
    let corpus: Value = serde_json::from_str(CORPUS).unwrap();
    assert_eq!(corpus["kind"], "audit-command-conformance");
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 87);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let binary: PathBuf = root.join(if cfg!(windows) { "ah.exe" } else { "ah" });
    fs::copy(env!("CARGO_BIN_EXE_ah"), &binary).unwrap();
    let fixtures = corpus["fixtures"].as_object().unwrap();
    for (id, record) in fixtures {
        assert!(
            id.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
        fs::write(root.join(format!("{id}.json")), bytes(record)).unwrap();
    }
    let cases: Vec<&Value> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["group"] == name)
        .collect();
    assert_eq!(cases.len(), count);
    for case in cases {
        let action = case["command"].as_str().unwrap();
        assert!(["gate", "compare"].contains(&action));
        let mut cmd = command(&binary, root);
        cmd.arg(action);
        for input in case["inputs"].as_array().unwrap() {
            let id = input.as_str().unwrap();
            assert!(fixtures.contains_key(id));
            cmd.arg(format!("{id}.json"));
        }
        for option in case["options"].as_array().unwrap() {
            cmd.arg(option.as_str().unwrap());
        }
        check(case, cmd.output().expect("invoke copied ah"));
    }
    for (id, record) in fixtures {
        assert_eq!(
            fs::read(root.join(format!("{id}.json"))).unwrap(),
            bytes(record),
            "inputs must remain unchanged"
        );
    }
}

#[test]
fn command_readers_reject_all_duplicate_key_locations() {
    group("duplicate", 33);
}

#[test]
fn command_readers_reject_malformed_and_nonfinite_inputs() {
    group("invalid", 27);
}

#[test]
fn command_gates_and_comparisons_preserve_supported_behavior() {
    group("preservation", 27);
}
