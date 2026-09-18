//! Synthetic process tests: no user project scripts or credentials are used.
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

#[test]
fn fixture_child() {
    let Ok(mode) = std::env::var("AH_FIXTURE") else {
        return;
    };
    assert!(std::env::var_os("AH_PARENT_ONLY_SYNTHETIC").is_none());
    match mode.as_str() {
        "fail" => panic!("synthetic failure"),
        "timeout" => std::thread::sleep(Duration::from_secs(10)),
        "flood" => {
            let _ = std::io::stdout().write_all(&vec![b'x'; 2_000_000]);
        }
        "mutate" => fs::write("src/main.txt", "changed by approved synthetic check").unwrap(),
        "remove-cwd" => fs::remove_dir("later").unwrap(),
        "cancel-wait" => {
            fs::write("READY", std::process::id().to_string()).unwrap();
            std::thread::sleep(Duration::from_secs(10));
            fs::write("SURVIVED_CANCELLATION", "unexpected").unwrap();
        }
        "touch" => fs::write("MUST_NOT_EXIST", "unexpected").unwrap(),
        "delayed" => {
            std::thread::sleep(Duration::from_millis(500));
            fs::write("LATE_CHILD_MARKER", "unexpected surviving descendant").unwrap();
        }
        "descendant" => {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "fixture_child", "--nocapture"])
                .env("AH_FIXTURE", "delayed")
                .spawn()
                .unwrap();
            // Keep a waiter until this synthetic parent exits; its ordinary
            // same-group descendant must then be stopped by the supervisor.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        _ => println!("SYNTHETIC_OUTPUT_MUST_NOT_BE_PUBLISHED"),
    }
}

#[test]
fn fixture_forced_failure() {
    if std::env::var_os("AH_FIXTURE").is_some() {
        panic!("optional synthetic failure");
    }
}

fn supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

fn fixture(mode: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::create_dir(temp.path().join(".agentic")).unwrap();
    fs::write(temp.path().join("src/main.txt"), "unchanged").unwrap();
    save(
        temp.path(),
        "checks.json",
        &json!({
            "format_version":1,"kind":"check-policy","inputs":["src"],
            "checks":[{"id":"unit","argv":["test-helper","--exact","fixture_child","--nocapture"],
                "cwd":".","required":true,"timeout_ms":2000,"max_output_bytes":65536}],
            "required_controls":[{"rule_id":"fixture.boundary","capability":"enforced"}],
            "max_age_ms":3600000
        }),
    );
    save(
        temp.path(),
        "check-execution.json",
        &json!({
            "format_version":1,"kind":"check-execution-settings",
            "tools":{"test-helper":std::env::current_exe().unwrap()},
            "environment":{"PATH":"","AH_FIXTURE":mode},"max_total_ms":60000
        }),
    );
    temp
}

fn save(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(".agentic").join(name), value.to_string()).unwrap();
}

fn load(root: &Path, name: &str) -> Value {
    serde_json::from_slice(&fs::read(root.join(".agentic").join(name)).unwrap()).unwrap()
}

fn command(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ah"))
        .args(args)
        .current_dir(root)
        .env("AH_PARENT_ONLY_SYNTHETIC", "must not be inherited")
        .output()
        .unwrap()
}

