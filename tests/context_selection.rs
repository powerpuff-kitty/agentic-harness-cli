use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn root(&self) -> &Path {
        self.0.path()
    }
    fn run(&self, args: &[&str], expected: i32) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_ah"))
            .args(args)
            .current_dir(self.root())
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(expected),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(if expected == 2 {
            &out.stderr
        } else {
            &out.stdout
        })
        .unwrap()
    }
    fn manifest(&self, name: &str) -> Value {
        serde_yaml_ng::from_str(
            &fs::read_to_string(self.root().join(name).join(".agentic/manifest.yaml")).unwrap(),
        )
        .unwrap()
    }
    fn write_manifest(&self, name: &str, value: &Value) {
        fs::write(
            self.root().join(name).join(".agentic/manifest.yaml"),
            serde_yaml_ng::to_string(value).unwrap(),
        )
        .unwrap();
    }
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}

#[test]
fn minimal_and_full_variants_validate_and_minimal_has_no_empty_scaffolding() {
    let f = Fixture::new();
    for variant in [
        "base",
        "web-app",
        "backend-api",
        "saas",
        "monorepo",
        "library-sdk",
    ] {
        let report = f.run(
            &[
                "init",
                variant,
                "--boilerplate",
                variant,
                "--context-profile",
                "minimal",
            ],
            0,
        );
        assert_eq!(report["context_profile"], "minimal");
        assert_eq!(f.run(&["validate", variant], 0)["valid"], true);
        let root = f.root().join(variant);
        let manifest = f.manifest(variant);
        for route in ["reference", "docs", "plans", "tasks", "evals"] {
            assert!(manifest["context"][route].is_null());
        }
        for absent in [
            ".agentic/REFERENCE.md",
            ".agentic/docs",
            ".agentic/plans",
            ".agentic/tasks",
            ".agentic/evals",
            ".github/copilot-instructions.md",
            "apps",
            "packages",
        ] {
            assert!(!root.join(absent).exists(), "{variant}: {absent}");
        }
        assert!(root.join(".agentic/THIRD_PARTY_NOTICES.md").is_file());
        let map = fs::read_to_string(root.join(".agentic/README.md")).unwrap();
        assert!(!map.contains("{{"));
        assert!(map.contains("Structure installed, project configured"));
        for line in map.lines().filter(|line| line.starts_with("| `")) {
            let relative = line.split('`').nth(1).unwrap();
            assert!(
                root.join(".agentic").join(relative).exists(),
                "unresolved map path: {relative}"
            );
        }
        let full = format!("{variant}-full");
        f.run(
            &[
                "init",
                &full,
                "--boilerplate",
                variant,
                "--context-profile",
                "full",
            ],
            0,
        );
        assert_eq!(f.run(&["validate", &full], 0)["valid"], true);
        assert!(snapshot(&root).len() < snapshot(&f.root().join(&full)).len());
        assert!(f.root().join(full).join(".agentic/REFERENCE.md").is_file());
    }
}

#[test]
fn modules_install_completely_and_design_packs_add_context_to_base() {
    let f = Fixture::new();
    f.run(
        &[
            "init",
            "minimal",
            "--context-profile",
            "minimal",
            "--pack",
            "web-app",
            "--skill",
            "codebase-audit",
        ],
        0,
    );
    f.run(
        &[
            "init",
            "full",
            "--pack",
            "web-app",
            "--skill",
            "codebase-audit",
        ],
        0,
    );
    let root = f.root().join("minimal");
    assert_eq!(f.manifest("minimal")["context"]["design"], "DESIGN.md");
    assert!(root.join(".agentic/DESIGN.md").is_file());
    for path in [".agentic/packs", ".agentic/policies", ".agents/skills"] {
        assert_eq!(
            snapshot(&root.join(path)),
            snapshot(&f.root().join("full").join(path))
        );
    }
    assert_eq!(f.run(&["validate", "minimal"], 0)["valid"], true);
}

#[test]
fn omitted_selection_and_variant_are_inherited_and_repeated_upgrade_is_byte_stable() {
    let f = Fixture::new();
    f.run(
        &[
            "init",
            "repo",
            "--boilerplate",
            "web-app",
            "--context-profile",
            "minimal",
        ],
        0,
    );
    let root = f.root().join("repo");
    let before = snapshot(&root);
    for args in [
        vec!["upgrade", "repo"],
        vec!["upgrade", "repo", "--context-profile", "minimal"],
        vec!["init", "repo", "--allow-existing"],
    ] {
        let report = f.run(&args, 0);
        assert_eq!(report["context_profile"], "minimal");
        assert_eq!(report["boilerplate"], "web-app");
        assert_eq!(report["created"], json!([]));
        assert_eq!(snapshot(&root), before);
    }
}

