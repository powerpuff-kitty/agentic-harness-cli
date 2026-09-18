use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("AGENTS.md"),
        "# Instructions\nRead custom-context/overview.md.\n",
    )
    .unwrap();
    fs::create_dir(temp.path().join("custom-context")).unwrap();
    fs::write(
        temp.path().join("custom-context/overview.md"),
        "Owner-approved project rules.\n",
    )
    .unwrap();
    temp
}

fn call(root: &Path, args: &[&str], code: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ah"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if code == 2 {
        assert!(output.stdout.is_empty());
        let diagnostic: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(diagnostic["kind"], "diagnostic");
        diagnostic
    } else {
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

fn preview(root: &Path, host: &str, profile: &str) -> Value {
    call(
        root,
        &["adapters", "sync", "--host", host, "--profile", profile],
        0,
    )
}

fn apply_args<'a>(host: &'a str, profile: &'a str, digest: &'a str) -> Vec<&'a str> {
    vec![
        "adapters",
        "sync",
        "--host",
        host,
        "--profile",
        profile,
        "--apply",
        "--review",
        digest,
    ]
}

fn install(root: &Path, host: &str, profile: &str) -> Value {
    let plan = preview(root, host, profile);
    call(
        root,
        &apply_args(host, profile, plan["plan_digest"].as_str().unwrap()),
        0,
    )
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, current: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_string_lossy().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result);
    result
}

