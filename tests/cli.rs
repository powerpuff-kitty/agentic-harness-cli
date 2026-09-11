use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn path(&self) -> &Path {
        self.0.path()
    }
    fn put(&self, path: &str, text: impl AsRef<[u8]>) -> PathBuf {
        let p = self.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, text).unwrap();
        p
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ah"))
            .args(args)
            .current_dir(self.path())
            .env_remove("AH_REGISTRY")
            .output()
            .unwrap()
    }
    fn json(&self, args: &[&str], exit: i32) -> Value {
        let output = self.run(args);
        assert_eq!(
            output.status.code(),
            Some(exit),
            "args={args:?}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
#[test]
fn every_family_is_available_in_the_installed_binary() {
    let f = Fixture::new();
    let copy = f.path().join(if cfg!(windows) { "ah.exe" } else { "ah" });
    fs::copy(env!("CARGO_BIN_EXE_ah"), &copy).unwrap();
    for args in [
        vec!["--help"],
        vec!["architecture", "--help"],
        vec!["design", "--help"],
        vec!["agentic", "--help"],
        vec!["--version"],
    ] {
        let o = Command::new(&copy)
            .args(args)
            .current_dir(f.path())
            .env_remove("AH_REGISTRY")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let models = f.json(&["agentic", "models", "."], 0);
    assert!(models["models"].as_array().unwrap().len() >= 2);
    assert!(models["models"][0]["current_structure_compatibility"].is_null());
}
#[test]
fn validates_all_boilerplates_with_current_context() {
    let f = Fixture::new();
    for boilerplate in [
        "base",
        "web-app",
        "backend-api",
        "saas",
        "monorepo",
        "library-sdk",
    ] {
        f.json(
            &[
                "init",
                boilerplate,
                "--boilerplate",
                boilerplate,
                "--name",
                "name: with YAML punctuation",
                "--maturity",
                "production",
            ],
            0,
        );
        let root = f.path().join(boilerplate);
        assert!(root.join(".agentic/manifest.yaml").is_file());
        assert!(!root.join("agentic.yaml").exists());
        assert_eq!(f.json(&["validate", boilerplate], 0)["valid"], true);
        assert_eq!(f.json(&["harness-audit", boilerplate], 0)["score"], 100);
        let output = f.run(&["audit", boilerplate]);
        assert!(matches!(output.status.code(), Some(0 | 1)));
        let audit: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(audit["target_maturity"], "production");
        assert!(audit["overall"].is_null());
    }
}
#[test]
fn empty_malformed_and_missing_context_never_validate() {
    let f = Fixture::new();
    assert_eq!(f.json(&["validate", "."], 1)["valid"], false);
    f.put(".agentic/manifest.yaml", "invalid: [\n");
    assert_eq!(f.json(&["validate", "."], 1)["valid"], false);
    fs::remove_file(f.path().join(".agentic/manifest.yaml")).unwrap();
    f.json(&["init", "repo"], 0);
    fs::remove_file(f.path().join("repo/.agentic/PRODUCT.md")).unwrap();
    assert_eq!(f.json(&["validate", "repo"], 1)["valid"], false);
}
#[test]
fn invalid_selection_leaves_no_target() {
    let f = Fixture::new();
    let o = f.run(&["init", "repo", "--pack", "no-such-pack"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(!f.path().join("repo").exists());
}
#[test]
fn upgrade_preserves_custom_module_and_project_content() {
    let f = Fixture::new();
    f.json(&["init", "repo", "--pack", "web-app"], 0);
    f.put("repo/.agentic/packs/web-app/LOCAL.md", "custom");
    f.put("repo/.agentic/PRODUCT.md", "accepted product truth");
    let r = f.json(&["upgrade", "repo", "--pack", "web-app"], 0);
    assert_eq!(
        fs::read_to_string(f.path().join("repo/.agentic/PRODUCT.md")).unwrap(),
        "accepted product truth"
    );
    assert_eq!(
        fs::read_to_string(f.path().join("repo/.agentic/packs/web-app/LOCAL.md")).unwrap(),
        "custom"
    );
    assert!(!f.path().join("repo/PRODUCT.md").exists());
    assert!(
        r["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == ".agentic/PRODUCT.md")
    );
}
fn audit(overall: Value) -> Value {
    json!({"format_version":2,"kind":"codebase-audit","overall":overall,"scores":{"security":20},"findings":[],"checks":{"performed":[],"not_checked":[]},"architecture":{"compliance":{"deterministic_errors":0,"passed":true}}})
}
#[test]
fn gates_validate_artifacts_and_finite_thresholds() {
    let f = Fixture::new();
    f.put("empty.json", "{}");
    f.put("bad.json", audit(json!(30)).to_string());
    for args in [
        vec!["gate", "empty.json"],
        vec!["compare", "empty.json", "empty.json"],
        vec!["gate", "bad.json", "--min-overall", "NaN"],
        vec!["gate", "bad.json", "--min-score", "security=NaN"],
        vec!["gate", "bad.json", "--min-overall", "101"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2), "{args:?}");
    }
    assert_eq!(
        f.json(&["gate", "bad.json", "--min-overall", "80"], 1)["passed"],
        false
    );
    assert_eq!(
        f.json(
            &[
                "gate",
                "bad.json",
                "--min-overall",
                "20",
                "--fail-on-architecture-error"
            ],
            0
        )["passed"],
        true
    );
    f.put("unknown.json", audit(Value::Null).to_string());
    assert_eq!(
        f.json(&["gate", "unknown.json", "--min-overall", "0"], 1)["passed"],
        false
    );
}
#[test]
fn arguments_and_model_ids_are_strict() {
    let f = Fixture::new();
    f.put("file", "{}");
    for args in [
        vec!["audit", ".", "--typo"],
        vec!["audit", "file"],
        vec!["agentic", "migrate", "."],
        vec!["agentic", "improve", ".", "--apply"],
        vec!["agentic", "compare", "unknown-a", "unknown-b"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2), "{args:?}");
    }
    for cmd in ["audit", "context", "skills", "models", "improve"] {
        assert_eq!(
            f.run(&["agentic", cmd, "nonexistent"]).status.code(),
            Some(2)
        );
    }
}
#[test]
fn runtime_type_and_dynamic_imports_are_distinct() {
    let f = Fixture::new();
    f.put("a.ts", "import type { B } from './b'; export type A = B;");
    f.put(
        "b.ts",
        "import type { A } from './a'; export type B = { a: A };",
    );
    let report = f.json(&["architecture", "analyze", "."], 0);
    assert_eq!(report["compliance"]["deterministic_errors"], 0);
    assert_eq!(report["graph"]["edges"][0]["kind"], "type");
    f.put(
        "a.ts",
        "import { b } from './b'; export const a = () => b();",
    );
    f.put(
        "b.ts",
        "import { a } from './a'; export const b = () => a();",
    );
    assert_eq!(
        f.json(&["architecture", "analyze", "."], 0)["compliance"]["deterministic_errors"],
        1
    );
    f.put("a.ts", "export const a = () => import('./b');");
    assert_eq!(
        f.json(&["architecture", "analyze", "."], 0)["compliance"]["deterministic_errors"],
        0
    );
}
#[test]
fn parser_ignores_strings_comments_and_resolves_ts_and_shared_aliases() {
    let f = Fixture::new();
    f.put("a.ts","import { b } from './b.js';\nimport { x } from '#shared/x';\nconst text = `import { bad } from './not-real'`;\n// import './also-not-real';\n");
    f.put("b.ts", "export const b = 1;");
    f.put("shared/x.ts", "export const x = 1;");
    let report = f.json(&["architecture", "analyze", "."], 0);
    assert_eq!(report["graph"]["edges"].as_array().unwrap().len(), 2);
    assert!(
        report["graph"]["unresolved_local_imports"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn expired_exceptions_never_waive_a_runtime_cycle() {
    let f = Fixture::new();
    f.put(
        "a.ts",
        "import { b } from './b'; export const a = () => b();",
    );
    f.put(
        "b.ts",
        "import { a } from './a'; export const b = () => a();",
    );
    f.json(&["architecture", "enforce", ".", "--write"], 0);
    let p = f.path().join(".agentic/architecture.json");
    let mut c: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    c["exceptions"] = json!([{"rule_id":"dependency.no-import-cycles","path":"a.ts","rationale":"temporary compatibility allowance","expires":"2026-09-10"}]);
    fs::write(&p, c.to_string()).unwrap();
    let r = f.json(
        &["architecture", "analyze", ".", "--as-of", "2026-09-11"],
        0,
    );
    assert_eq!(r["compliance"]["deterministic_errors"], 1);
    assert_eq!(r["expired_exceptions"].as_array().unwrap().len(), 1);
    assert_eq!(
        f.json(
            &["architecture", "analyze", ".", "--as-of", "2026-09-10"],
            0
        )["compliance"]["deterministic_errors"],
        0
    );
    assert_eq!(
        f.run(&["architecture", "analyze", ".", "--as-of", "2026-02-30"])
            .status
            .code(),
        Some(2)
    );
}
#[test]
fn design_controls_are_template_elements_not_types_or_test_strings() {
    let f = Fixture::new();
    f.put("src/App.vue","<script setup lang=\"ts\">const value = computed<ButtonRouteTarget>(() => null);</script><template><div /></template>");
    f.put(
        "packages/design-system/components/Modal.vue",
        "<template><div role=\"dialog\" /></template>",
    );
    f.put(
        "packages/design-system/theme.css",
        ":root { --ds-primary: #123456; }",
    );
    f.put(
        "tests/page.test.ts",
        "const fake = '<button>fake</button>';",
    );
    f.put(
        ".agentic/design-system.json",
        json!({"required_components":["dialog","tokens"]}).to_string(),
    );
    let r = f.json(&["audit", "."], 1);
    assert!(
        r["design_system"]["missing_components"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        r["design_system"]["violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.put(
        "src/App.vue",
        "<template><button>Actual native control</button></template>",
    );
    let r = f.json(&["audit", "."], 1);
    assert!(
        r["design_system"]["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["type"] == "raw-control-bypass")
    );
}
#[test]
fn workspace_detection_cites_nested_manifests() {
    let f = Fixture::new();
    f.put("package.json", r#"{"workspaces":["packages/*"]}"#);
    f.put(
        "packages/web/package.json",
        r#"{"dependencies":{"vue":"3","pinia":"3","vue-router":"4","vite":"7"}}"#,
    );
    f.put("packages/web/src/App.vue", "<template><div /></template>");
    let r = f.json(&["architecture", "detect", "."], 0);
    assert!(!r["tooling"].as_array().unwrap().is_empty());
    assert!(
        r["ecosystem"]
            .to_string()
            .contains("packages/web/package.json")
    );
}
#[test]
fn ignores_generated_files_and_respects_repository_ignores() {
    let f = Fixture::new();
    f.put(".gitignore", "ignored/\n");
    f.put("src/a.ts", "export const a = 1;");
    f.put("ignored/a.ts", "import './missing';");
    f.put("dist-ssr/a.js", "import './missing';");
    f.put(".wrangler/tmp/a.js", "import './missing';");
    let r = f.json(&["architecture", "analyze", "."], 0);
    assert_eq!(r["graph"]["source_files"], 1);
    assert!(
        r["graph"]["unresolved_local_imports"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[cfg(unix)]
#[test]
fn never_follows_external_symlinks_or_writes_through_them() {
    let f = Fixture::new();
    let outside = Fixture::new();
    outside.put("external.ts", "export const outside = true;");
    outside.put("key.txt", format!("{}{}", "AKIA", "ABCDEFGHIJKLMNOP"));
    std::os::unix::fs::symlink(outside.path(), f.path().join("external")).unwrap();
    let r = f.json(&["architecture", "analyze", "."], 0);
    assert_eq!(r["graph"]["source_files"], 0);
    assert_eq!(f.json(&["security-scan", "."], 0)["passed"], true);
    fs::create_dir(f.path().join("project")).unwrap();
    std::os::unix::fs::symlink(outside.path(), f.path().join("project/.agentic")).unwrap();
    assert_eq!(
        f.run(&["init", "project", "--allow-existing"])
            .status
            .code(),
        Some(2)
    );
    assert!(!outside.path().join("PRODUCT.md").exists());
}
#[test]
fn secret_markers_are_bounded_and_redacted() {
    let f = Fixture::new();
    f.put("benign.txt", "TAKIAKI and -----BEGIN CERTIFICATE-----");
    assert_eq!(f.json(&["security-scan", "."], 0)["passed"], true);
    let key = format!("{}{}", "AKIA", "ABCDEFGHIJKLMNOP");
    f.put("key.txt", &key);
    let output = f.run(&["security-scan", "."]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&key));
}
#[test]
fn design_closed_loop_preserves_candidates_and_compiles_only_approved_input() {
    let f = Fixture::new();
    f.put("src/main.css", ".x { color: #123456; padding: 8px; }");
    f.json(&["design", "analyze", ".", "--output", "analysis.json"], 0);
    f.json(
        &[
            "design",
            "preserve",
            "--analysis",
            "analysis.json",
            "--output",
            "candidate.json",
        ],
        0,
    );
    f.json(&["design", "diff", "analysis.json", "analysis.json"], 0);
    f.put(
        "task.json",
        include_str!("fixtures/design-task.settings.json"),
    );
    f.put(
        "approved.json",
        include_str!("fixtures/design-genome.approved.json"),
    );
    assert_eq!(
        f.run(&[
            "design",
            "prompt",
            "--genome",
            "candidate.json",
            "--task",
            "task.json"
        ])
        .status
        .code(),
        Some(2)
    );
    let p = f.run(&[
        "design",
        "prompt",
        "--genome",
        "approved.json",
        "--task",
        "task.json",
    ]);
    assert!(p.status.success());
    let brief = String::from_utf8_lossy(&p.stdout);
    assert!(brief.contains("settings.compact"));
    assert!(!brief.contains("marketing.display"));
}

#[test]
fn custom_context_paths_are_preserved_without_parallel_truth() {
    let f = Fixture::new();
    f.json(&["init", "repo"], 0);
    let path = f.path().join("repo/.agentic/manifest.yaml");
    let mut manifest: Value = serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest["context"]["product"] = json!("../docs/current-product.md");
    fs::write(&path, serde_yaml_ng::to_string(&manifest).unwrap()).unwrap();
    fs::create_dir_all(f.path().join("repo/docs")).unwrap();
    fs::rename(
        f.path().join("repo/.agentic/PRODUCT.md"),
        f.path().join("repo/docs/current-product.md"),
    )
    .unwrap();
    f.json(&["upgrade", "repo"], 0);
    assert!(!f.path().join("repo/.agentic/PRODUCT.md").exists());
    assert!(!f.path().join("repo/PRODUCT.md").exists());
    f.json(&["validate", "repo"], 0);
}
#[test]
fn workspace_exports_and_json_tsconfig_paths_resolve() {
    let f = Fixture::new();
    f.put("package.json", r#"{"workspaces":["packages/*"]}"#);
    f.put(
        "packages/core/package.json",
        r#"{"name":"@test/core","exports":{".":"./src/index.ts"}}"#,
    );
    f.put("packages/core/src/index.ts", "export const value=1;");
    f.put("packages/app/package.json", "{}");
    f.put(
        "packages/app/tsconfig.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@local/*":["src/*"]}}}"#,
    );
    f.put("packages/app/src/local.ts", "export const local=1;");
    f.put(
        "packages/app/src/main.ts",
        "import { value } from '@test/core';\nimport { local } from '@local/local';",
    );
    let r = f.json(&["architecture", "analyze", "."], 0);
    assert_eq!(r["graph"]["local_edges"], 2);
    assert_eq!(r["graph"]["external_imports"], 0);
    assert!(
        r["graph"]["unresolved_local_imports"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn incomplete_import_coverage_cannot_pass_an_architecture_gate() {
    let f = Fixture::new();
    f.put("src/a.ts", "import { missing } from './missing';");
    let o = f.run(&["audit", "."]);
    let r: Value = serde_json::from_slice(&o.stdout).unwrap();
    f.put("audit.json", r.to_string());
    assert!(r["scores"]["architecture"].is_null());
    assert_eq!(
        f.json(&["gate", "audit.json", "--max-architecture-errors", "0"], 1)["passed"],
        false
    );
}

#[test]
fn upgrade_merges_declared_modules_without_replacing_project_metadata() {
    let f = Fixture::new();
    f.json(&["init", "repo", "--name", "accepted-name"], 0);
    let path = f.path().join("repo/.agentic/manifest.yaml");
    let mut value: Value = serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    value["custom_owner"] = json!("retain this");
    fs::write(&path, serde_yaml_ng::to_string(&value).unwrap()).unwrap();
    f.json(&["upgrade", "repo", "--pack", "web-app"], 0);
    let value: Value = serde_yaml_ng::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(value["project"]["name"], "accepted-name");
    assert_eq!(value["custom_owner"], "retain this");
    assert!(
        value["modules"]["packs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "web-app")
    );
}

#[test]
fn legacy_mixed_and_wrong_typed_manifests_are_handled_explicitly() {
    let f = Fixture::new();
    f.put("agentic.yaml", "version: 1\nproject:\n  name: Legacy\n  type: base\n  maturity: startup\nsources: {}\nagent: {}\n");
    for path in [
        "AGENTS.md",
        "PRODUCT.md",
        "ARCHITECTURE.md",
        "SECURITY.md",
        "DESIGN.md",
        "REFERENCE.md",
    ] {
        f.put(path, "legacy truth");
    }
    f.json(&["validate", "."], 0);
    assert_eq!(f.run(&["upgrade", "."]).status.code(), Some(2));
    assert!(!f.path().join(".agentic/manifest.yaml").exists());
    f.put(".agentic/manifest.yaml", "format_version: 1\n");
    f.json(&["validate", "."], 1);
    fs::remove_file(f.path().join("agentic.yaml")).unwrap();
    f.json(&["validate", "."], 1);
    f.json(&["init", "modern"], 0);
    let path = f.path().join("modern/.agentic/manifest.yaml");
    let mut manifest: Value = serde_yaml_ng::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest["modules"]["packs"] = json!("web-app");
    fs::write(path, serde_yaml_ng::to_string(&manifest).unwrap()).unwrap();
    f.json(&["validate", "modern"], 1);
}

#[test]
fn configured_aliases_override_defaults_and_missing_alias_targets_are_incomplete() {
    let f = Fixture::new();
    f.put("package.json", "{}");
    f.put(
        "tsconfig.json",
        r#"{"compilerOptions":{"paths":{"@/*":["app/*"],"@missing/*":["absent/*"]}}}"#,
    );
    f.put("app/other.ts", "export const value = 1;");
    f.put(
        "app/main.ts",
        "import {value} from '@/other'; import '@missing/thing';",
    );
    let report = f.json(&["architecture", "analyze", "."], 0);
    assert_eq!(report["graph"]["local_edges"], 1);
    assert_eq!(report["compliance"]["complete"], false);
}

#[test]
fn fractional_scores_and_incomplete_audits_do_not_silently_pass() {
    let f = Fixture::new();
    let mut a = audit(json!(75.5));
    a["scores"]["security"] = json!(20.5);
    f.put("a.json", a.to_string());
    a["scores"]["security"] = json!(21.0);
    f.put("b.json", a.to_string());
    assert_eq!(
        f.json(&["compare", "a.json", "b.json"], 0)["scores"]["security"]["delta"],
        0.5
    );
    assert_eq!(
        f.run(&["gate", "a.json", "--min-score", "invented=10"])
            .status
            .code(),
        Some(2)
    );
    a["architecture"]["compliance"]["complete"] = json!(false);
    f.put("partial.json", a.to_string());
    f.json(&["gate", "partial.json"], 1);
}

#[test]
fn svelte_markup_and_explicit_design_configuration_are_respected() {
    let f = Fixture::new();
    f.put("components/ui/Button.svelte", "<button><slot /></button>");
    f.put(
        "App.svelte",
        "<script>const text = '<input />';</script><button>Save</button>",
    );
    let report = f.json(&["audit", "."], 1);
    assert_eq!(
        report["design_system"]["violations"][0]["evidence"][0]["count"],
        1
    );
    f.put(".agentic/design-system.json", r#"{"aliases": []}"#);
    let report = f.json(&["audit", "."], 1);
    assert_eq!(report["design_system"]["status"], "invalid-configuration");
}

#[cfg(unix)]
#[test]
fn explicit_generated_artifacts_reject_external_symlink_destinations() {
    let f = Fixture::new();
    let outside = Fixture::new();
    let file = outside.put("keep.json", "retain original");
    f.put(".agentic/README.md", "context");
    std::os::unix::fs::symlink(&file, f.path().join(".agentic/architecture.json")).unwrap();
    assert_eq!(
        f.run(&["architecture", "enforce", ".", "--write"])
            .status
            .code(),
        Some(2)
    );
    std::os::unix::fs::symlink(&file, f.path().join("DESIGN_SYSTEM_COMPONENTS.md")).unwrap();
    assert_eq!(
        f.run(&["design-system-components", ".", "--write"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(file).unwrap(), "retain original");
}

#[test]
fn wildcard_workspace_exports_and_non_code_resources_are_classified() {
    let f = Fixture::new();
    f.put(
        "packages/core/package.json",
        r#"{"name":"@test/core","exports":{"./utils/*":"./src/utils/*.ts"}}"#,
    );
    f.put(
        "packages/core/src/utils/value.ts",
        "export const value = 1;",
    );
    f.put("packages/app/src/style.css", "body { color: red; }");
    f.put("packages/app/src/data.json", "{}");
    f.put("packages/app/src/App.svelte", "<script>import { value } from '@test/core/utils/value'; import './style.css'; import data from './data.json';</script><button>Save</button>");
    let report = f.json(&["architecture", "analyze", "."], 0);
    assert_eq!(report["graph"]["source_files"], 2);
    assert_eq!(report["graph"]["local_edges"], 1);
    assert_eq!(report["graph"]["resource_imports"], 2);
    assert_eq!(report["compliance"]["complete"], true);
}

#[test]
fn dotted_module_stems_resolve_without_dropping_the_stem_suffix() {
    let f = Fixture::new();
    f.put(
        "src/resource.types.ts",
        "export interface Resource { id: string }",
    );
    f.put(
        "src/app.ts",
        "import type { Resource } from './resource.types'; export const value = 1;",
    );
    let report = f.json(&["architecture", "analyze", "."], 0);
    assert_eq!(report["graph"]["local_edges"], 1);
    assert_eq!(report["graph"]["edges"][0]["kind"], "type");
    assert_eq!(report["compliance"]["complete"], true);
}

#[test]
fn parser_failure_is_incomplete_and_worker_recovers_for_the_next_file() {
    let f = Fixture::new();
    f.put(
        "src/a.ts",
        format!(
            "const deep = {}1{};",
            "(".repeat(100_000),
            ")".repeat(100_000)
        ),
    );
    f.put(
        "src/b.ts",
        "export type Generic = import('./c').Box<{ value: string }>;",
    );
    f.put("src/c.ts", "export interface Box<T> { value: T }");
    let output = f.run(&["audit", "."]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stderr.is_empty(),
        "parser crash details must not leak to report stderr"
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let architecture = &report["architecture"];
    assert_eq!(architecture["compliance"]["complete"], false);
    assert!(report["scores"]["architecture"].is_null());
    assert_eq!(
        architecture["graph"]["unresolved_local_imports"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        architecture["graph"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["from"] == "src/b.ts" && e["to"] == "src/c.ts" && e["kind"] == "type")
    );
    f.put("audit.json", report.to_string());
    assert_eq!(f.json(&["gate", "audit.json"], 1)["passed"], false);
}

#[test]
fn internal_parser_protocol_rejects_oversized_and_invalid_frames() {
    use std::io::Write;
    use std::process::Stdio;
    for frame in [
        (16 * 1024 * 1024 + 1u32).to_le_bytes().to_vec(),
        vec![1, 0, 0, 0, b'{'],
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ah"))
            .arg("--internal-syntax-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&frame).unwrap();
        let result = child.wait_with_output().unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }
}

#[test]
fn mit_attribution_is_retained_without_setting_application_licensing() {
    let f = Fixture::new();
    for variant in [
        "base",
        "web-app",
        "backend-api",
        "saas",
        "monorepo",
        "library-sdk",
    ] {
        f.json(&["init", variant, "--boilerplate", variant], 0);
        let notice_path = format!("{variant}/.agentic/THIRD_PARTY_NOTICES.md");
        let notice = fs::read_to_string(f.path().join(&notice_path)).unwrap();
        assert!(notice.contains("MIT License"));
        assert!(notice.contains("Copyright (c) 2026 Agentic Harness contributors"));
        assert!(notice.contains("does not set the license"));
        assert!(!f.path().join(variant).join("LICENSE").exists());
        let lock: Value = serde_json::from_slice(
            &fs::read(f.path().join(variant).join(".agentic/lock.json")).unwrap(),
        )
        .unwrap();
        assert!(lock["checksums"][".agentic/THIRD_PARTY_NOTICES.md"].is_string());
        f.put(
            &format!("{variant}/LICENSE"),
            "Application owner's separate terms\n",
        );
        fs::remove_file(f.path().join(&notice_path)).unwrap(); // An older installation lacks notices.
        f.json(&["upgrade", variant], 0);
        assert_eq!(
            fs::read_to_string(f.path().join(&notice_path)).unwrap(),
            notice
        );
        assert_eq!(
            fs::read_to_string(f.path().join(variant).join("LICENSE")).unwrap(),
            "Application owner's separate terms\n"
        );
    }
}
