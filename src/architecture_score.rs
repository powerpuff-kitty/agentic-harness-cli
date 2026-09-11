use crate::{architecture_analysis, architecture_contract};
use serde_json::{Value, json};
use std::path::Path;

fn clamp(value: i64) -> i64 {
    value.clamp(0, 100)
}

fn invalid_analysis(root: &Path, message: String) -> Value {
    json!({
        "format_version": 1,
        "target": root,
        "profiles": [],
        "graph": {
            "source_files": 0,
            "local_edges": 0,
            "external_imports": 0,
            "unresolved_local_imports": [],
            "edges": [],
            "cycles": []
        },
        "compliance": {
            "passed": false,
            "deterministic_errors": 1,
            "warnings": 0
        },
        "findings": [{
            "rule_id": "architecture.contract.invalid",
            "severity": "error",
            "authority": "harness",
            "enforceability": "deterministic",
            "profile": null,
            "message": message,
            "evidence": {"path": architecture_contract::CONTRACT_PATH}
        }],
        "suppressed_findings": [],
        "exceptions_applied": [],
        "checks": {
            "performed": ["architecture contract validation"],
            "not_checked": ["source architecture analysis because the project contract is invalid"]
        }
    })
}

pub fn audit(root: &Path) -> Value {
    let profiles = match architecture_contract::profiles_for_analysis(root, &[]) {
        Ok(profiles) => profiles,
        Err(error) => return decorate(root, invalid_analysis(root, error)),
    };

    let mut analysis = architecture_analysis::analyze(root, &profiles);
    if let Err(error) = architecture_contract::apply_exceptions(root, &mut analysis) {
        return decorate(root, invalid_analysis(root, error));
    }
    decorate(root, analysis)
}

fn decorate(root: &Path, mut analysis: Value) -> Value {
    let source_files = analysis["graph"]["source_files"].as_i64().unwrap_or(0);
    let deterministic_errors = analysis["compliance"]["deterministic_errors"]
        .as_i64()
        .unwrap_or(0);
    let warnings = analysis["compliance"]["warnings"].as_i64().unwrap_or(0);
    let unresolved = analysis["graph"]["unresolved_local_imports"]
        .as_array()
        .map(|items| items.len() as i64)
        .unwrap_or(0);
    let contract_present = root.join(architecture_contract::CONTRACT_PATH).is_file();
    let docs_present = root.join("ARCHITECTURE.md").is_file()
        || root.join("docs/architecture").exists()
        || root.join(".agentic/ARCHITECTURE.md").is_file();

    let score = if source_files > 0 && unresolved == 0 && analysis["scan"]["complete"] != false {
        Some(clamp(
            100 - ((deterministic_errors * 100) / source_files.max(1)).min(100),
        ))
    } else {
        None
    };
    let coverage = if source_files > 0 {
        "supported-source-graph"
    } else {
        "limited-no-supported-source-graph"
    };
    analysis["score"] = json!(score);
    analysis["score_provenance"] = json!({"formula_version":2,"kind":"heuristic","formula":"100 - min(100, deterministic_errors * 100 / supported_source_files)","warnings":warnings,"unresolved_imports":"coverage gap; no score penalty","not_a_runtime_health_score":true});
    analysis["coverage"] = json!({
        "level": coverage,
        "supported_source_files": source_files,
        "unresolved_local_imports": unresolved,
        "contract_present": contract_present,
        "architecture_docs_present": docs_present,
        "score_without_supported_source_graph": null
    });
    analysis
}

pub fn codebase_findings(architecture: &Value) -> Vec<Value> {
    architecture["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|finding| {
            let architecture_severity = finding["severity"].as_str().unwrap_or("warning");
            let severity = match architecture_severity {
                "error" => "high",
                "warning" => "medium",
                _ => "low",
            };
            json!({
                "severity": severity,
                "dimension": "architecture",
                "rule_id": finding["rule_id"],
                "architecture_severity": finding["severity"],
                "message": finding["message"],
                "evidence": finding["evidence"],
                "authority": finding["authority"],
                "enforceability": finding["enforceability"],
                "profile": finding["profile"]
            })
        })
        .collect()
}

pub fn deterministic_error_count(audit: &Value) -> Option<u64> {
    audit["architecture"]["compliance"]["deterministic_errors"].as_u64()
}

pub fn gate_failure(audit: &Value, max_errors: u64) -> Option<String> {
    if audit["architecture"]["compliance"]["complete"] == false
        || audit["scan"]["complete"] == false
    {
        return Some("architecture or repository scan coverage is incomplete".into());
    }
    match deterministic_error_count(audit) {
        Some(actual) if actual > max_errors => Some(format!(
            "architecture deterministic errors {actual} > {max_errors}"
        )),
        Some(_) => None,
        None => Some("architecture compliance data is missing from audit".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "ah-architecture-score-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(root: &Path, path: &str, content: &str) {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, content).unwrap();
    }

    #[test]
    fn cycle_reduces_architecture_score() {
        let clean = fixture("clean");
        write(&clean, "src/a.ts", "export const a = 1;");
        write(
            &clean,
            "src/b.ts",
            "import { a } from './a'; export const b = a;",
        );
        let clean_audit = audit(&clean);

        let cycle = fixture("cycle");
        write(
            &cycle,
            "src/a.ts",
            "import { b } from './b'; export const a = b;",
        );
        write(
            &cycle,
            "src/b.ts",
            "import { a } from './a'; export const b = a;",
        );
        let cycle_audit = audit(&cycle);

        assert!(clean_audit["score"].as_i64().unwrap() > cycle_audit["score"].as_i64().unwrap());
        assert_eq!(cycle_audit["compliance"]["deterministic_errors"], 1);
        let _ = fs::remove_dir_all(clean);
        let _ = fs::remove_dir_all(cycle);
    }

    #[test]
    fn unsupported_graph_cannot_receive_high_score_from_docs_alone() {
        let root = fixture("unsupported");
        write(&root, "ARCHITECTURE.md", "# Architecture\n");
        write(&root, "src/main.rs", "fn main() {}\n");
        let result = audit(&root);
        assert!(result["score"].is_null());
        assert_eq!(
            result["coverage"]["level"],
            "limited-no-supported-source-graph"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn architecture_gate_is_independent_of_overall_score() {
        let data = json!({
            "overall": 99,
            "architecture": {"compliance": {"deterministic_errors": 2}}
        });
        assert!(gate_failure(&data, 0).is_some());
        assert!(gate_failure(&data, 2).is_none());
    }
}