#[test]
fn preview_is_deterministic_and_does_not_write() {
    let root = fixture();
    let before = snapshot(root.path());
    let plan = preview(root.path(), "claude", "base");
    assert_eq!(plan, preview(root.path(), "claude", "base"));
    assert_eq!(plan["operation"], "preview");
    assert_eq!(plan["status"], "ready");
    assert_eq!(plan["created_files"], json!([]));
    assert_eq!(before, snapshot(root.path()));
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn claude_base_creates_exact_bridge_and_retains_license() {
    let root = fixture();
    let result = install(root.path(), "claude", "base");
    assert_eq!(result["status"], "applied");
    assert_eq!(
        fs::read(root.path().join("CLAUDE.md")).unwrap(),
        b"@AGENTS.md\n"
    );
    assert_eq!(
        fs::read(root.path().join(".agents/adapters/LICENSE")).unwrap(),
        include_bytes!("../upstream/agentic-harness-agents/LICENSE")
    );
    assert!(!root.path().join(".claude").exists());
}

#[test]
fn typed_ui_is_explicit_and_uses_the_pinned_payload() {
    let root = fixture();
    install(root.path(), "claude", "typed-ui");
    let actual = fs::read(root.path().join(".claude/rules/agentic-typed-ui.md")).unwrap();
    assert_eq!(
        actual,
        include_bytes!(
            "../upstream/agentic-harness-agents/adapters/claude/files/.claude/rules/agentic-typed-ui.md"
        )
    );
}

#[test]
fn cursor_base_and_codex_are_no_op_native_routing() {
    for host in ["cursor", "codex"] {
        let root = fixture();
        let before = snapshot(root.path());
        let result = install(root.path(), host, "base");
        assert_eq!(result["entries"], json!([]));
        assert_eq!(result["created_files"], json!([]));
        assert_eq!(before, snapshot(root.path()));
    }
}

#[test]
fn cursor_scoped_rule_does_not_create_claude_or_overrides() {
    let root = fixture();
    install(root.path(), "cursor", "typed-ui");
    let actual = fs::read(root.path().join(".cursor/rules/agentic-typed-ui.mdc")).unwrap();
    assert_eq!(
        actual,
        include_bytes!(
            "../upstream/agentic-harness-agents/adapters/cursor/files/.cursor/rules/agentic-typed-ui.mdc"
        )
    );
    assert!(!root.path().join("CLAUDE.md").exists());
    assert!(!root.path().join("AGENTS.override.md").exists());
}

#[test]
fn repeated_installation_preserves_existing_file_bytes() {
    let root = fixture();
    install(root.path(), "claude", "typed-ui");
    let before = snapshot(root.path());
    let result = install(root.path(), "claude", "typed-ui");
    assert_eq!(result["created_files"], json!([]));
    assert_eq!(result["created_directories"], json!([]));
    assert!(
        result["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["action"] == "unchanged")
    );
    assert_eq!(before, snapshot(root.path()));
}

#[test]
fn conflicting_bridge_prevents_the_whole_batch() {
    let root = fixture();
    fs::write(root.path().join("CLAUDE.md"), "# Keep my instructions\n").unwrap();
    let before = snapshot(root.path());
    let plan = call(
        root.path(),
        &[
            "adapters",
            "sync",
            "--host",
            "claude",
            "--profile",
            "typed-ui",
        ],
        1,
    );
    assert_eq!(plan["status"], "conflict");
    let result = call(
        root.path(),
        &apply_args("claude", "typed-ui", plan["plan_digest"].as_str().unwrap()),
        1,
    );
    assert_eq!(result["created_files"], json!([]));
    assert_eq!(before, snapshot(root.path()));
    assert!(!root.path().join(".claude").exists());
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn license_conflict_does_not_install_an_unattributed_bridge() {
    let root = fixture();
    fs::create_dir_all(root.path().join(".agents/adapters")).unwrap();
    fs::write(
        root.path().join(".agents/adapters/LICENSE"),
        "unrelated notice",
    )
    .unwrap();
    let plan = call(root.path(), &["adapters", "sync", "--host", "claude"], 1);
    call(
        root.path(),
        &apply_args("claude", "base", plan["plan_digest"].as_str().unwrap()),
        1,
    );
    assert!(!root.path().join("CLAUDE.md").exists());
}

#[test]
fn router_change_invalidates_a_review_without_writes() {
    let root = fixture();
    let plan = preview(root.path(), "claude", "base");
    fs::write(root.path().join("AGENTS.md"), "changed routing").unwrap();
    call(
        root.path(),
        &apply_args("claude", "base", plan["plan_digest"].as_str().unwrap()),
        2,
    );
    assert!(!root.path().join("CLAUDE.md").exists());
}

#[test]
fn destination_change_invalidates_a_review_without_overwrite() {
    let root = fixture();
    let plan = preview(root.path(), "claude", "base");
    fs::write(root.path().join("CLAUDE.md"), "owner file").unwrap();
    call(
        root.path(),
        &apply_args("claude", "base", plan["plan_digest"].as_str().unwrap()),
        2,
    );
    assert_eq!(
        fs::read(root.path().join("CLAUDE.md")).unwrap(),
        b"owner file"
    );
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn review_is_bound_to_target_and_profile() {
    let first = fixture();
    let second = fixture();
    let plan = preview(first.path(), "claude", "base");
    for (root, profile) in [(second.path(), "base"), (first.path(), "typed-ui")] {
        call(
            root,
            &apply_args("claude", profile, plan["plan_digest"].as_str().unwrap()),
            2,
        );
        assert!(!root.join("CLAUDE.md").exists());
    }
}

#[test]
fn custom_context_and_unrelated_host_settings_are_preserved() {
    let root = fixture();
    fs::create_dir(root.path().join(".claude")).unwrap();
    fs::write(
        root.path().join(".claude/settings.json"),
        "{\"custom\":true}",
    )
    .unwrap();
    fs::write(root.path().join(".cursorrules"), "user rules").unwrap();
    let before = snapshot(root.path());
    install(root.path(), "claude", "typed-ui");
    for (path, bytes) in before {
        assert_eq!(fs::read(root.path().join(path)).unwrap(), bytes);
    }
    assert!(!root.path().join(".agentic").exists());
}

#[test]
fn invalid_selections_and_options_never_write() {
    let root = fixture();
    let before = snapshot(root.path());
    for args in [
        vec!["adapters", "sync"],
        vec!["adapters", "sync", "--host", "unknown"],
        vec![
            "adapters",
            "sync",
            "--host",
            "codex",
            "--profile",
            "typed-ui",
        ],
        vec![
            "adapters",
            "sync",
            "--host",
            "claude",
            "--profile",
            "unknown",
        ],
        vec!["adapters", "sync", "--host", "claude", "--host", "cursor"],
        vec!["adapters", "sync", "--host", "claude", "--apply"],
        vec!["adapters", "sync", "--host", "claude", "--review", "fake"],
        vec!["adapters", "sync", "--host", "claude", "--force"],
        vec!["adapters", "remove", "--host", "claude"],
    ] {
        call(root.path(), &args, 2);
    }
    assert_eq!(before, snapshot(root.path()));
}

#[test]
fn missing_empty_nontext_and_oversized_router_are_rejected() {
    let root = fixture();
    let router = root.path().join("AGENTS.md");
    fs::remove_file(&router).unwrap();
    call(root.path(), &["adapters", "sync", "--host", "claude"], 2);
    for bytes in [vec![], vec![b' '], vec![0xff], vec![0], vec![b'a'; 65_537]] {
        fs::write(&router, bytes).unwrap();
        call(root.path(), &["adapters", "sync", "--host", "claude"], 2);
    }
    assert!(!root.path().join("CLAUDE.md").exists());
}

#[test]
fn directory_destination_and_parent_file_are_rejected() {
    let root = fixture();
    fs::create_dir(root.path().join("CLAUDE.md")).unwrap();
    call(root.path(), &["adapters", "sync", "--host", "claude"], 2);
    fs::remove_dir(root.path().join("CLAUDE.md")).unwrap();
    fs::write(root.path().join(".claude"), "not a directory").unwrap();
    call(
        root.path(),
        &[
            "adapters",
            "sync",
            "--host",
            "claude",
            "--profile",
            "typed-ui",
        ],
        2,
    );
}

#[test]
fn successful_copy_never_claims_host_delivery_or_enforcement() {
    let root = fixture();
    let result = install(root.path(), "claude", "base");
    assert_eq!(result["host_delivery_verified"], false);
    assert_eq!(result["enforcement_verified"], false);
    assert_eq!(
        result["source"]["repository"],
        "powerpuff-kitty/agentic-harness-agents"
    );
    assert_eq!(
        result["source"]["commit"],
        serde_json::from_str::<Value>(include_str!("../upstream.lock.json")).unwrap()["agents"]["commit"]
    );
}

#[cfg(unix)]
#[test]
fn symlink_router_destination_and_parent_are_rejected() {
    use std::os::unix::fs::symlink;
    let root = fixture();
    let outside = fixture();
    symlink(outside.path(), root.path().join(".claude")).unwrap();
    call(
        root.path(),
        &[
            "adapters",
            "sync",
            "--host",
            "claude",
            "--profile",
            "typed-ui",
        ],
        2,
    );
    fs::remove_file(root.path().join(".claude")).unwrap();
    symlink(outside.path().join("absent"), root.path().join("CLAUDE.md")).unwrap();
    call(root.path(), &["adapters", "sync", "--host", "claude"], 2);
    fs::remove_file(root.path().join("CLAUDE.md")).unwrap();
    fs::remove_file(root.path().join("AGENTS.md")).unwrap();
    symlink(
        outside.path().join("AGENTS.md"),
        root.path().join("AGENTS.md"),
    )
    .unwrap();
    call(root.path(), &["adapters", "sync", "--host", "claude"], 2);
    assert!(!outside.path().join("rules").exists());
}

#[cfg(windows)]
#[test]
fn windows_junction_parent_is_rejected() {
    let root = fixture();
    let outside = fixture();
    let created = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.path().join(".claude"))
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(created.status.success());
    call(
        root.path(),
        &[
            "adapters",
            "sync",
            "--host",
            "claude",
            "--profile",
            "typed-ui",
        ],
        2,
    );
    assert!(!outside.path().join("rules").exists());
    fs::remove_dir(root.path().join(".claude")).unwrap();
}
