use crate::scan;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn language_for(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|value| value.to_str()) {
        Some("ts" | "tsx" | "mts" | "cts") => Some("typescript"),
        Some("js" | "jsx" | "mjs" | "cjs") => Some("javascript"),
        Some("vue") => Some("vue"),
        Some("rs") => Some("rust"),
        Some("py") => Some("python"),
        Some("go") => Some("go"),
        _ => None,
    }
}

fn config_tool(name: &str) -> Option<&'static str> {
    if name == ".eslintrc" || name.starts_with(".eslintrc.") || name.starts_with("eslint.config.") {
        Some("eslint")
    } else if name.starts_with(".prettierrc") || name.starts_with("prettier.config.") {
        Some("prettier")
    } else if ["biome.json", "biome.jsonc"].contains(&name) {
        Some("biome")
    } else if name == ".editorconfig" {
        Some("editorconfig")
    } else if name == "tsconfig.json" || (name.starts_with("tsconfig.") && name.ends_with(".json"))
    {
        Some("typescript")
    } else if ["rustfmt.toml", ".rustfmt.toml"].contains(&name) {
        Some("rustfmt")
    } else if ["clippy.toml", ".clippy.toml"].contains(&name) {
        Some("clippy")
    } else if ["ruff.toml", ".ruff.toml"].contains(&name) {
        Some("ruff")
    } else {
        None
    }
}

fn dependency_tool(name: &str) -> Option<&'static str> {
    match name {
        "eslint" => Some("eslint"),
        "prettier" => Some("prettier"),
        "@biomejs/biome" => Some("biome"),
        "typescript" => Some("typescript"),
        "ruff" => Some("ruff"),
        _ => None,
    }
}

fn command_tool(command: &str) -> Option<&'static str> {
    let lower = command.to_ascii_lowercase();
    if lower.contains("eslint") {
        Some("eslint")
    } else if lower.contains("prettier") {
        Some("prettier")
    } else if lower.contains("biome") {
        Some("biome")
    } else if lower.contains("tsc") {
        Some("typescript")
    } else if lower.contains("cargo fmt") {
        Some("rustfmt")
    } else if lower.contains("clippy") {
        Some("clippy")
    } else if lower.contains("ruff") {
        Some("ruff")
    } else if lower.contains("gofmt") || lower.contains("go fmt") {
        Some("gofmt")
    } else if lower.contains("go vet") {
        Some("go-vet")
    } else {
        None
    }
}

fn quality_script(name: &str) -> bool {
    ["lint", "format", "fmt", "typecheck", "check"]
        .iter()
        .any(|prefix| name == *prefix || name.starts_with(&format!("{prefix}:")))
}

fn add_tool_evidence(
    tools: &mut BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    tool: &str,
    config: Option<String>,
    command: Option<String>,
) {
    let evidence = tools.entry(tool.to_string()).or_default();
    if let Some(config) = config {
        evidence.0.insert(config);
    }
    if let Some(command) = command {
        evidence.1.insert(command);
    }
}

fn inspect_package(
    root: &Path,
    path: &Path,
    tools: &mut BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    package_checks: &mut Vec<Value>,
    unsupported: &mut BTreeSet<String>,
) {
    let rel = relative(root, path);
    let Ok(text) = scan::read(root, path, 1_000_000) else {
        unsupported.insert(format!("{rel}: package.json could not be read"));
        return;
    };
    let Ok(package) = serde_json::from_str::<Value>(&text) else {
        unsupported.insert(format!("{rel}: package.json is not valid JSON"));
        return;
    };

    for section in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(dependencies) = package[section].as_object() {
            for name in dependencies.keys() {
                if let Some(tool) = dependency_tool(name) {
                    add_tool_evidence(tools, tool, Some(rel.clone()), None);
                }
            }
        }
    }

    if let Some(scripts) = package["scripts"].as_object() {
        for (name, command) in scripts {
            let Some(command) = command.as_str() else {
                continue;
            };
            if quality_script(name) {
                package_checks.push(json!({
                    "path": rel,
                    "name": name,
                    "command": command,
                    "executed": false
                }));
            }
            if let Some(tool) = command_tool(command) {
                add_tool_evidence(
                    tools,
                    tool,
                    Some(rel.clone()),
                    Some(format!("{name}: {command}")),
                );
            }
        }
    }

    if package.get("prettier").is_some() {
        add_tool_evidence(tools, "prettier", Some(rel.clone()), None);
    }
    if package.get("eslintConfig").is_some() {
        add_tool_evidence(tools, "eslint", Some(rel), None);
    }
}

