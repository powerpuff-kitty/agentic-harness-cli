use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn root(&self) -> &Path {
        self.0.path()
    }
    fn put(&self, path: &str, text: &str) {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn manifest(&self) -> Value {
        json!({
            "format_version": 1,
            "project": {"name": "fixture", "type": "base", "maturity": "prototype"},
            "context": {"product": "PRODUCT.md", "architecture": "ARCHITECTURE.md",
                "security": "SECURITY.md", "decisions": "decisions/"},
            "modules": {"packs": [], "policies": []}, "skills": [], "permissions": {},
            "adapters": {"canonical_router": "../AGENTS.md", "vendor_files_must_be_thin": true}
        })
    }
    fn project(&self) {
        self.put("AGENTS.md", "Preserve project rules.");
        self.put(".agentic/manifest.yaml", &self.manifest().to_string());
        for name in ["PRODUCT", "ARCHITECTURE", "SECURITY"] {
            self.put(&format!(".agentic/{name}.md"), "Accepted project truth.");
        }
    }
    fn plan(&self, task: &str, budget: &str) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_ah"))
            .args([
                "agentic",
                "context",
                ".",
                "--task",
                task,
                "--max-tokens",
                budget,
            ])
            .current_dir(self.root())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

fn included(value: &Value, path: &str) -> bool {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["source"]["path"] == path && item["disposition"] == "included")
}

#[test]
fn canonical_output_is_deterministic_and_selective() {
    let f = Fixture::new();
    f.put("AGENTS.md", "Read relevant context.");
    f.put(
        "src/github_project.rs",
        "fn validateGithubProjectCreation() {}",
    );
    f.put("src/gallery.rs", "fn render_cat_gallery() {}");
    let value = f.plan("validate GitHub project creation", "1000");
    assert_eq!(value, f.plan("validate GitHub project creation", "1000"));
    assert_eq!(value["kind"], "compiled-context-plan");
    assert_eq!(value["budget"]["input"]["limit"], 1000);
    assert_eq!(value["task"]["text"], "validate GitHub project creation");
    assert!(included(&value, "AGENTS.md"));
    assert!(included(&value, "src/github_project.rs"));
    assert!(!included(&value, "src/gallery.rs"));
    assert!(value.get("included").is_none());
    assert!(
        value["items"][0]["source"]["digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
}

#[test]
fn mandatory_sources_survive_budget_overflow() {
    let f = Fixture::new();
    f.project();
    let value = f.plan("validate project", "1");
    assert!(included(&value, ".agentic/SECURITY.md"));
    assert_eq!(value["budget"]["over_budget"], true);
    assert_eq!(value["coverage"]["complete"], true);
}

#[test]
fn missing_core_route_is_not_complete() {
    let f = Fixture::new();
    f.project();
    fs::remove_file(f.root().join(".agentic/SECURITY.md")).unwrap();
    let value = f.plan("validate project", "1000");
    assert_eq!(value["coverage"]["complete"], false);
    assert!(value["coverage"]["required_unavailable"].as_u64().unwrap() > 0);
}

#[test]
fn ignored_required_inputs_are_not_read_or_silently_omitted() {
    for ignored in [
        ".agentic/SECURITY.md",
        ".agentic/manifest.yaml",
        "AGENTS.md",
    ] {
        let f = Fixture::new();
        f.project();
        f.put(".ahignore", ignored);
        let value = f.plan("validate project", "1000");
        assert_eq!(value["coverage"]["complete"], false, "{ignored}");
        assert!(!included(&value, ignored), "{ignored}");
    }
}

#[test]
fn malformed_and_mixed_manifests_are_not_complete() {
    for path in [".agentic/manifest.yaml", "agentic.yaml"] {
        let f = Fixture::new();
        f.project();
        f.put(path, "invalid: [");
        assert_eq!(
            f.plan("validate project", "1000")["coverage"]["complete"],
            false
        );
    }
}

#[test]
fn custom_extensionless_truth_is_normalized_and_required() {
    let f = Fixture::new();
    f.project();
    let mut manifest = f.manifest();
    manifest["context"]["product"] = json!("../accepted-truth");
    f.put(".agentic/manifest.yaml", &manifest.to_string());
    f.put("accepted-truth", "Accepted project constraints.");
    let value = f.plan("validate project", "1");
    assert!(included(&value, "accepted-truth"));
    assert_eq!(value["coverage"]["complete"], true);
}

#[test]
fn declared_missing_policy_prevents_complete_coverage() {
    let f = Fixture::new();
    f.project();
    let mut manifest = f.manifest();
    manifest["modules"]["policies"] = json!(["restricted"]);
    f.put(".agentic/manifest.yaml", &manifest.to_string());
    assert_eq!(
        f.plan("validate project", "1000")["coverage"]["complete"],
        false
    );
    f.put(
        ".agentic/policies/restricted.md",
        "Explicitly required policy.",
    );
    let value = f.plan("validate project", "1");
    assert_eq!(value["coverage"]["complete"], true);
    assert!(included(&value, ".agentic/policies/restricted.md"));
}

#[test]
fn sensitive_paths_never_appear_as_context_items() {
    let f = Fixture::new();
    f.put("AGENTS.md", "Project rules.");
    f.put(".env.production.json", "validate project SECRET-SENTINEL");
    let value = f.plan("validate project", "1000");
    assert!(!included(&value, ".env.production.json"));
    assert_eq!(value["coverage"]["unsupported_files"], 1);
    assert!(!value.to_string().contains("SECRET-SENTINEL"));
}

#[test]
fn budget_ranking_prefers_information_density_over_large_path_hits() {
    let f = Fixture::new();
    f.put("AGENTS.md", "Rules");
    f.put("src/helper.rs", "validate project");
    f.put(
        "src/validate-project.rs",
        &format!("validate project\n{}", "x".repeat(180)),
    );
    let value = f.plan("validate project", "54");
    assert!(included(&value, "src/helper.rs"));
    assert!(!included(&value, "src/validate-project.rs"));
}

#[test]
fn invalid_task_and_budget_options_fail() {
    let f = Fixture::new();
    for args in [
        vec!["agentic", "context", ".", "--max-tokens", "100"],
        vec!["agentic", "context", ".", "--task", "   "],
        vec!["agentic", "context", ".", "--task", "line\nbreak"],
        vec![
            "agentic",
            "context",
            ".",
            "--task",
            "project",
            "--max-tokens",
            "0",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ah"))
            .args(args)
            .current_dir(f.root())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[cfg(unix)]
#[test]
fn symlinked_core_route_is_unavailable_even_with_an_internal_target() {
    let f = Fixture::new();
    f.project();
    f.put("actual.md", "Project rules.");
    fs::remove_file(f.root().join(".agentic/SECURITY.md")).unwrap();
    std::os::unix::fs::symlink("../actual.md", f.root().join(".agentic/SECURITY.md")).unwrap();
    assert_eq!(
        f.plan("validate project", "1000")["coverage"]["complete"],
        false
    );
}
