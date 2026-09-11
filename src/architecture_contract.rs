use crate::architecture;
use serde_json::{Value, json};
use std::collections::BTreeSet;
#[cfg(test)]
use std::fs;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

pub const CONTRACT_PATH: &str = ".agentic/architecture.json";

fn rule(id: &str, profile: &str, severity: &str, authority: &str, enforceability: &str) -> Value {
    json!({
        "id": id,
        "profile": profile,
        "severity": severity,
        "authority": authority,
        "enforceability": enforceability,
    })
}

pub fn known_profile(profile: &str) -> bool {
    matches!(
        profile,
        "pattern/dependency-hygiene/1"
            | "pattern/feature-first/1"
            | "pattern/layered/1"
            | "framework/nuxt/4"
    )
}

fn rules_for_profile(profile: &str) -> Vec<Value> {
    match profile {
        "pattern/dependency-hygiene/1" => vec![rule(
            "dependency.no-import-cycles",
            profile,
            "error",
            "harness",
            "deterministic",
        )],
        "pattern/feature-first/1" => vec![
            rule(
                "feature-first.group-by-feature",
                profile,
                "recommendation",
                "harness",
                "heuristic",
            ),
            rule(
                "feature-first.no-cross-feature-internals",
                profile,
                "error",
                "harness",
                "deterministic",
            ),
            rule(
                "feature-first.shared-not-dumping-ground",
                profile,
                "warning",
                "harness",
                "heuristic",
            ),
        ],
        "pattern/layered/1" => vec![
            rule(
                "layered.presentation-direction",
                profile,
                "error",
                "harness",
                "deterministic",
            ),
            rule(
                "layered.application-direction",
                profile,
                "error",
                "harness",
                "deterministic",
            ),
            rule(
                "layered.domain-independent",
                profile,
                "error",
                "harness",
                "deterministic",
            ),
            rule(
                "layered.infrastructure-inward",
                profile,
                "error",
                "harness",
                "deterministic",
            ),
        ],
        "framework/nuxt/4" => vec![
            rule(
                "nuxt.app.path-role",
                profile,
                "warning",
                "official",
                "deterministic",
            ),
            rule(
                "nuxt.server.path-role",
                profile,
                "warning",
                "official",
                "deterministic",
            ),
            rule(
                "nuxt.boundary.app-server",
                profile,
                "error",
                "official",
                "deterministic",
            ),
            rule(
                "nuxt.shared.neutral-boundary",
                profile,
                "error",
                "official",
                "deterministic",
            ),
            rule(
                "nuxt.layers.modular-architecture",
                profile,
                "recommendation",
                "official",
                "advisory",
            ),
        ],
        _ => Vec::new(),
    }
}

fn detection_profiles(detection: &Value) -> BTreeSet<String> {
    let mut profiles = BTreeSet::new();
    if let Some(candidates) = detection
        .get("candidate_profiles")
        .and_then(Value::as_array)
    {
        profiles.extend(
            candidates
                .iter()
                .filter_map(Value::as_str)
                .filter(|profile| known_profile(profile))
                .map(str::to_string),
        );
    }
    if detection["structure"]["layer_buckets"]
        .as_array()
        .is_some_and(|items| items.len() >= 3)
    {
        profiles.insert("pattern/layered/1".to_string());
    }
    profiles
}

pub fn resolve_profiles(root: &Path, requested: &[String]) -> Result<BTreeSet<String>, String> {
    let mut profiles = BTreeSet::new();
    profiles.insert("pattern/dependency-hygiene/1".to_string());
    if requested.is_empty() {
        profiles.extend(detection_profiles(&architecture::detect(root)));
    } else {
        for profile in requested {
            if !known_profile(profile) {
                return Err(format!("unsupported architecture profile: {profile}"));
            }
            profiles.insert(profile.clone());
        }
    }
    Ok(profiles)
}

fn exception_path_is_broad(path: &str) -> bool {
    matches!(path.trim(), "" | "*" | "**" | "/**" | "./**" | "src/**")
}

fn validate_exceptions(value: &Value) -> Result<Vec<Value>, String> {
    let Some(items) = value.as_array() else {
        return Err("architecture contract exceptions must be an array".to_string());
    };
    let mut result = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let Some(object) = item.as_object() else {
            return Err(format!("architecture exception {index} must be an object"));
        };
        let Some(rule_id) = object.get("rule_id").and_then(Value::as_str) else {
            return Err(format!("architecture exception {index} is missing rule_id"));
        };
        let Some(path) = object.get("path").and_then(Value::as_str) else {
            return Err(format!("architecture exception {index} is missing path"));
        };
        let Some(rationale) = object.get("rationale").and_then(Value::as_str) else {
            return Err(format!(
                "architecture exception {index} is missing rationale"
            ));
        };
        if rule_id.trim().is_empty() {
            return Err(format!(
                "architecture exception {index} has an empty rule_id"
            ));
        }
        if exception_path_is_broad(path) {
            return Err(format!(
                "architecture exception {index} is too broad; scope it below a concrete directory or file"
            ));
        }
        if rationale.trim().len() < 8 {
            return Err(format!(
                "architecture exception {index} rationale is too short to be auditable"
            ));
        }
        if let Some(expires) = object.get("expires") {
            let Some(expires) = expires.as_str() else {
                return Err(format!(
                    "architecture exception {index} expires must be a string"
                ));
            };
            crate::date::validate(expires)
                .map_err(|e| format!("architecture exception {index}: {e}"))?;
        }
        result.push(item.clone());
    }
    result.sort_by(|left, right| {
        let left_key = (
            left["rule_id"].as_str().unwrap_or_default(),
            left["path"].as_str().unwrap_or_default(),
        );
        let right_key = (
            right["rule_id"].as_str().unwrap_or_default(),
            right["path"].as_str().unwrap_or_default(),
        );
        left_key.cmp(&right_key)
    });
    Ok(result)
}

