use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

fn read_package_names(root: &Path) -> BTreeSet<String> {
    let path = root.join("package.json");
    let Ok(text) = fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    let Ok(value): Result<Value, _> = serde_json::from_str(&text) else {
        return BTreeSet::new();
    };

    let mut names = BTreeSet::new();
    for section in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(object) = value.get(section).and_then(Value::as_object) {
            names.extend(object.keys().cloned());
        }
    }
    names
}

fn has_any(root: &Path, paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .find(|path| root.join(path).exists())
        .map(|path| (*path).to_string())
}

fn has_extension(root: &Path, extension: &str) -> bool {
    fn walk(path: &Path, extension: &str, depth: usize) -> bool {
        if depth > 5 {
            return false;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if [
                "node_modules",
                ".git",
                "dist",
                "build",
                ".nuxt",
                ".next",
                "target",
            ]
            .contains(&name)
            {
                continue;
            }
            if path.is_dir() {
                if walk(&path, extension, depth + 1) {
                    return true;
                }
            } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
                return true;
            }
        }
        false
    }

    walk(root, extension, 0)
}

fn record(kind: &str, name: &str, confidence: &str, evidence: Vec<String>) -> Value {
    json!({
        "kind": kind,
        "name": name,
        "confidence": confidence,
        "evidence": evidence,
    })
}

fn project_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for candidate in ["src/app", "src", "app"] {
        let path = root.join(candidate);
        if path.is_dir() && seen.insert(path.clone()) {
            roots.push(path);
        }
    }
    if roots.is_empty() {
        roots.push(root.to_path_buf());
    }
    roots
}

fn top_level_dirs(root: &Path) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();
    for candidate in project_roots(root) {
        let Ok(entries) = fs::read_dir(candidate) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
                    dirs.insert(name.to_string());
                }
            }
        }
    }
    dirs
}

fn present(dirs: &BTreeSet<String>, names: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for name in names {
        if dirs.contains(*name) {
            found.push((*name).to_string());
        }
    }
    found
}

fn infer_structure(root: &Path) -> Value {
    let dirs = top_level_dirs(root);
    let technical = present(
        &dirs,
        &[
            "components",
            "services",
            "stores",
            "views",
            "pages",
            "api",
            "composables",
            "utils",
        ],
    );
    let features = present(&dirs, &["features", "domains", "modules"]);
    let layered = present(
        &dirs,
        &["presentation", "application", "domain", "infrastructure"],
    );
    let ports_adapters = dirs.contains("ports") && dirs.contains("adapters");
    let clean_layers = ["domain", "application", "infrastructure"]
        .iter()
        .all(|name| dirs.contains(*name));

    let (style, confidence, rationale) = if !features.is_empty() && technical.len() >= 3 {
        (
            "mixed",
            "medium",
            "Both feature/domain buckets and several technical-layer buckets were detected.",
        )
    } else if !features.is_empty() {
        (
            "feature-first",
            "high",
            "Feature/domain/module buckets were detected at an application source root.",
        )
    } else if ports_adapters || clean_layers {
        (
            "hexagonal-or-clean",
            "medium",
            "Domain/application/infrastructure or ports/adapters boundaries were detected.",
        )
    } else if layered.len() >= 3 {
        (
            "layered",
            "high",
            "Three or more canonical architecture-layer directories were detected.",
        )
    } else if technical.len() >= 3 {
        (
            "technical-layered",
            "high",
            "Several technical buckets such as components/services/stores/views/api were detected.",
        )
    } else {
        (
            "unknown",
            "low",
            "The visible directory layout does not match a supported architecture shape strongly enough.",
        )
    };

    json!({
        "style": style,
        "confidence": confidence,
        "rationale": rationale,
        "top_level_directories": dirs,
        "technical_buckets": technical,
        "feature_buckets": features,
        "layer_buckets": layered,
        "ports_and_adapters": ports_adapters,
    })
}

fn canonical_profile(kind: &str, name: &str) -> Option<&'static str> {
    match (kind, name) {
        ("framework", "vue") => Some("framework/vue/3"),
        ("framework", "nuxt") => Some("framework/nuxt/4"),
        ("framework", "angular") => Some("framework/angular/current"),
        ("tooling", "vite") => Some("tooling/vite/current"),
        ("tooling", "nx") => Some("tooling/nx/current"),
        ("ecosystem", "pinia") => Some("ecosystem/pinia/3"),
        ("ecosystem", "vue-router") => Some("ecosystem/vue-router/current"),
        ("pattern", "feature-first") => Some("pattern/feature-first/1"),
        _ => None,
    }
}