pub fn detect(root: &Path) -> Value {
    let inventory = scan::inventory(root);
    let mut languages = BTreeSet::new();
    let mut tools: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    let mut package_checks = Vec::new();
    let mut unsupported = BTreeSet::new();

    for path in &inventory.files {
        if let Some(language) = language_for(path) {
            languages.insert(language.to_string());
        }

        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if let Some(tool) = config_tool(name) {
            add_tool_evidence(&mut tools, tool, Some(relative(root, path)), None);
        }

        if name == "package.json" {
            inspect_package(
                root,
                path,
                &mut tools,
                &mut package_checks,
                &mut unsupported,
            );
        }

        if name == "Cargo.toml" {
            languages.insert("rust".to_string());
        }
        if name == "pyproject.toml" {
            languages.insert("python".to_string());
        }
        if name == "go.mod" {
            languages.insert("go".to_string());
        }
    }

    for error in &inventory.errors {
        unsupported.insert(format!("repository scan: {error}"));
    }

    let tools = tools
        .into_iter()
        .map(|(id, (config, commands))| {
            json!({
                "id": id,
                "detected": true,
                "executed": false,
                "config": config,
                "commands": commands,
                "version": Value::Null
            })
        })
        .collect::<Vec<_>>();

    json!({
        "format_version": 1,
        "kind": "quality-detection",
        "target": root.to_string_lossy(),
        "languages": languages,
        "tools": tools,
        "package_checks": package_checks,
        "scan": inventory.report(),
        "coverage": {
            "performed": [
                "repository inventory",
                "language detection",
                "quality tool/config detection",
                "quality package-script discovery"
            ],
            "not_checked": [
                "tool availability or version",
                "formatter execution",
                "lint execution",
                "typecheck execution",
                "project command execution"
            ],
            "unsupported": unsupported
        }
    })
}

fn tsconfig_paths(root: &Path) -> Vec<PathBuf> {
    scan::inventory(root)
        .files
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| {
                    name == "tsconfig.json"
                        || (name.starts_with("tsconfig.") && name.ends_with(".json"))
                })
        })
        .collect()
}