pub fn load_contract(root: &Path) -> Result<Option<Value>, String> {
    let path = root.join(CONTRACT_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let text = crate::scan::read(root, &path, 1_000_000)?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    if value["format_version"] != 1 || value["kind"] != "architecture-contract" {
        return Err(format!(
            "{} is not an Architecture Contract v1",
            path.display()
        ));
    }
    if let Some(profiles) = value.get("profiles").and_then(Value::as_array) {
        for profile in profiles.iter().filter_map(Value::as_str) {
            if !known_profile(profile) {
                return Err(format!(
                    "{} references unsupported architecture profile: {profile}",
                    path.display()
                ));
            }
        }
    } else {
        return Err(format!("{} is missing profiles", path.display()));
    }
    validate_exceptions(value.get("exceptions").unwrap_or(&json!([])))?;
    Ok(Some(value))
}

fn existing_exceptions(root: &Path) -> Result<Vec<Value>, String> {
    match load_contract(root)? {
        Some(value) => validate_exceptions(value.get("exceptions").unwrap_or(&json!([]))),
        None => Ok(Vec::new()),
    }
}

pub fn contract(root: &Path, requested: &[String]) -> Result<Value, String> {
    let profiles = resolve_profiles(root, requested)?;
    let exceptions = existing_exceptions(root)?;
    let mut rules = Vec::new();
    for profile in &profiles {
        rules.extend(rules_for_profile(profile));
    }
    rules.sort_by(|left, right| {
        left["id"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["id"].as_str().unwrap_or_default())
    });
    Ok(json!({
        "format_version": 1,
        "kind": "architecture-contract",
        "registry_schema_version": 1,
        "profiles": profiles,
        "rules": rules,
        "exceptions": exceptions,
    }))
}

pub fn preview(root: &Path, requested: &[String]) -> Result<Value, String> {
    let contract = contract(root, requested)?;
    Ok(json!({
        "target": root,
        "write": false,
        "path": CONTRACT_PATH,
        "contract": contract,
        "conflicts": [],
        "write_plan": {
            "create_or_replace": CONTRACT_PATH,
            "preserves": ["exceptions"],
        }
    }))
}

pub fn write(root: &Path, requested: &[String]) -> Result<Value, String> {
    let contract = contract(root, requested)?;
    let path = root.join(CONTRACT_PATH);
    let mut text = serde_json::to_string_pretty(&contract)
        .map_err(|error| format!("failed to serialize architecture contract: {error}"))?;
    text.push('\n');
    crate::scan::write(root, CONTRACT_PATH, text.as_bytes())
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(json!({
        "target": root,
        "write": true,
        "written": CONTRACT_PATH,
        "contract": contract,
        "conflicts": [],
    }))
}

pub fn profiles_for_analysis(root: &Path, explicit: &[String]) -> Result<Vec<String>, String> {
    if !explicit.is_empty() {
        return Ok(explicit.to_vec());
    }
    let Some(contract) = load_contract(root)? else {
        return Ok(Vec::new());
    };
    Ok(contract["profiles"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect())
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim_start_matches("./").trim_start_matches('/');
    let path = path.trim_start_matches("./").trim_start_matches('/');
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    path == pattern
}

fn finding_matches_exception(finding: &Value, exception: &Value) -> bool {
    if finding["rule_id"] != exception["rule_id"] {
        return false;
    }
    let Some(pattern) = exception["path"].as_str() else {
        return false;
    };
    for key in ["from", "to"] {
        if finding["evidence"][key]
            .as_str()
            .is_some_and(|path| path_matches(pattern, path))
        {
            return true;
        }
    }
    finding["evidence"]["modules"]
        .as_array()
        .is_some_and(|modules| {
            modules
                .iter()
                .filter_map(Value::as_str)
                .any(|path| path_matches(pattern, path))
        })
}

pub fn apply_exceptions(root: &Path, analysis: &mut Value) -> Result<(), String> {
    let Some(contract) = load_contract(root)? else {
        analysis["suppressed_findings"] = json!([]);
        analysis["exceptions_applied"] = json!([]);
        return Ok(());
    };
    let exceptions = validate_exceptions(contract.get("exceptions").unwrap_or(&json!([])))?;
    if exceptions.is_empty() {
        analysis["suppressed_findings"] = json!([]);
        analysis["exceptions_applied"] = json!([]);
        return Ok(());
    }

    let today = crate::date::today();
    analysis["evaluation_date"] = json!(today);
    analysis["expired_exceptions"] = json!(
        exceptions
            .iter()
            .filter(|e| e["expires"].as_str().is_some_and(|d| d < today.as_str()))
            .collect::<Vec<_>>()
    );
    let current = analysis["findings"].as_array().cloned().unwrap_or_default();
    let mut kept = Vec::new();
    let mut suppressed = Vec::new();
    let mut applied = BTreeSet::new();
    for finding in current {
        if let Some((index, _exception)) = exceptions.iter().enumerate().find(|(_, exception)| {
            exception["expires"]
                .as_str()
                .is_none_or(|date| date >= today.as_str())
                && finding_matches_exception(&finding, exception)
        }) {
            let mut suppressed_finding = finding.clone();
            suppressed_finding["suppressed_by_exception"] = json!(index);
            suppressed.push(suppressed_finding);
            applied.insert(index);
        } else {
            kept.push(finding);
        }
    }

    let deterministic_errors = kept
        .iter()
        .filter(|finding| {
            finding["severity"] == "error" && finding["enforceability"] == "deterministic"
        })
        .count();
    let warnings = kept
        .iter()
        .filter(|finding| finding["severity"] == "warning")
        .count();
    analysis["findings"] = json!(kept);
    analysis["suppressed_findings"] = json!(suppressed);
    analysis["exceptions_applied"] = json!(applied.into_iter().collect::<Vec<_>>());
    analysis["compliance"]["deterministic_errors"] = json!(deterministic_errors);
    analysis["compliance"]["warnings"] = json!(warnings);
    analysis["compliance"]["passed"] = json!(deterministic_errors == 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn fixture(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "ah-architecture-contract-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn preview_is_deterministic() {
        let root = fixture("preview");
        let first = preview(&root, &["pattern/feature-first/1".to_string()]).unwrap();
        let second = preview(&root, &["pattern/feature-first/1".to_string()]).unwrap();
        assert_eq!(first["contract"], second["contract"]);
        assert!(!root.join(CONTRACT_PATH).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_creates_contract() {
        let root = fixture("write");
        write(&root, &["pattern/layered/1".to_string()]).unwrap();
        assert!(root.join(CONTRACT_PATH).exists());
        let loaded = load_contract(&root).unwrap().unwrap();
        assert!(
            loaded["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| profile == "pattern/layered/1")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_preserves_existing_exceptions() {
        let root = fixture("preserve");
        let path = root.join(CONTRACT_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
              "format_version":1,
              "kind":"architecture-contract",
              "registry_schema_version":1,
              "profiles":["pattern/dependency-hygiene/1"],
              "rules":[],
              "exceptions":[{
                "rule_id":"dependency.no-import-cycles",
                "path":"src/legacy/**",
                "rationale":"Temporary legacy boundary during migration",
                "expires":"2026-12-31"
              }]
            }"#,
        )
        .unwrap();
        write(&root, &["pattern/feature-first/1".to_string()]).unwrap();
        let loaded = load_contract(&root).unwrap().unwrap();
        assert_eq!(loaded["exceptions"].as_array().unwrap().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_blanket_exception() {
        let value = json!([{
            "rule_id":"dependency.no-import-cycles",
            "path":"**",
            "rationale":"This would disable the rule everywhere"
        }]);
        assert!(validate_exceptions(&value).is_err());
    }

    #[test]
    fn suppresses_matching_finding() {
        let root = fixture("suppress");
        let path = root.join(CONTRACT_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
              "format_version":1,
              "kind":"architecture-contract",
              "registry_schema_version":1,
              "profiles":["pattern/dependency-hygiene/1"],
              "rules":[],
              "exceptions":[{
                "rule_id":"dependency.no-import-cycles",
                "path":"src/legacy/**",
                "rationale":"Temporary legacy cycle while migration lands"
              }]
            }"#,
        )
        .unwrap();
        let mut analysis = json!({
            "findings":[{
                "rule_id":"dependency.no-import-cycles",
                "severity":"error",
                "enforceability":"deterministic",
                "evidence":{"modules":["src/legacy/a.ts","src/legacy/b.ts"]}
            }],
            "compliance":{"passed":false,"deterministic_errors":1,"warnings":0}
        });
        apply_exceptions(&root, &mut analysis).unwrap();
        assert!(analysis["findings"].as_array().unwrap().is_empty());
        assert_eq!(analysis["suppressed_findings"].as_array().unwrap().len(), 1);
        assert_eq!(analysis["compliance"]["passed"], true);
        let _ = fs::remove_dir_all(root);
    }
}