fn collect_profile(
    candidate_profiles: &mut BTreeSet<String>,
    unresolved_profile_families: &mut BTreeSet<String>,
    kind: &str,
    name: &str,
) {
    if let Some(profile) = canonical_profile(kind, name) {
        candidate_profiles.insert(profile.to_string());
    } else {
        unresolved_profile_families.insert(format!("{kind}/{name}"));
    }
}

pub fn detect(root: &Path) -> Value {
    let packages = read_package_names(root);
    let mut frameworks = Vec::new();
    let mut tooling = Vec::new();
    let mut ecosystem = Vec::new();
    let mut languages = Vec::new();

    let has_nuxt = packages.contains("nuxt")
        || has_any(
            root,
            &["nuxt.config.ts", "nuxt.config.js", "nuxt.config.mjs"],
        )
        .is_some();
    let has_angular = packages.contains("@angular/core") || root.join("angular.json").exists();
    let has_next = packages.contains("next")
        || has_any(
            root,
            &["next.config.js", "next.config.mjs", "next.config.ts"],
        )
        .is_some();
    let has_vue = packages.contains("vue") || has_extension(root, "vue");
    let has_react =
        packages.contains("react") || has_extension(root, "jsx") || has_extension(root, "tsx");

    if has_nuxt {
        let mut evidence = Vec::new();
        if packages.contains("nuxt") {
            evidence.push("package.json:nuxt".to_string());
        }
        if let Some(path) = has_any(
            root,
            &["nuxt.config.ts", "nuxt.config.js", "nuxt.config.mjs"],
        ) {
            evidence.push(path);
        }
        frameworks.push(record("framework", "nuxt", "high", evidence));
    } else if has_angular {
        let mut evidence = Vec::new();
        if packages.contains("@angular/core") {
            evidence.push("package.json:@angular/core".to_string());
        }
        if root.join("angular.json").exists() {
            evidence.push("angular.json".to_string());
        }
        frameworks.push(record("framework", "angular", "high", evidence));
    } else if has_next {
        let mut evidence = Vec::new();
        if packages.contains("next") {
            evidence.push("package.json:next".to_string());
        }
        if let Some(path) = has_any(
            root,
            &["next.config.js", "next.config.mjs", "next.config.ts"],
        ) {
            evidence.push(path);
        }
        frameworks.push(record("framework", "next", "high", evidence));
    } else if has_vue {
        let mut evidence = Vec::new();
        if packages.contains("vue") {
            evidence.push("package.json:vue".to_string());
        }
        if has_extension(root, "vue") {
            evidence.push("source:**/*.vue".to_string());
        }
        frameworks.push(record("framework", "vue", "high", evidence));
    } else if has_react {
        let mut evidence = Vec::new();
        if packages.contains("react") {
            evidence.push("package.json:react".to_string());
        }
        let confidence = if packages.contains("react") {
            "high"
        } else {
            "medium"
        };
        frameworks.push(record("framework", "react", confidence, evidence));
    }

    if packages.contains("vite")
        || has_any(
            root,
            &["vite.config.ts", "vite.config.js", "vite.config.mjs"],
        )
        .is_some()
    {
        let mut evidence = Vec::new();
        if packages.contains("vite") {
            evidence.push("package.json:vite".to_string());
        }
        if let Some(path) = has_any(
            root,
            &["vite.config.ts", "vite.config.js", "vite.config.mjs"],
        ) {
            evidence.push(path);
        }
        tooling.push(record("build-tool", "vite", "high", evidence));
    }
    if packages.contains("nx") || packages.contains("@nx/devkit") || root.join("nx.json").exists()
    {
        let mut evidence = Vec::new();
        if packages.contains("nx") {
            evidence.push("package.json:nx".to_string());
        }
        if packages.contains("@nx/devkit") {
            evidence.push("package.json:@nx/devkit".to_string());
        }
        if root.join("nx.json").exists() {
            evidence.push("nx.json".to_string());
        }
        tooling.push(record("workspace-tool", "nx", "high", evidence));
    }

    for (package, name, kind) in [
        ("pinia", "pinia", "state"),
        ("vue-router", "vue-router", "router"),
        ("@angular/router", "angular-router", "router"),
    ] {
        if packages.contains(package) {
            ecosystem.push(record(
                kind,
                name,
                "high",
                vec![format!("package.json:{package}")],
            ));
        }
    }

    if packages.contains("typescript") || root.join("tsconfig.json").exists() {
        let mut evidence = Vec::new();
        if packages.contains("typescript") {
            evidence.push("package.json:typescript".to_string());
        }
        if root.join("tsconfig.json").exists() {
            evidence.push("tsconfig.json".to_string());
        }
        languages.push(record("language", "typescript", "high", evidence));
    } else if root.join("package.json").exists() {
        languages.push(record(
            "language",
            "javascript",
            "medium",
            vec!["package.json".to_string()],
        ));
    }

    let structure = infer_structure(root);
    let mut candidate_profiles = BTreeSet::new();
    let mut unresolved_profile_families = BTreeSet::new();

    for item in &frameworks {
        if let Some(name) = item.get("name").and_then(Value::as_str) {
            collect_profile(
                &mut candidate_profiles,
                &mut unresolved_profile_families,
                "framework",
                name,
            );
        }
    }
    for item in &tooling {
        if let Some(name) = item.get("name").and_then(Value::as_str) {
            collect_profile(
                &mut candidate_profiles,
                &mut unresolved_profile_families,
                "tooling",
                name,
            );
        }
    }
    for item in &ecosystem {
        if let Some(name) = item.get("name").and_then(Value::as_str) {
            collect_profile(
                &mut candidate_profiles,
                &mut unresolved_profile_families,
                "ecosystem",
                name,
            );
        }
    }
    if let Some(style) = structure.get("style").and_then(Value::as_str) {
        if style != "unknown" && style != "mixed" {
            collect_profile(
                &mut candidate_profiles,
                &mut unresolved_profile_families,
                "pattern",
                style,
            );
        }
    }

    json!({
        "target": root,
        "frameworks": frameworks,
        "tooling": tooling,
        "ecosystem": ecosystem,
        "languages": languages,
        "structure": structure,
        "candidate_profiles": candidate_profiles,
        "unresolved_profile_families": unresolved_profile_families,
        "notes": [
            "Detection is offline and evidence-based.",
            "Build tools such as Vite are reported separately from application architecture.",
            "Only profiles that currently exist in the canonical Architecture Registry are emitted as candidate profile IDs.",
            "Detected concepts without a current canonical profile are reported as unresolved profile families instead of inventing rules."
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn fixture(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "ah-architecture-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn detects_vue_vite_pinia_router_and_technical_layers() {
        let root = fixture("vue");
        fs::write(
            root.join("package.json"),
            r#"{
              "dependencies": {"vue":"^3.0.0","pinia":"^3.0.0","vue-router":"^4.0.0"},
              "devDependencies": {"vite":"^7.0.0","typescript":"^5.0.0"}
            }"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("src/components")).unwrap();
        fs::create_dir_all(root.join("src/services")).unwrap();
        fs::create_dir_all(root.join("src/stores")).unwrap();
        fs::create_dir_all(root.join("src/views")).unwrap();

        let result = detect(&root);
        assert_eq!(result["frameworks"][0]["name"], "vue");
        assert!(
            result["tooling"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["name"] == "vite")
        );
        assert!(
            result["ecosystem"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["name"] == "pinia")
        );
        assert_eq!(result["structure"]["style"], "technical-layered");
        assert!(
            result["candidate_profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "framework/vue/3")
        );
        assert!(
            result["candidate_profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "tooling/vite/current")
        );
        assert!(
            result["unresolved_profile_families"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "pattern/technical-layered")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_nuxt_as_framework_and_features_as_feature_first() {
        let root = fixture("nuxt");
        fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"nuxt":"^4.0.0","vue":"^3.0.0"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("nuxt.config.ts"),
            "export default defineNuxtConfig({})",
        )
        .unwrap();
        fs::create_dir_all(root.join("app/features/auth")).unwrap();

        let result = detect(&root);
        assert_eq!(result["frameworks"][0]["name"], "nuxt");
        assert_eq!(result["structure"]["style"], "feature-first");
        assert!(
            result["candidate_profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "framework/nuxt/4")
        );
        assert!(
            result["candidate_profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "pattern/feature-first/1")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_angular_src_app_as_feature_first() {
        let root = fixture("angular");
        fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@angular/core":"^21.0.0","@angular/router":"^21.0.0"}}"#,
        )
        .unwrap();
        fs::write(root.join("angular.json"), "{}").unwrap();
        fs::create_dir_all(root.join("src/app/features/orders")).unwrap();

        let result = detect(&root);
        assert_eq!(result["frameworks"][0]["name"], "angular");
        assert_eq!(result["structure"]["style"], "feature-first");
        assert!(
            result["ecosystem"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["name"] == "angular-router")
        );
        assert!(
            result["candidate_profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "framework/angular/current")
        );
        assert!(
            result["unresolved_profile_families"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "ecosystem/angular-router")
        );
        let _ = fs::remove_dir_all(root);
    }
}