pub fn analyze(root: &Path) -> Value {
    let detection = detect(root);
    let mut findings = Vec::new();
    let mut performed = BTreeSet::from([
        "repository inventory".to_string(),
        "language detection".to_string(),
        "quality tool/config detection".to_string(),
        "quality package-script discovery".to_string(),
    ]);
    let mut not_checked = BTreeSet::from([
        "formatter execution".to_string(),
        "lint execution".to_string(),
        "typecheck execution".to_string(),
        "complexity analysis".to_string(),
        "duplication analysis".to_string(),
        "dead-code analysis".to_string(),
        "autofix".to_string(),
    ]);
    let mut unsupported = BTreeSet::new();

    for path in tsconfig_paths(root) {
        let rel = relative(root, &path);
        let Ok(text) = scan::read(root, &path, 1_000_000) else {
            unsupported.insert(format!("{rel}: tsconfig could not be read"));
            continue;
        };
        let Ok(config) = serde_json::from_str::<Value>(&text) else {
            unsupported.insert(format!(
                "{rel}: JSONC/invalid JSON tsconfig static inspection is not supported"
            ));
            continue;
        };
        performed.insert("plain JSON tsconfig strict-mode inspection".to_string());

        match config
            .get("compilerOptions")
            .and_then(|value| value.get("strict"))
            .and_then(Value::as_bool)
        {
            Some(false) => findings.push(json!({
                "rule_id": "typescript.type-safety.strict-mode",
                "native_code": Value::Null,
                "category": "type-safety",
                "severity": "recommendation",
                "enforceability": "deterministic",
                "message": "TypeScript strict mode is explicitly disabled in this configuration.",
                "evidence": [{
                    "path": rel,
                    "line": Value::Null,
                    "detail": "compilerOptions.strict=false"
                }],
                "remediation": "Review whether the project quality contract should require strict mode; do not enable it blindly on a legacy codebase."
            })),
            Some(true) => {}
            None => {
                not_checked.insert(
                    "effective TypeScript strictness when inherited or expressed through individual strict flags"
                        .to_string(),
                );
            }
        }
    }

    if let Some(items) = detection["coverage"]["unsupported"].as_array() {
        for item in items {
            if let Some(item) = item.as_str() {
                unsupported.insert(item.to_string());
            }
        }
    }

    json!({
        "format_version": 1,
        "kind": "quality-analysis",
        "target": detection["target"].clone(),
        "languages": detection["languages"].clone(),
        "tools": detection["tools"].clone(),
        "findings": findings,
        "coverage": {
            "performed": performed,
            "not_checked": not_checked,
            "unsupported": unsupported
        }
    })
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn contract_digest(root: &Path) -> Result<String, String> {
    let path = root.join(".agentic/quality.json");
    if !path.exists() {
        return Ok(sha256(b"absent-quality-contract"));
    }
    let text = scan::read(root, &path, 1_000_000)
        .map_err(|error| format!(".agentic/quality.json: {error}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!(".agentic/quality.json is not valid JSON: {error}"))?;
    if value["format_version"] != 1 || value["kind"] != "quality-contract" {
        return Err(
            ".agentic/quality.json must be a Quality Contract v1 before it can anchor a baseline"
                .to_string(),
        );
    }
    Ok(sha256(text.as_bytes()))
}

fn tool_config_digest(root: &Path, tool: &Value) -> Option<String> {
    let configs = tool["config"].as_array()?;
    if configs.is_empty() {
        return None;
    }
    let mut bytes = Vec::new();
    for config in configs {
        let path = config.as_str()?;
        let full = root.join(path);
        let text = scan::read(root, &full, 1_000_000).ok()?;
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
    }
    Some(sha256(&bytes))
}

fn baseline_finding(finding: &Value) -> Result<Value, String> {
    let rule_id = finding["rule_id"]
        .as_str()
        .ok_or_else(|| "quality finding is missing rule_id".to_string())?;
    let severity = finding["severity"]
        .as_str()
        .ok_or_else(|| "quality finding is missing severity".to_string())?;
    let evidence = finding["evidence"]
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| format!("{rule_id}: baseline finding has no source evidence"))?;
    let path = evidence["path"]
        .as_str()
        .ok_or_else(|| format!("{rule_id}: baseline finding evidence has no path"))?;
    let line = evidence["line"].as_u64();
    let native_code = finding["native_code"].as_str();
    let fingerprint_source = format!(
        "{rule_id}\0{}\0{path}\0{}",
        native_code.unwrap_or(""),
        line.map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    );

    Ok(json!({
        "fingerprint": sha256(fingerprint_source.as_bytes()),
        "rule_id": rule_id,
        "native_code": native_code,
        "severity": severity,
        "path": path,
        "line": line
    }))
}

pub fn baseline(root: &Path) -> Result<Value, String> {
    let analysis = analyze(root);
    let contract_digest = contract_digest(root)?;
    let mut tools = Vec::new();
    for tool in analysis["tools"].as_array().into_iter().flatten() {
        let id = tool["id"]
            .as_str()
            .ok_or_else(|| "quality tool entry is missing id".to_string())?;
        tools.push(json!({
            "id": id,
            "version": tool["version"].clone(),
            "config_digest": tool_config_digest(root, tool)
        }));
    }

    let mut findings = Vec::new();
    for finding in analysis["findings"].as_array().into_iter().flatten() {
        findings.push(baseline_finding(finding)?);
    }
    findings.sort_by(|left, right| {
        left["fingerprint"]
            .as_str()
            .cmp(&right["fingerprint"].as_str())
    });

    Ok(json!({
        "format_version": 1,
        "kind": "quality-baseline",
        "created_at": crate::date::today(),
        "target": {
            "root": root.to_string_lossy(),
            "revision": Value::Null,
            "tree_digest": Value::Null
        },
        "contract_digest": contract_digest,
        "tools": tools,
        "findings": findings,
        "metrics": {},
        "coverage": analysis["coverage"].clone()
    }))
}

fn validate_baseline(value: &Value) -> Result<(), String> {
    if value["format_version"] != 1 || value["kind"] != "quality-baseline" {
        return Err("expected Quality Baseline v1".to_string());
    }
    if !value["contract_digest"].is_string()
        || !value["tools"].is_array()
        || !value["findings"].is_array()
        || !value["coverage"].is_object()
    {
        return Err("quality baseline is missing required identity/evidence fields".to_string());
    }
    for finding in value["findings"].as_array().unwrap() {
        if !finding["fingerprint"].is_string()
            || !finding["rule_id"].is_string()
            || !finding["severity"].is_string()
            || !finding["path"].is_string()
        {
            return Err("quality baseline contains an invalid finding".to_string());
        }
    }
    Ok(())
}

pub fn diff(root: &Path, baseline_path: &str, previous: &Value) -> Result<Value, String> {
    validate_baseline(previous)?;
    let current = baseline(root)?;

    let mut stale_reasons = BTreeSet::new();
    if previous["contract_digest"] != current["contract_digest"] {
        stale_reasons.insert("quality contract identity changed".to_string());
    }
    if previous["tools"] != current["tools"] {
        stale_reasons.insert("quality tool/config identity changed".to_string());
    }

    let old = previous["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| {
            finding["fingerprint"]
                .as_str()
                .map(|fingerprint| (fingerprint.to_string(), finding.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let new = current["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| {
            finding["fingerprint"]
                .as_str()
                .map(|fingerprint| (fingerprint.to_string(), finding.clone()))
        })
        .collect::<BTreeMap<_, _>>();

    let added = new
        .iter()
        .filter(|(fingerprint, _)| !old.contains_key(*fingerprint))
        .map(|(_, finding)| finding.clone())
        .collect::<Vec<_>>();
    let removed = old
        .iter()
        .filter(|(fingerprint, _)| !new.contains_key(*fingerprint))
        .map(|(_, finding)| finding.clone())
        .collect::<Vec<_>>();
    let unchanged = new
        .keys()
        .filter(|fingerprint| old.contains_key(*fingerprint))
        .count();

    Ok(json!({
        "format_version": 1,
        "kind": "quality-diff",
        "baseline": baseline_path,
        "current": root.to_string_lossy(),
        "stale": !stale_reasons.is_empty(),
        "stale_reasons": stale_reasons,
        "added": added,
        "removed": removed,
        "unchanged": unchanged,
        "coverage": current["coverage"].clone()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &Path, path: &str, text: &str) {
        let destination = root.join(path);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, text).unwrap();
    }

    #[test]
    fn detection_records_configuration_without_execution() {
        let temp = tempfile::tempdir().unwrap();
        put(
            temp.path(),
            "package.json",
            r#"{
              "scripts":{"lint":"eslint src","format":"prettier --check .","typecheck":"tsc --noEmit"},
              "devDependencies":{"eslint":"9.0.0","prettier":"3.0.0","typescript":"5.9.3"}
            }"#,
        );
        put(
            temp.path(),
            "src/app.ts",
            "export const answer: number = 42;",
        );

        let report = detect(temp.path());
        assert_eq!(report["kind"], "quality-detection");
        assert!(
            report["languages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "typescript")
        );
        assert!(report["tools"].as_array().unwrap().iter().all(|tool| {
            tool["detected"] == true && tool["executed"] == false && tool["version"].is_null()
        }));
        assert_eq!(report["package_checks"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn analysis_reports_explicitly_disabled_typescript_strict_mode() {
        let temp = tempfile::tempdir().unwrap();
        put(
            temp.path(),
            "tsconfig.json",
            r#"{"compilerOptions":{"strict":false}}"#,
        );
        put(temp.path(), "src/app.ts", "export const value: any = 1;");

        let report = analyze(temp.path());
        assert_eq!(report["kind"], "quality-analysis");
        assert_eq!(
            report["findings"][0]["rule_id"],
            "typescript.type-safety.strict-mode"
        );
        assert_eq!(report["findings"][0]["severity"], "recommendation");
        assert!(
            report["coverage"]["not_checked"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "lint execution")
        );
    }

    #[test]
    fn jsonc_tsconfig_is_a_coverage_gap_not_a_pass() {
        let temp = tempfile::tempdir().unwrap();
        put(
            temp.path(),
            "tsconfig.json",
            "{\n  // inherited strictness\n  \"extends\": \"./base.json\"\n}",
        );
        put(temp.path(), "src/app.ts", "export const value = 1;");

        let report = analyze(temp.path());
        assert!(report["findings"].as_array().unwrap().is_empty());
        assert!(
            report["coverage"]["unsupported"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value.as_str().unwrap().contains("JSONC"))
        );
    }
    #[test]
    fn baseline_keeps_findings_visible_and_diff_detects_config_staleness() {
        let temp = tempfile::tempdir().unwrap();
        put(
            temp.path(),
            "tsconfig.json",
            r#"{"compilerOptions":{"strict":false}}"#,
        );
        put(temp.path(), "src/app.ts", "export const value = 1;");
        let previous = baseline(temp.path()).unwrap();
        assert_eq!(previous["kind"], "quality-baseline");
        assert_eq!(previous["findings"].as_array().unwrap().len(), 1);

        put(
            temp.path(),
            "tsconfig.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        );
        let report = diff(temp.path(), "quality-baseline.json", &previous).unwrap();
        assert_eq!(report["kind"], "quality-diff");
        assert_eq!(report["stale"], true);
        assert_eq!(report["added"].as_array().unwrap().len(), 0);
        assert_eq!(report["removed"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn malformed_quality_contract_blocks_baseline_creation() {
        let temp = tempfile::tempdir().unwrap();
        put(temp.path(), ".agentic/quality.json", r#"{"kind":"other"}"#);
        let error = baseline(temp.path()).unwrap_err();
        assert!(error.contains("Quality Contract v1"));
    }
}
