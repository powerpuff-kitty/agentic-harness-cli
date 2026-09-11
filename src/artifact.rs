use serde_json::Value;
pub fn score(value: &Value) -> bool {
    value.is_null()
        || value
            .as_f64()
            .is_some_and(|n| n.is_finite() && (0.0..=100.0).contains(&n))
}
pub fn validate(value: &Value) -> Result<(), String> {
    let legacy = value.get("format_version").is_none();
    if !legacy && (value["format_version"] != 2 || value["kind"] != "codebase-audit") {
        return Err("expected Codebase Audit v2 or a complete legacy audit".into());
    }
    if value.get("overall").is_none() || !score(&value["overall"]) {
        return Err("audit overall must be a score or explicit null".into());
    }
    let scores = value["scores"]
        .as_object()
        .ok_or("audit scores must be an object")?;
    if scores.is_empty() || scores.values().any(|v| !score(v)) {
        return Err("audit scores must contain bounded numeric or explicit null values".into());
    }
    if !value["findings"].is_array()
        || !value["checks"]["performed"].is_array()
        || !value["checks"]["not_checked"].is_array()
    {
        return Err("audit requires findings and performed/not_checked arrays".into());
    }
    for key in ["performed", "not_checked"] {
        if !value["checks"][key]
            .as_array()
            .unwrap()
            .iter()
            .all(Value::is_string)
        {
            return Err(format!("checks.{key} must contain strings"));
        }
    }
    for finding in value["findings"].as_array().unwrap() {
        if finding["severity"]
            .as_str()
            .is_none_or(|s| !["critical", "high", "medium", "low", "info"].contains(&s))
            || !finding["dimension"].is_string()
            || !finding["message"].is_string()
        {
            return Err("invalid audit finding".into());
        }
        if let Some(evidence) = finding.get("evidence")
            && evidence
                .as_array()
                .is_none_or(|items| items.iter().any(|v| !v.is_string() && !v.is_object()))
        {
            return Err("finding evidence must contain strings or objects".into());
        }
    }
    if let Some(maturity) = value.get("target_maturity")
        && maturity.as_str().is_none_or(|s| {
            ![
                "prototype",
                "startup",
                "production",
                "critical",
                "beta",
                "unknown",
            ]
            .contains(&s)
        })
    {
        return Err("invalid target maturity".into());
    }
    if legacy || value.get("readiness").is_some() {
        if !value["readiness"].is_object()
            || ["prototype", "startup", "production", "critical"]
                .iter()
                .any(|k| value["readiness"].get(k).is_none() || !score(&value["readiness"][k]))
        {
            return Err("readiness requires four bounded numeric or explicit null values".into());
        }
        if legacy
            && (value["overall"].is_null()
                || ["prototype", "startup", "production", "critical"]
                    .iter()
                    .any(|k| value["readiness"][k].is_null()))
        {
            return Err("legacy scores/readiness must follow the numeric legacy contract".into());
        }
    }
    if let Some(scan) = value.get("scan")
        && (!scan["complete"].is_boolean()
            || scan["errors"]
                .as_array()
                .is_none_or(|a| a.iter().any(|v| !v.is_string())))
    {
        return Err("invalid scan coverage".into());
    }
    if value.get("architecture").is_some()
        && (!value["architecture"]["compliance"]["deterministic_errors"].is_u64()
            || !value["architecture"]["compliance"]["passed"].is_boolean()
            || value["architecture"]["compliance"]
                .get("complete")
                .is_some_and(|v| !v.is_boolean()))
    {
        return Err("invalid architecture compliance data".into());
    }
    Ok(())
}
pub fn threshold(text: &str) -> Result<f64, String> {
    let n = text
        .parse::<f64>()
        .map_err(|_| "threshold must be numeric")?;
    if !n.is_finite() || !(0.0..=100.0).contains(&n) {
        return Err("threshold must be finite and within 0..100".into());
    }
    Ok(n)
}