fn prepare(root: &Path) -> Value {
    let output = command(root, &["checks", "prepare"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn execute(root: &Path, review: &Value) -> (Output, Value) {
    let output = command(
        root,
        &[
            "checks",
            "run",
            "--approve-review",
            review["approval_digest"].as_str().unwrap(),
            "--allow-unsandboxed",
        ],
    );
    let value = serde_json::from_slice(&output.stdout).unwrap();
    (output, value)
}

#[test]
fn preparing_is_deterministic_and_never_runs_a_tool() {
    let temp = fixture("touch");
    let review = prepare(temp.path());
    assert_eq!(review, prepare(temp.path()));
    assert_eq!(review["checks_executed"], false);
    assert_eq!(review["execution_permitted"], false);
    assert_eq!(review["inherit_environment"], false);
    assert_eq!(review["execution_supported"], supported());
    assert!(review["tools"]["test-helper"]["runtime_version"].is_null());
    assert!(!temp.path().join("MUST_NOT_EXIST").exists());
}

#[test]
fn execution_needs_review_and_separate_unsandboxed_acknowledgment() {
    let temp = fixture("touch");
    let review = prepare(temp.path());
    for args in [
        vec!["checks", "run"],
        vec!["checks", "run", "--allow-unsandboxed"],
        vec![
            "checks",
            "run",
            "--approve-review",
            review["approval_digest"].as_str().unwrap(),
        ],
        vec![
            "checks",
            "run",
            "--approve-review",
            "wrong",
            "--allow-unsandboxed",
        ],
    ] {
        let output = command(temp.path(), &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    assert!(!temp.path().join("MUST_NOT_EXIST").exists());
}

#[test]
fn changed_source_or_environment_refuses_old_review_before_spawn() {
    for change in ["source", "environment"] {
        let temp = fixture("touch");
        let review = prepare(temp.path());
        if change == "source" {
            fs::write(temp.path().join("src/main.txt"), "changed").unwrap();
        } else {
            let mut settings = load(temp.path(), "check-execution.json");
            settings["environment"]["EXPLICIT_NEW_VALUE"] = json!("changed");
            save(temp.path(), "check-execution.json", &settings);
        }
        let output = command(
            temp.path(),
            &[
                "checks",
                "run",
                "--approve-review",
                review["approval_digest"].as_str().unwrap(),
                "--allow-unsandboxed",
            ],
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(!temp.path().join("MUST_NOT_EXIST").exists());
    }
}

#[test]
fn successful_checks_omit_raw_logs_and_do_not_promote_governance() {
    let temp = fixture("pass");
    let (output, result) = execute(temp.path(), &prepare(temp.path()));
    assert_eq!(
        output.status.code(),
        Some(if supported() { 0 } else { 1 }),
        "{result}"
    );
    assert_eq!(result["checks_passed"], supported());
    assert_eq!(result["checks_executed"], supported());
    assert_eq!(result["completion_verified"], false);
    assert_eq!(result["governance_controls"][0]["status"], "unverified");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC_OUTPUT_MUST_NOT_BE_PUBLISHED")
    );
    assert!(output.stderr.is_empty());
    if supported() {
        let outcome = &result["results"][0]["outcome"];
        assert_eq!(outcome["status"], "passed");
        assert_eq!(outcome["direct_child_reaped"], true);
        assert_eq!(outcome["stdout"]["complete"], true);
    } else {
        assert_eq!(result["results"][0]["outcome"]["status"], "unsupported");
    }
}

#[test]
fn failures_timeouts_and_output_limits_are_distinct() {
    if !supported() {
        return;
    }
    for (mode, expected) in [
        ("fail", "failed"),
        ("timeout", "timeout"),
        ("flood", "output-limit"),
    ] {
        let temp = fixture(mode);
        let mut policy = load(temp.path(), "checks.json");
        if mode == "timeout" {
            policy["checks"][0]["timeout_ms"] = json!(50);
        }
        if mode == "flood" {
            policy["checks"][0]["max_output_bytes"] = json!(128);
        }
        save(temp.path(), "checks.json", &policy);
        let (output, result) = execute(temp.path(), &prepare(temp.path()));
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(result["results"][0]["outcome"]["status"], expected);
        assert_eq!(result["results"][0]["outcome"]["direct_child_reaped"], true);
        assert_eq!(result["checks_passed"], false);
        assert!(result["duration_ms"].as_u64().unwrap() < 61000);
    }
}

#[test]
fn mutation_during_a_check_invalidates_results_and_skips_later_checks() {
    if !supported() {
        return;
    }
    let temp = fixture("mutate");
    let mut policy = load(temp.path(), "checks.json");
    let mut second = policy["checks"][0].clone();
    second["id"] = json!("later");
    policy["checks"].as_array_mut().unwrap().push(second);
    save(temp.path(), "checks.json", &policy);
    let (output, result) = execute(temp.path(), &prepare(temp.path()));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["inputs_current"], false);
    assert_eq!(result["results"][1]["outcome"]["status"], "skipped");
}

#[test]
fn optional_failure_remains_visible_without_failing_required_success() {
    if !supported() {
        return;
    }
    let temp = fixture("pass");
    let mut policy = load(temp.path(), "checks.json");
    let mut second = policy["checks"][0].clone();
    second["id"] = json!("optional");
    second["required"] = json!(false);
    second["argv"][2] = json!("fixture_forced_failure");
    policy["checks"].as_array_mut().unwrap().push(second);
    save(temp.path(), "checks.json", &policy);
    let (output, result) = execute(temp.path(), &prepare(temp.path()));
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(result["results"][1]["outcome"]["status"], "failed");
    assert_eq!(result["completion_verified"], false);
}

#[test]
fn ordinary_descendants_are_stopped_when_the_leader_exits() {
    if !supported() {
        return;
    }
    let temp = fixture("descendant");
    let (output, result) = execute(temp.path(), &prepare(temp.path()));
    assert_eq!(output.status.code(), Some(0), "{result}");
    std::thread::sleep(Duration::from_millis(650));
    assert!(!temp.path().join("LATE_CHILD_MARKER").exists());
}

#[test]
fn settings_reject_ambiguous_json_relative_tools_and_missing_path() {
    let temp = fixture("touch");
    for mutation in [
        "relative",
        "missing-path",
        "relative-path",
        "extra-tool",
        "duplicate",
    ] {
        let original = load(temp.path(), "check-execution.json");
        let mut value = original.clone();
        match mutation {
            "relative" => value["tools"]["test-helper"] = json!("relative-executable"),
            "missing-path" => value["environment"] = json!({}),
            "relative-path" => value["environment"]["PATH"] = json!("."),
            "extra-tool" => value["tools"]["unused"] = json!("/synthetic/unused"),
            _ => {}
        }
        save(temp.path(), "check-execution.json", &value);
        if mutation == "duplicate" {
            let raw = value.to_string().replacen("{", "{\"kind\":\"hidden\",", 1);
            fs::write(temp.path().join(".agentic/check-execution.json"), raw).unwrap();
        }
        assert_eq!(
            command(temp.path(), &["checks", "prepare"]).status.code(),
            Some(2)
        );
        save(temp.path(), "check-execution.json", &original);
    }
    assert!(!temp.path().join("MUST_NOT_EXIST").exists());
}

#[test]
fn duplicate_policy_keys_are_refused_even_for_read_only_planning() {
    let temp = fixture("touch");
    let raw = load(temp.path(), "checks.json").to_string();
    let ambiguous = raw.replacen("{", "{\"kind\":\"hidden\",", 1);
    fs::write(temp.path().join(".agentic/checks.json"), ambiguous).unwrap();
    for operation in ["plan", "prepare"] {
        assert_eq!(
            command(temp.path(), &["checks", operation]).status.code(),
            Some(2)
        );
    }
}

#[test]
fn missing_later_cwd_keeps_first_outcome_and_stops_dispatch() {
    if !supported() {
        return;
    }
    let temp = fixture("remove-cwd");
    fs::create_dir(temp.path().join("later")).unwrap();
    let mut policy = load(temp.path(), "checks.json");
    let mut second = policy["checks"][0].clone();
    second["id"] = json!("later");
    second["cwd"] = json!("later");
    policy["checks"].as_array_mut().unwrap().push(second);
    save(temp.path(), "checks.json", &policy);
    let (output, result) = execute(temp.path(), &prepare(temp.path()));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["results"][0]["outcome"]["status"], "passed");
    assert_eq!(result["results"][1]["outcome"]["spawned"], false);
    assert_eq!(result["results"][1]["reason"], "InputRevalidation");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sigint_and_sigterm_cancel_only_owned_synthetic_processes() {
    use std::time::Instant;
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let temp = fixture("cancel-wait");
        let review = prepare(temp.path());
        let mut child = Command::new(env!("CARGO_BIN_EXE_ah"))
            .args([
                "checks",
                "run",
                "--approve-review",
                review["approval_digest"].as_str().unwrap(),
                "--allow-unsandboxed",
            ])
            .current_dir(temp.path())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !temp.path().join("READY").exists() && Instant::now() < deadline {
            assert!(
                child.try_wait().unwrap().is_none(),
                "supervisor exited before child readiness"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(temp.path().join("READY").exists());
        // SAFETY: this test owns the live, unreaped supervisor child. It never
        // signals arbitrary process IDs or tries to induce PID reuse.
        assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
        let output = child.wait_with_output().unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(result["checks_passed"], false);
        assert_eq!(result["results"][0]["outcome"]["direct_child_reaped"], true);
        assert_eq!(result["results"][0]["outcome"]["status"], "execution-error");
        assert!(!temp.path().join("SURVIVED_CANCELLATION").exists());
    }
}
