use serde_json::{Map, Value, json};

fn frequency_values(analysis: &Value, domain: &str) -> Vec<Value> {
    analysis["domains"][domain]["measurements"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|measurement| measurement["value"].as_array().into_iter().flatten())
        .cloned()
        .collect()
}

fn summary(analysis: &Value, domain: &str) -> Value {
    analysis["domains"][domain]["summary"].clone()
}

fn observation_rule(id: &str, statement: String) -> Value {
    json!({
        "id": id,
        "kind": "info",
        "importance": "optional",
        "statement": statement,
        "source_refs": ["analysis.static"]
    })
}

pub fn candidate_from_analysis(analysis: &Value, analysis_uri: &str) -> Result<Value, String> {
    if analysis["format_version"].as_u64() != Some(1) {
        return Err(
            "candidate genome generation requires Design Analysis format_version 1".to_string(),
        );
    }
    if !analysis["domains"].is_object() {
        return Err("candidate genome generation requires analysis.domains".to_string());
    }

    let colors = frequency_values(analysis, "color");
    let typography = frequency_values(analysis, "typography");
    let spacing = frequency_values(analysis, "spacing");
    let geometry = frequency_values(analysis, "geometry");
    let tokens = analysis["domains"]["tokens"]["measurements"]
        .as_array()
        .and_then(|rows| rows.first())
        .map(|row| row["value"].clone())
        .unwrap_or_else(|| json!({"defined": [], "referenced": []}));

    let mut visual = Map::new();
    visual.insert(
        "color".to_string(),
        json!({
            "evidence_status": "observed",
            "frequency": colors,
            "summary": summary(analysis, "color")
        }),
    );
    visual.insert(
        "typography".to_string(),
        json!({
            "evidence_status": "observed",
            "font_size_frequency": typography,
            "summary": summary(analysis, "typography")
        }),
    );
    visual.insert(
        "spacing".to_string(),
        json!({
            "evidence_status": "observed",
            "frequency": spacing,
            "summary": summary(analysis, "spacing")
        }),
    );
    visual.insert(
        "geometry".to_string(),
        json!({
            "evidence_status": "observed",
            "radius_frequency": geometry,
            "summary": summary(analysis, "geometry")
        }),
    );
    visual.insert("token_sources".to_string(), json!([]));
    visual.insert(
        "density".to_string(),
        json!({
            "evidence_status": "unknown",
            "notes": "Static source analysis does not yet establish visual density intent."
        }),
    );

    let mut rules = Vec::new();
    for (domain, label) in [
        ("color", "color vocabulary"),
        ("typography", "font-size vocabulary"),
        ("spacing", "spacing vocabulary"),
        ("geometry", "radius vocabulary"),
    ] {
        let domain_summary = &analysis["domains"][domain]["summary"];
        let occurrences = domain_summary["occurrences"].as_u64().unwrap_or(0);
        let unique = domain_summary["unique_values"].as_u64().unwrap_or(0);
        if occurrences > 0 {
            rules.push(observation_rule(
                &format!("observed.{domain}.vocabulary"),
                format!(
                    "Static analysis observed {occurrences} occurrences across {unique} unique values in the project's {label}. Treat this as evidence to review, not as an automatically approved design constraint."
                ),
            ));
        }
    }

    let token_defined = analysis["domains"]["tokens"]["summary"]["defined"]
        .as_u64()
        .unwrap_or(0);
    let token_referenced = analysis["domains"]["tokens"]["summary"]["referenced"]
        .as_u64()
        .unwrap_or(0);
    if token_defined > 0 || token_referenced > 0 {
        rules.push(observation_rule(
            "observed.tokens.css-custom-properties",
            format!(
                "Static analysis observed {token_defined} CSS custom-property definitions and {token_referenced} referenced custom properties. Review which of these are intentional semantic design tokens before promotion."
            ),
        ));
    }

    let not_checked = analysis["checks"]["not_checked"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let findings = analysis["findings"].as_array().cloned().unwrap_or_default();

    Ok(json!({
        "format_version": 1,
        "id": "design-genome.candidate.static-analysis",
        "version": "0.1.0-candidate",
        "status": "candidate",
        "identity": {
            "principles": [
                "Preserve observed project evidence while it is being reviewed; do not infer brand intent from frequency alone."
            ],
            "anti_patterns": [
                "Do not promote automatically observed values to required design rules without human review.",
                "Do not claim runtime, accessibility, responsive, originality, or identity conclusions that were not measured."
            ]
        },
        "visual": Value::Object(visual),
        "components": [],
        "rules": rules,
        "examples": [],
        "sources": [
            {
                "id": "analysis.static",
                "type": "analysis",
                "uri": analysis_uri,
                "notes": "Deterministically generated from Design Analysis format v1."
            }
        ],
        "metadata": {
            "generator": "agentic-harness-cli",
            "generator_version": env!("CARGO_PKG_VERSION"),
            "deterministic": true,
            "review_required": true,
            "evidence_model": {
                "observed": "Directly carried from deterministic analysis measurements.",
                "inferred": "No inferred identity rules are generated in this command.",
                "unknown": "Intent or behavior not established by the supplied analysis."
            },
            "tokens": tokens,
            "analysis_findings": findings,
            "not_checked": not_checked,
            "review_queue": [
                "Confirm which observed values are intentional design-system vocabulary versus incidental literals.",
                "Define identity purpose, audience, personality, differentiation, and voice from human/project evidence.",
                "Review and approve semantic tokens before turning observations into constraints.",
                "Add approved component contracts and composition rules from project evidence.",
                "Run missing runtime/visual checks before making claims they would support."
            ]
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis_fixture() -> Value {
        json!({
            "format_version": 1,
            "source": {"kind": "repository", "uri": "."},
            "domains": {
                "color": {
                    "measurements": [{"value": [{"value": "#ff5500", "count": 3, "evidence": [{"path": "app.css"}]}]}],
                    "summary": {"occurrences": 3, "unique_values": 1}
                },
                "typography": {
                    "measurements": [{"value": [{"value": "14px", "count": 2}]}],
                    "summary": {"occurrences": 2, "unique_values": 1}
                },
                "spacing": {
                    "measurements": [{"value": [{"value": "16px", "count": 4}]}],
                    "summary": {"occurrences": 4, "unique_values": 1}
                },
                "geometry": {
                    "measurements": [{"value": [{"value": "4px", "count": 1}]}],
                    "summary": {"occurrences": 1, "unique_values": 1}
                },
                "tokens": {
                    "measurements": [{"value": {"defined": ["--color-accent"], "referenced": ["--color-accent"]}}],
                    "summary": {"defined": 1, "referenced": 1}
                }
            },
            "findings": [{"id": "color.hex-values"}],
            "checks": {"performed": ["static.hex-colors"], "not_checked": ["runtime.contrast"]}
        })
    }

    #[test]
    fn candidate_is_deterministic_and_never_auto_approved() {
        let analysis = analysis_fixture();
        let first = candidate_from_analysis(&analysis, "analysis.json").unwrap();
        let second = candidate_from_analysis(&analysis, "analysis.json").unwrap();
        assert_eq!(first, second);
        assert_eq!(first["status"], "candidate");
        assert_eq!(first["metadata"]["review_required"], true);
        assert_eq!(first["visual"]["color"]["evidence_status"], "observed");
        assert_eq!(first["visual"]["density"]["evidence_status"], "unknown");
        assert!(
            first["rules"]
                .as_array()
                .unwrap()
                .iter()
                .all(|rule| rule["importance"] != "required")
        );
    }

    #[test]
    fn rejects_unknown_analysis_format() {
        let mut analysis = analysis_fixture();
        analysis["format_version"] = json!(2);
        assert!(candidate_from_analysis(&analysis, "analysis.json").is_err());
    }
}