#[test]
fn both_mode_transitions_preserve_authored_files_routes_and_original_checksums() {
    let f = Fixture::new();
    f.run(&["init", "repo", "--boilerplate", "web-app"], 0);
    let root = f.root().join("repo");
    fs::create_dir_all(root.join(".agentic/custom")).unwrap();
    fs::rename(
        root.join(".agentic/PRODUCT.md"),
        root.join(".agentic/custom/product.md"),
    )
    .unwrap();
    fs::write(
        root.join(".agentic/custom/product.md"),
        "accepted project truth",
    )
    .unwrap();
    fs::write(
        root.join(".agentic/REFERENCE.md"),
        "authored optional context",
    )
    .unwrap();
    fs::write(
        root.join(".agentic/packs/web-app/PACK.md"),
        "custom pack guidance",
    )
    .unwrap();
    fs::write(root.join(".agentic/README.md"), "authored context map").unwrap();
    let mut manifest = f.manifest("repo");
    manifest["context"]["product"] = json!("custom/product.md");
    manifest["context"]["design"] = Value::Null;
    f.write_manifest("repo", &manifest);
    let before = snapshot(&root);
    let original_lock: Value = serde_json::from_slice(&before[".agentic/lock.json"]).unwrap();
    for mode in ["minimal", "full"] {
        let report = f.run(
            &[
                "upgrade",
                "repo",
                "--context-profile",
                mode,
                "--pack",
                "web-app",
            ],
            0,
        );
        assert_eq!(report["removed"], json!([]));
        assert!(
            report["conflicts"]
                .as_array()
                .unwrap()
                .contains(&json!(".agentic/packs/web-app/PACK.md"))
        );
        assert!(
            report["conflicts"]
                .as_array()
                .unwrap()
                .contains(&json!(".agentic/README.md"))
        );
        let after = snapshot(&root);
        for (path, bytes) in &before {
            if ![".agentic/manifest.yaml", ".agentic/lock.json"].contains(&path.as_str()) {
                assert_eq!(after.get(path), Some(bytes), "{mode}: {path}");
            }
        }
        assert_eq!(f.manifest("repo")["context"], manifest["context"]);
        let lock: Value = serde_json::from_slice(&after[".agentic/lock.json"]).unwrap();
        assert_eq!(
            lock["checksums"][".agentic/packs/web-app/PACK.md"],
            original_lock["checksums"][".agentic/packs/web-app/PACK.md"]
        );
        assert_eq!(f.run(&["validate", "repo"], 0)["valid"], true);
        assert!(!root.join(".agentic/PRODUCT.md").exists());
    }
}

#[test]
fn minimal_expansion_adds_full_files_but_retains_null_routes() {
    let f = Fixture::new();
    f.run(
        &[
            "init",
            "repo",
            "--boilerplate",
            "web-app",
            "--context-profile",
            "minimal",
        ],
        0,
    );
    let original = f.manifest("repo");
    let report = f.run(&["upgrade", "repo", "--context-profile", "full"], 0);
    assert_eq!(report["boilerplate"], "web-app");
    assert!(f.root().join("repo/.agentic/docs/README.md").is_file());
    assert_eq!(f.manifest("repo")["context"], original["context"]);
    assert_eq!(f.run(&["validate", "repo"], 0)["valid"], true);
    assert_eq!(f.run(&["upgrade", "repo"], 0)["context_profile"], "full");
}

#[test]
fn invalid_selection_and_legacy_layout_fail_without_writes() {
    let f = Fixture::new();
    for invalid in ["small", "", "../minimal"] {
        f.run(&["init", "absent", "--context-profile", invalid], 2);
        assert!(!f.root().join("absent").exists());
    }
    f.run(&["init", "repo"], 0);
    let original = f.manifest("repo");
    for invalid in [
        json!(null),
        json!({}),
        json!({"context_profile":"small"}),
        json!({"context_profile":"minimal", "prune":true}),
    ] {
        let mut manifest = original.clone();
        manifest["composition"] = invalid;
        f.write_manifest("repo", &manifest);
        let before = snapshot(&f.root().join("repo"));
        f.run(&["upgrade", "repo", "--context-profile", "minimal"], 2);
        assert_eq!(snapshot(&f.root().join("repo")), before);
        assert_eq!(f.run(&["validate", "repo"], 1)["valid"], false);
    }
    fs::create_dir(f.root().join("legacy")).unwrap();
    fs::write(f.root().join("legacy/agentic.yaml"), "version: 1\n").unwrap();
    let before = snapshot(&f.root().join("legacy"));
    f.run(&["upgrade", "legacy", "--context-profile", "minimal"], 2);
    assert_eq!(snapshot(&f.root().join("legacy")), before);
}

#[test]
fn old_manifest_defaults_to_full_and_copied_binary_works_without_runtime_sources() {
    let f = Fixture::new();
    f.run(&["init", "old"], 0);
    let mut manifest = f.manifest("old");
    manifest.as_object_mut().unwrap().remove("composition");
    f.write_manifest("old", &manifest);
    assert_eq!(f.run(&["upgrade", "old"], 0)["context_profile"], "full");
    // Hand-authored projects without a lock can use a non-catalog project type.
    // Keep the old base fallback rather than making the additive option a breaking change.
    fs::remove_file(f.root().join("old/.agentic/lock.json")).unwrap();
    manifest["project"]["type"] = json!("custom-tooling");
    f.write_manifest("old", &manifest);
    assert_eq!(f.run(&["upgrade", "old"], 0)["boilerplate"], "base");
    assert_eq!(f.manifest("old")["project"]["type"], "custom-tooling");
    let binary = f.root().join(if cfg!(windows) {
        "copied.exe"
    } else {
        "copied"
    });
    fs::copy(env!("CARGO_BIN_EXE_ah"), &binary).unwrap();
    let out = Command::new(&binary)
        .args(["init", "fresh", "--context-profile", "minimal"])
        .current_dir(f.root())
        .env("PATH", "")
        .env_remove("AH_REGISTRY")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        f.manifest("fresh")["composition"]["context_profile"],
        "minimal"
    );
    assert_eq!(f.run(&["validate", "fresh"], 0)["valid"], true);
}
