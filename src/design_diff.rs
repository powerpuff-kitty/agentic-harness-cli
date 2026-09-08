use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

fn frequency_map(domain: Option<&Value>) -> BTreeMap<String, i64> {
    let mut result = BTreeMap::new();
    let Some(measurements) = domain
        .and_then(|value| value.get("measurements"))
        .and_then(Value::as_array)
    else {
        return result;
    };

    for measurement in measurements {
        let Some(rows) = measurement.get("value").and_then(Value::as_array) else {
            continue;
        };
        for row in rows {
            let Some(value) = row.get("value").and_then(Value::as_str) else {
                continue;
            };
            let Some(count) = row.get("count").and_then(Value::as_i64) else {
                continue;
            };
            result.insert(value.to_string(), count);
        }
        if !result.is_empty() {
            break;
        }
    }
    result
}

fn finding_ids(analysis: &Value) -> BTreeSet<String> {
    analysis
        .get("findings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|finding| finding.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

fn check_set(analysis: &Value, key: &str) -> BTreeSet<String> {
    analysis
        .get("checks")
        .and_then(|checks| checks.get(key))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn source_summary(analysis: &Value) -> Value {
    let source = analysis.get("source").cloned().unwrap_or_else(|| json!({}));
    json!({
        "source": source,
        "analysis_id": analysis.get("analysis_id").cloned().unwrap_or(Value::Null),
        "generated_at": analysis.get("generated_at").cloned().unwrap_or(Value::Null)
    })
}

pub fn validate_analysis(analysis: &Value) -> Result<(), String> {
    if analysis.get("format_version").and_then(Value::as_i64) != Some(1) {
        return Err("expected Design Analysis format_version 1".to_string());
    }
    if !analysis.get("domains").is_some_and(Value::is_object) {
        return Err("analysis artifact is missing domains".to_string());
    }
    if !analysis.get("findings").is_some_and(Value::is_array) {
        return Err("analysis artifact is missing findings".to_string());
    }
    if !analysis.get("checks").is_some_and(Value::is_object) {
        return Err("analysis artifact is missing checks".to_string());
    }
    Ok(())
}

pub fn diff_analysis(before: &Value, after: &Value) -> Result<Value, String> {
    validate_analysis(before)?;
    validate_analysis(after)?;

    let before_domains = before.get("domains").and_then(Value::as_object).expect("validated");
    let after_domains = after.get("domains").and_then(Value::as_object).expect("validated");
    let domain_names = before_domains
        .keys()
        .chain(after_domains.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut domains = Map::new();
    let mut observations = Vec::new();

    for domain in domain_names {
        let before_values = frequency_map(before_domains.get(&domain));
        let after_values = frequency_map(after_domains.get(&domain));
        let value_names = before_values
            .keys()
            .chain(after_values.keys())
            .cloned()
            .collect::<BTreeSet<_>>();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        for value in value_names {
            match (before_values.get(&value), after_values.get(&value)) {
                (None, Some(after_count)) => added.push(json!({"value": value, "count": after_count})),
                (Some(before_count), None) => removed.push(json!({"value": value, "count": before_count})),
                (Some(before_count), Some(after_count)) if before_count != after_count => changed.push(json!({
                    "value": value,
                    "before": before_count,
                    "after": after_count,
                    "delta": after_count - before_count
                })),
                _ => {}
            }
        }

        if !added.is_empty() {
            observations.push(json!({
                "id": format!("drift.{domain}.added-values"),
                "domain": domain,
                "classification": "observation",
                "severity": "info",
                "statement": format!("{} previously unseen measured value(s) appeared in the {domain} domain.", added.len()),
                "source_type": "static",
                "confidence": 1.0
            }));
        }

        domains.insert(
            domain,
            json!({
                "added_values": added,
                "removed_values": removed,
                "changed_counts": changed,
                "before_unique_values": before_values.len(),
                "after_unique_values": after_values.len()
            }),
        );
    }

    let before_findings = finding_ids(before);
    let after_findings = finding_ids(after);
    let added_findings = after_findings.difference(&before_findings).cloned().collect::<Vec<_>>();
    let removed_findings = before_findings.difference(&after_findings).cloned().collect::<Vec<_>>();

    let before_performed = check_set(before, "performed");
    let after_performed = check_set(after, "performed");
    let before_not_checked = check_set(before, "not_checked");
    let after_not_checked = check_set(after, "not_checked");

    Ok(json!({
        "format_version": 1,
        "kind": "design-analysis-diff",
        "before": source_summary(before),
        "after": source_summary(after),
        "domains": domains,
        "finding_changes": {
            "added": added_findings,
            "removed": removed_findings
        },
        "check_changes": {
            "newly_performed": after_performed.difference(&before_performed).cloned().collect::<Vec<_>>(),
            "no_longer_performed": before_performed.difference(&after_performed).cloned().collect::<Vec<_>>(),
            "newly_unchecked": after_not_checked.difference(&before_not_checked).cloned().collect::<Vec<_>>(),
            "no_longer_unchecked": before_not_checked.difference(&after_not_checked).cloned().collect::<Vec<_>>()
        },
        "observations": observations,
        "metadata": {
            "deterministic": true,
            "quality_score": null,
            "note": "New or changed values are drift evidence, not an automatic quality or originality judgment."
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis(rows: Value, findings: &[&str]) -> Value {
        json!({
            "format_version": 1,
            "source": {"kind": "repository", "revision": "fixture"},
            "domains": {
                "color": {
                    "measurements": [{"id": "color.hex-frequency", "metric": "hex-color-frequency", "value": rows, "source_type": "static"}],
                    "summary": {}
                }
            },
            "findings": findings.iter().map(|id| json!({"id": id})).collect::<Vec<_>>(),
            "checks": {"performed": ["static.hex-colors"], "not_checked": ["runtime.contrast"]}
        })
    }

    #[test]
    fn reports_new_removed_and_changed_values_deterministically() {
        let before = analysis(json!([
            {"value": "#111111", "count": 4},
            {"value": "#ffffff", "count": 2}
        ]), &["old"]);
        let after = analysis(json!([
            {"value": "#111111", "count": 6},
            {"value": "#ff5500", "count": 1}
        ]), &["new"]);

        let first = diff_analysis(&before, &after).unwrap();
        let second = diff_analysis(&before, &after).unwrap();
        assert_eq!(first, second);
        assert_eq!(first["domains"]["color"]["added_values"][0]["value"], "#ff5500");
        assert_eq!(first["domains"]["color"]["removed_values"][0]["value"], "#ffffff");
        assert_eq!(first["domains"]["color"]["changed_counts"][0]["delta"], 2);
        assert_eq!(first["finding_changes"]["added"][0], "new");
        assert_eq!(first["finding_changes"]["removed"][0], "old");
    }

    #[test]
    fn rejects_non_v1_artifacts() {
        let invalid = json!({"format_version": 2});
        assert!(diff_analysis(&invalid, &invalid).is_err());
    }
}
