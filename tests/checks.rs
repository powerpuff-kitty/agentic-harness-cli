use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn policy() -> Value {
    json!({
        "format_version":1,"kind":"check-policy","inputs":["src"],
        "checks":[{"id":"unit","argv":["sh","-c","touch MUST_NOT_EXIST"],
            "cwd":".","required":true,"timeout_ms":60000,"max_output_bytes":65536}],
        "required_controls":[{"rule_id":"architecture.boundaries","capability":"enforced"}],
        "max_age_ms":3600000
    })
}

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join(".agentic")).unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/main.ts"),
        "export const answer = 42;\n",
    )
    .unwrap();
    save(temp.path(), &policy());
    temp
}

fn save(root: &Path, value: &Value) {
    fs::write(root.join(".agentic/checks.json"), value.to_string()).unwrap();
}

fn command(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ah"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn plan(root: &Path) -> Value {
    let output = command(root, &["checks", "plan"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn rejects(root: &Path, args: &[&str]) {
    let output = command(root, args);
    assert_eq!(output.status.code(), Some(2));
    let diagnostic: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(diagnostic["kind"], "diagnostic");
    assert!(output.stdout.is_empty());
}

#[test]
fn check_preview_is_deterministic_and_never_executes_or_approves() {
    let temp = fixture();
    let before = fs::read(temp.path().join(".agentic/checks.json")).unwrap();
    let one = plan(temp.path());
    assert_eq!(one, plan(temp.path()));
    for key in [
        "checks_executed",
        "execution_permitted",
        "executable_identity_verified",
    ] {
        assert_eq!(one[key], false);
    }
    assert_eq!(one["control_requirements"][0]["status"], "unverified");
    assert!(!temp.path().join("MUST_NOT_EXIST").exists());
    assert_eq!(
        before,
        fs::read(temp.path().join(".agentic/checks.json")).unwrap()
    );
}

#[test]
fn source_edits_additions_and_removals_change_review_identity() {
    let temp = fixture();
    let baseline = plan(temp.path())["review_digest"].clone();
    fs::write(temp.path().join("src/extra.ts"), "extra").unwrap();
    assert_ne!(baseline, plan(temp.path())["review_digest"]);
    fs::remove_file(temp.path().join("src/extra.ts")).unwrap();
    assert_eq!(baseline, plan(temp.path())["review_digest"]);
    fs::write(temp.path().join("src/main.ts"), "changed").unwrap();
    assert_ne!(baseline, plan(temp.path())["review_digest"]);
}

#[test]
fn policy_byte_changes_invalidate_preview_even_with_same_semantics() {
    let temp = fixture();
    let baseline = plan(temp.path());
    fs::write(
        temp.path().join(".agentic/checks.json"),
        serde_json::to_string_pretty(&policy()).unwrap(),
    )
    .unwrap();
    let changed = plan(temp.path());
    assert_ne!(baseline["policy_digest"], changed["policy_digest"]);
    assert_ne!(baseline["review_digest"], changed["review_digest"]);
    assert_eq!(baseline["source_digest"], changed["source_digest"]);
}

#[test]
fn ignore_rules_do_not_hide_declared_inputs() {
    let temp = fixture();
    fs::write(temp.path().join(".gitignore"), "src/ignored.ts\n").unwrap();
    let baseline = plan(temp.path());
    fs::write(temp.path().join("src/ignored.ts"), "observed").unwrap();
    let changed = plan(temp.path());
    assert_ne!(baseline["source_digest"], changed["source_digest"]);
    assert!(
        changed["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"] == "src/ignored.ts")
    );
}

#[test]
fn empty_directory_changes_are_fingerprinted() {
    let temp = fixture();
    let baseline = plan(temp.path());
    fs::create_dir(temp.path().join("src/empty")).unwrap();
    assert_ne!(
        baseline["source_digest"],
        plan(temp.path())["source_digest"]
    );
}

#[test]
fn overlapping_inputs_do_not_duplicate_snapshot_entries() {
    let temp = fixture();
    let baseline = plan(temp.path());
    let mut value = policy();
    value["inputs"] = json!(["src", "src/main.ts"]);
    save(temp.path(), &value);
    let changed = plan(temp.path());
    assert_eq!(baseline["inputs"], changed["inputs"]);
    assert_ne!(baseline["review_digest"], changed["review_digest"]);
}

#[test]
fn missing_policy_has_no_template_fallback() {
    let temp = fixture();
    fs::remove_file(temp.path().join(".agentic/checks.json")).unwrap();
    rejects(temp.path(), &["checks", "plan"]);
}

#[test]
fn malformed_policy_is_rejected_without_echoing_content() {
    let temp = fixture();
    fs::write(
        temp.path().join(".agentic/checks.json"),
        "{sensitive-example",
    )
    .unwrap();
    let output = command(temp.path(), &["checks", "plan"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("sensitive-example"));
}

#[test]
fn invalid_policy_fields_fail_closed() {
    let temp = fixture();
    for (key, bad) in [
        ("format_version", json!(true)),
        ("kind", json!("unknown")),
        ("checks", json!([])),
        ("inputs", json!([])),
        ("max_age_ms", json!(0)),
        ("max_age_ms", json!(86_400_001)),
        ("unexpected", json!(true)),
    ] {
        let mut value = policy();
        value[key] = bad;
        save(temp.path(), &value);
        rejects(temp.path(), &["checks", "plan"]);
    }
}

#[test]
fn invalid_checks_and_limits_fail_closed() {
    let temp = fixture();
    for (key, bad) in [
        ("argv", json!("sh -c command")),
        ("argv", json!([])),
        ("argv", json!(["../executable"])),
        ("argv", json!(["sh -c"])),
        ("argv", json!(["sh", "embedded\nnewline"])),
        ("required", json!(false)),
        ("required", json!("true")),
        ("timeout_ms", json!(300_001)),
        ("timeout_ms", json!(0)),
        ("max_output_bytes", json!(1_048_577)),
        ("id", json!("bad identifier")),
        ("unexpected", json!(true)),
    ] {
        let mut value = policy();
        value["checks"][0][key] = bad;
        save(temp.path(), &value);
        rejects(temp.path(), &["checks", "plan"]);
    }
}

#[test]
fn duplicate_ids_and_control_requirements_are_rejected() {
    let temp = fixture();
    let mut value = policy();
    let duplicate = value["checks"][0].clone();
    value["checks"].as_array_mut().unwrap().push(duplicate);
    save(temp.path(), &value);
    rejects(temp.path(), &["checks", "plan"]);
    value = policy();
    let duplicate = value["required_controls"][0].clone();
    value["required_controls"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    save(temp.path(), &value);
    rejects(temp.path(), &["checks", "plan"]);
}

#[test]
fn distinct_capabilities_for_same_rule_are_independent() {
    let temp = fixture();
    let mut value = policy();
    value["required_controls"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "rule_id":"architecture.boundaries","capability":"declared"
        }));
    save(temp.path(), &value);
    let preview = plan(temp.path());
    assert_eq!(preview["control_requirements"].as_array().unwrap().len(), 2);
    assert!(
        preview["control_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] == "unverified")
    );
}

#[test]
fn unknown_capabilities_are_rejected() {
    let temp = fixture();
    let mut value = policy();
    value["required_controls"][0]["capability"] = json!("secure");
    save(temp.path(), &value);
    rejects(temp.path(), &["checks", "plan"]);
}

#[test]
fn path_escapes_and_missing_inputs_are_rejected() {
    let temp = fixture();
    for bad in [
        ".",
        "..",
        "../outside",
        "/etc/passwd",
        "C:/outside",
        "src\\main.ts",
        "src/../src",
        "src//main.ts",
        "absent",
    ] {
        let mut value = policy();
        value["inputs"] = json!([bad]);
        save(temp.path(), &value);
        rejects(temp.path(), &["checks", "plan"]);
    }
}

#[test]
fn declared_secret_or_git_paths_are_rejected_not_read() {
    let temp = fixture();
    for name in [".env", ".env.local", ".git"] {
        let path = temp.path().join("src").join(name);
        fs::write(&path, "do not include").unwrap();
        rejects(temp.path(), &["checks", "plan"]);
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn working_directory_is_validated_without_running_it() {
    let temp = fixture();
    for bad in ["../outside", "absent", "src/main.ts"] {
        let mut value = policy();
        value["checks"][0]["cwd"] = json!(bad);
        save(temp.path(), &value);
        rejects(temp.path(), &["checks", "plan"]);
    }
}

#[test]
fn oversized_files_and_policy_are_not_silently_skipped() {
    let temp = fixture();
    fs::write(temp.path().join("src/large.bin"), vec![0u8; 2_000_001]).unwrap();
    rejects(temp.path(), &["checks", "plan"]);
    fs::remove_file(temp.path().join("src/large.bin")).unwrap();
    fs::write(
        temp.path().join(".agentic/checks.json"),
        vec![b' '; 262_145],
    )
    .unwrap();
    rejects(temp.path(), &["checks", "plan"]);
}

#[test]
fn binary_inputs_are_included_in_content_identity() {
    let temp = fixture();
    fs::write(temp.path().join("src/data.bin"), [0, 255, 0]).unwrap();
    let baseline = plan(temp.path());
    fs::write(temp.path().join("src/data.bin"), [0, 255, 1]).unwrap();
    assert_ne!(
        baseline["source_digest"],
        plan(temp.path())["source_digest"]
    );
}

#[test]
fn unsupported_execution_and_approval_flags_are_rejected() {
    let temp = fixture();
    for args in [
        vec!["checks", "run"],
        vec!["checks", "plan", "--apply"],
        vec!["checks", "plan", "--approve", "fake"],
        vec!["checks", "plan", "--write"],
        vec!["checks", "plan", "--config"],
        vec!["checks", "plan", "--config", "../outside"],
        vec![
            "checks",
            "plan",
            "--config",
            ".agentic/checks.json",
            "--config",
            ".agentic/checks.json",
        ],
    ] {
        rejects(temp.path(), &args);
    }
}

#[test]
fn experimental_family_help_is_available() {
    let temp = fixture();
    let output = command(temp.path(), &["checks", "--help"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("read-only"));
}

#[cfg(unix)]
#[test]
fn linked_inputs_config_and_working_directories_are_rejected() {
    use std::os::unix::fs::symlink;
    let temp = fixture();
    symlink(
        temp.path().join(".agentic/checks.json"),
        temp.path().join("src/link"),
    )
    .unwrap();
    rejects(temp.path(), &["checks", "plan"]);
    fs::remove_file(temp.path().join("src/link")).unwrap();
    symlink(temp.path().join("src"), temp.path().join("linked")).unwrap();
    let mut value = policy();
    value["checks"][0]["cwd"] = json!("linked");
    save(temp.path(), &value);
    rejects(temp.path(), &["checks", "plan"]);
    save(temp.path(), &policy());
    symlink(
        temp.path().join(".agentic/checks.json"),
        temp.path().join("config.json"),
    )
    .unwrap();
    rejects(temp.path(), &["checks", "plan", "--config", "config.json"]);
}

#[cfg(windows)]
#[test]
fn windows_junction_inputs_and_cwd_are_rejected() {
    let temp = fixture();
    let created = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(temp.path().join("junction"))
        .arg(temp.path().join("src"))
        .output()
        .unwrap();
    assert!(created.status.success());
    let mut value = policy();
    value["inputs"] = json!(["junction"]);
    save(temp.path(), &value);
    rejects(temp.path(), &["checks", "plan"]);
    value = policy();
    value["checks"][0]["cwd"] = json!("junction");
    save(temp.path(), &value);
    rejects(temp.path(), &["checks", "plan"]);
    fs::remove_dir(temp.path().join("junction")).unwrap();
}
