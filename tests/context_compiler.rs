use serde_json::Value;
use std::{fs, process::Command};

fn put(root: &std::path::Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn task_context_command_is_budgeted_and_explainable() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "AGENTS.md", "Load only relevant context and preserve required policy.");
    put(
        dir.path(),
        "src/github_project.rs",
        "pub fn validate_github_project_creation() -> bool { true }",
    );
    put(dir.path(), "src/gallery.rs", "pub fn render_cat_gallery() {}");

    let output = Command::new(env!("CARGO_BIN_EXE_ah"))
        .args([
            "agentic",
            "context",
            ".",
            "--task",
            "validate GitHub project creation",
            "--max-tokens",
            "1000",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["kind"], "compiled-context-plan");
    assert_eq!(value["budget"]["max_tokens"], 1000);
    assert_eq!(value["included"][0]["path"], "AGENTS.md");
    assert!(
        value["included"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["path"] == "src/github_project.rs")
    );
    assert!(
        value["deferred"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["path"] == "src/gallery.rs")
    );
    assert!(
        value["budget"]["estimation"]
            .as_str()
            .unwrap()
            .contains("not provider billing")
    );
}

#[test]
fn max_tokens_requires_a_task() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "AGENTS.md", "router");
    let output = Command::new(env!("CARGO_BIN_EXE_ah"))
        .args(["agentic", "context", ".", "--max-tokens", "100"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--max-tokens requires --task")
    );
}
