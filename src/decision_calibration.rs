//! Offline empirical evaluation, calibration and regression gating for Decision Kernel receipts.

use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

const EPSILON: f64 = 1e-15;

#[derive(Debug, Clone)]
pub(crate) struct CalibrationOptions {
    pub bins: usize,
    pub target_accuracy: Option<f64>,
    pub minimum_coverage: Option<f64>,
    pub minimum_samples: usize,
    pub generated_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RegressionBudgets {
    pub max_accuracy_drop: f64,
    pub max_coverage_drop: f64,
    pub max_brier_increase: f64,
    pub max_ece_increase: f64,
    pub max_ordinal_mae_increase: f64,
    pub max_mean_latency_increase_ms: Option<f64>,
    pub max_total_cost_increase_usd: Option<f64>,
    pub generated_at: String,
}

fn object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("decisions: {context} must be an object"))
}

fn array<'a>(value: &'a Value, context: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("decisions: {context} must be an array"))
}

fn text<'a>(value: &'a Value, context: &str) -> Result<&'a str, String> {
    let value = value
        .as_str()
        .ok_or_else(|| format!("decisions: {context} must be a string"))?;
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(format!("decisions: invalid {context}"));
    }
    Ok(value)
}

fn positive_u64(value: &Value, context: &str) -> Result<u64, String> {
    let value = value
        .as_u64()
        .ok_or_else(|| format!("decisions: {context} must be a positive integer"))?;
    if value == 0 {
        return Err(format!("decisions: {context} must be a positive integer"));
    }
    Ok(value)
}

fn finite_ratio(value: f64, context: &str) -> Result<f64, String> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(format!("decisions: {context} must be within [0,1]"));
    }
    Ok(value)
}

fn optional_metric(value: &Value, context: &str) -> Result<Option<f64>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let value = value
        .as_f64()
        .ok_or_else(|| format!("decisions: {context} must be numeric or null"))?;
    if !value.is_finite() {
        return Err(format!("decisions: {context} must be finite"));
    }
    Ok(Some(value))
}

fn provider_identity(
    receipt: &Value,
) -> Result<(String, String, Option<String>, Option<String>), String> {
    let provider = object(
        receipt
            .get("provider")
            .ok_or("decisions: evaluation receipt.provider is required")?,
        "evaluation receipt.provider",
    )?;
    Ok((
        text(
            provider
                .get("type")
                .ok_or("decisions: evaluation receipt.provider.type is required")?,
            "evaluation receipt.provider.type",
        )?
        .to_string(),
        text(
            provider
                .get("id")
                .ok_or("decisions: evaluation receipt.provider.id is required")?,
            "evaluation receipt.provider.id",
        )?
        .to_string(),
        provider
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        provider
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string),
    ))
}

fn same_version(left: &Value, right: &Value) -> bool {
    left == right
}

pub(crate) fn validate_dataset(value: &Value) -> Result<(), String> {
    let root = object(value, "decision-eval-dataset")?;
    if root.get("format_version").and_then(Value::as_u64) != Some(1)
        || root.get("kind").and_then(Value::as_str) != Some("decision-eval-dataset")
    {
        return Err("decisions: unsupported decision-eval-dataset".into());
    }
    text(
        root.get("id")
            .ok_or("decisions: decision-eval-dataset.id is required")?,
        "decision-eval-dataset.id",
    )?;
    positive_u64(
        root.get("revision")
            .ok_or("decisions: decision-eval-dataset.revision is required")?,
        "decision-eval-dataset.revision",
    )?;
    let split = text(
        root.get("split")
            .ok_or("decisions: decision-eval-dataset.split is required")?,
        "decision-eval-dataset.split",
    )?;
    if !["training", "calibration", "validation", "test"].contains(&split) {
        return Err("decisions: unsupported evaluation dataset split".into());
    }

    let decision = object(
        root.get("decision")
            .ok_or("decisions: decision-eval-dataset.decision is required")?,
        "decision-eval-dataset.decision",
    )?;
    let spec_id = text(
        decision
            .get("spec_id")
            .ok_or("decisions: dataset decision.spec_id is required")?,
        "dataset decision.spec_id",
    )?;
    let spec_revision = positive_u64(
        decision
            .get("spec_revision")
            .ok_or("decisions: dataset decision.spec_revision is required")?,
        "dataset decision.spec_revision",
    )?;
    let decision_kind = text(
        decision
            .get("decision_kind")
            .ok_or("decisions: dataset decision.decision_kind is required")?,
        "dataset decision.decision_kind",
    )?;
    if !["boolean", "choice", "ordinal"].contains(&decision_kind) {
        return Err("decisions: evaluation datasets support boolean/choice/ordinal only".into());
    }
    let options = decision
        .get("options")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| text(value, "dataset decision option").map(str::to_string))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let levels = decision
        .get("levels")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| text(value, "dataset decision level").map(str::to_string))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    if decision_kind == "choice" && options.len() < 2 {
        return Err("decisions: choice evaluation dataset requires options".into());
    }
    if decision_kind == "ordinal" && levels.len() < 2 {
        return Err("decisions: ordinal evaluation dataset requires levels".into());
    }

    let state_schema = object(
        root.get("state_schema")
            .ok_or("decisions: dataset state_schema is required")?,
        "dataset state_schema",
    )?;
    let state_schema_id = text(
        state_schema
            .get("id")
            .ok_or("decisions: dataset state_schema.id is required")?,
        "dataset state_schema.id",
    )?;
    let state_schema_version = state_schema
        .get("version")
        .ok_or("decisions: dataset state_schema.version is required")?;

    let cases = array(
        root.get("cases")
            .ok_or("decisions: dataset cases are required")?,
        "dataset cases",
    )?;
    if cases.is_empty() {
        return Err("decisions: evaluation dataset must contain at least one case".into());
    }

    let mut case_ids = BTreeSet::new();
    let mut receipt_ids = BTreeSet::new();
    let mut first_provider: Option<(String, String, Option<String>, Option<String>)> = None;

    for case in cases {
        let case = object(case, "evaluation case")?;
        let case_id = text(
            case.get("id")
                .ok_or("decisions: evaluation case.id is required")?,
            "evaluation case.id",
        )?;
        if !case_ids.insert(case_id.to_string()) {
            return Err(format!(
                "decisions: duplicate evaluation case id: {case_id}"
            ));
        }

        let receipt = case
            .get("receipt")
            .ok_or("decisions: evaluation case.receipt is required")?;
        crate::decisions::validate_receipt(receipt)?;
        let receipt_id = text(
            receipt
                .get("id")
                .ok_or("decisions: evaluation receipt.id is required")?,
            "evaluation receipt.id",
        )?;
        if !receipt_ids.insert(receipt_id.to_string()) {
            return Err(format!(
                "decisions: duplicate receipt in evaluation dataset: {receipt_id}"
            ));
        }
        if receipt["spec"]["id"].as_str() != Some(spec_id)
            || receipt["spec"]["revision"].as_u64() != Some(spec_revision)
        {
            return Err(format!(
                "decisions: evaluation case {case_id} receipt spec does not match dataset"
            ));
        }
        if receipt["state"]["schema_id"].as_str() != Some(state_schema_id)
            || !same_version(&receipt["state"]["schema_version"], state_schema_version)
        {
            return Err(format!(
                "decisions: evaluation case {case_id} state schema does not match dataset"
            ));
        }

        let identity = provider_identity(receipt)?;
        if let Some(expected) = &first_provider {
            if expected != &identity {
                return Err(
                    "decisions: evaluation dataset must use one exact provider/model/version identity"
                        .into(),
                );
            }
        } else {
            first_provider = Some(identity);
        }

        let expected = object(
            case.get("expected")
                .ok_or("decisions: evaluation case.expected is required")?,
            "evaluation case.expected",
        )?;
        match decision_kind {
            "boolean" => {
                if !expected.get("value").is_some_and(Value::is_boolean) {
                    return Err(format!(
                        "decisions: boolean case {case_id} expected.value must be boolean"
                    ));
                }
            }
            "choice" => {
                let expected_value =
                    expected
                        .get("value")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            format!(
                                "decisions: choice case {case_id} expected.value must be a string"
                            )
                        })?;
                if !options.iter().any(|option| option == expected_value) {
                    return Err(format!(
                        "decisions: choice case {case_id} expected.value is not declared"
                    ));
                }
            }
            "ordinal" => {
                let index = expected
                    .get("ordinal_index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        format!(
                            "decisions: ordinal case {case_id} expected.ordinal_index is required"
                        )
                    })? as usize;
                if index >= levels.len() {
                    return Err(format!(
                        "decisions: ordinal case {case_id} expected.ordinal_index is out of range"
                    ));
                }
            }
            _ => unreachable!(),
        }

        let truth = object(
            case.get("truth")
                .ok_or("decisions: evaluation case.truth is required")?,
            "evaluation case.truth",
        )?;
        let verification = text(
            truth
                .get("verification_type")
                .ok_or("decisions: evaluation truth.verification_type is required")?,
            "evaluation truth.verification_type",
        )?;
        if ![
            "human",
            "deterministic",
            "external-authority",
            "measurement",
        ]
        .contains(&verification)
        {
            return Err(format!(
                "decisions: evaluation case {case_id} truth must be independently verified"
            ));
        }
        text(
            truth
                .get("observed_at")
                .ok_or("decisions: evaluation truth.observed_at is required")?,
            "evaluation truth.observed_at",
        )?;
        if let Some(cost) = case.get("cost_usd").filter(|value| !value.is_null()) {
            let cost = cost
                .as_f64()
                .ok_or("decisions: cost_usd must be numeric or null")?;
            if !cost.is_finite() || cost < 0.0 {
                return Err("decisions: cost_usd must be non-negative and finite".into());
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_calibration(value: &Value) -> Result<(), String> {
    let root = object(value, "decision-calibration")?;
    if root.get("format_version").and_then(Value::as_u64) != Some(1)
        || root.get("kind").and_then(Value::as_str) != Some("decision-calibration")
    {
        return Err("decisions: unsupported decision-calibration".into());
    }
    let dataset = object(
        root.get("dataset")
            .ok_or("decisions: calibration dataset identity is required")?,
        "calibration dataset",
    )?;
    let split = text(
        dataset
            .get("split")
            .ok_or("decisions: calibration dataset split is required")?,
        "calibration dataset split",
    )?;
    let metrics = object(
        root.get("metrics")
            .ok_or("decisions: calibration metrics are required")?,
        "calibration metrics",
    )?;
    let total = positive_u64(
        metrics
            .get("total")
            .ok_or("decisions: calibration metrics.total is required")?,
        "calibration metrics.total",
    )?;
    let produced = metrics
        .get("produced")
        .and_then(Value::as_u64)
        .ok_or("decisions: calibration produced must be unsigned")?;
    let abstained = metrics
        .get("abstained")
        .and_then(Value::as_u64)
        .ok_or("decisions: calibration abstained must be unsigned")?;
    let failed = metrics
        .get("failed")
        .and_then(Value::as_u64)
        .ok_or("decisions: calibration failed must be unsigned")?;
    let correct = metrics
        .get("correct")
        .and_then(Value::as_u64)
        .ok_or("decisions: calibration correct must be unsigned")?;
    if produced + abstained + failed != total {
        return Err("decisions: calibration coverage counts do not sum to total".into());
    }
    let coverage = metrics["coverage"]
        .as_f64()
        .ok_or("decisions: calibration coverage must be numeric")?;
    finite_ratio(coverage, "calibration coverage")?;
    if (coverage - produced as f64 / total as f64).abs() > 1e-9 {
        return Err("decisions: calibration coverage arithmetic mismatch".into());
    }
    match metrics.get("accuracy") {
        Some(Value::Null) if produced == 0 => {}
        Some(value) if produced > 0 => {
            let accuracy = value
                .as_f64()
                .ok_or("decisions: calibration accuracy must be numeric or null")?;
            finite_ratio(accuracy, "calibration accuracy")?;
            if (accuracy - correct as f64 / produced as f64).abs() > 1e-9 {
                return Err("decisions: calibration accuracy arithmetic mismatch".into());
            }
        }
        _ => return Err("decisions: calibration accuracy presence is inconsistent".into()),
    }
    for (field, count) in [("abstention_rate", abstained), ("failure_rate", failed)] {
        let value = metrics[field]
            .as_f64()
            .ok_or_else(|| format!("decisions: calibration {field} must be numeric"))?;
        finite_ratio(value, &format!("calibration {field}"))?;
        if (value - count as f64 / total as f64).abs() > 1e-9 {
            return Err(format!(
                "decisions: calibration {field} arithmetic mismatch"
            ));
        }
    }

    let reliability = object(
        root.get("reliability")
            .ok_or("decisions: calibration reliability is required")?,
        "calibration reliability",
    )?;
    let bin_count = reliability["bin_count"]
        .as_u64()
        .ok_or("decisions: reliability bin_count must be unsigned")? as usize;
    if !(2..=100).contains(&bin_count) {
        return Err("decisions: reliability bin_count must be 2..=100".into());
    }
    let bins = array(
        reliability
            .get("bins")
            .ok_or("decisions: reliability bins are required")?,
        "reliability bins",
    )?;
    if bins.len() != bin_count {
        return Err("decisions: reliability bin count does not match bins length".into());
    }

    let threshold = object(
        root.get("threshold")
            .ok_or("decisions: calibration threshold evidence is required")?,
        "calibration threshold",
    )?;
    let tuning_allowed = threshold["tuning_allowed"]
        .as_bool()
        .ok_or("decisions: threshold.tuning_allowed must be boolean")?;
    if split != "calibration" && tuning_allowed {
        return Err("decisions: threshold tuning is only allowed on calibration split".into());
    }
    if let Some(selected) = threshold.get("selected").filter(|value| !value.is_null()) {
        if !tuning_allowed {
            return Err("decisions: selected threshold requires tuning_allowed".into());
        }
        let selected = object(selected, "selected threshold")?;
        let accepted = selected["accepted"]
            .as_u64()
            .ok_or("decisions: selected threshold accepted must be unsigned")?;
        let selected_coverage = selected["coverage"]
            .as_f64()
            .ok_or("decisions: selected threshold coverage must be numeric")?;
        if accepted > total || (selected_coverage - accepted as f64 / total as f64).abs() > 1e-9 {
            return Err("decisions: selected threshold coverage arithmetic mismatch".into());
        }
    }
    if root.get("side_effects").and_then(Value::as_bool) != Some(false)
        || root.get("consequence_authorized").and_then(Value::as_bool) != Some(false)
    {
        return Err("decisions: calibration artifacts are side-effect free/non-authorizing".into());
    }
    Ok(())
}

pub(crate) fn validate_regression(value: &Value) -> Result<(), String> {
    let root = object(value, "decision-regression")?;
    if root.get("format_version").and_then(Value::as_u64) != Some(1)
        || root.get("kind").and_then(Value::as_str) != Some("decision-regression")
    {
        return Err("decisions: unsupported decision-regression".into());
    }
    let failures = array(
        root.get("failures")
            .ok_or("decisions: regression failures are required")?,
        "regression failures",
    )?;
    let passed = root
        .get("passed")
        .and_then(Value::as_bool)
        .ok_or("decisions: regression passed must be boolean")?;
    if passed != failures.is_empty() {
        return Err("decisions: regression passed must match failure list".into());
    }
    if root.get("side_effects").and_then(Value::as_bool) != Some(false)
        || root.get("consequence_authorized").and_then(Value::as_bool) != Some(false)
    {
        return Err("decisions: regression artifacts are side-effect free/non-authorizing".into());
    }
    Ok(())
}

fn expected_label(
    decision_kind: &str,
    expected: &Map<String, Value>,
    options: &[String],
) -> Result<String, String> {
    match decision_kind {
        "boolean" => Ok(if expected["value"].as_bool().unwrap() {
            "true".to_string()
        } else {
            "false".to_string()
        }),
        "choice" => Ok(expected["value"].as_str().unwrap().to_string()),
        "ordinal" => Ok(expected["ordinal_index"].as_u64().unwrap().to_string()),
        _ => {
            let _ = options;
            Err("decisions: unsupported evaluation decision kind".into())
        }
    }
}

fn produced_correct(
    decision_kind: &str,
    receipt: &Value,
    expected: &Map<String, Value>,
    options: &[String],
    levels: &[String],
) -> Result<(bool, Option<f64>), String> {
    let result = object(
        receipt
            .get("result")
            .ok_or("decisions: produced receipt result is required")?,
        "produced receipt result",
    )?;
    match decision_kind {
        "boolean" => {
            let predicted = result
                .get("value")
                .and_then(Value::as_bool)
                .ok_or("decisions: boolean receipt result.value must be boolean")?;
            Ok((predicted == expected["value"].as_bool().unwrap(), None))
        }
        "choice" => {
            let predicted = result
                .get("value")
                .and_then(Value::as_str)
                .ok_or("decisions: choice receipt result.value must be string")?;
            if !options.iter().any(|option| option == predicted) {
                return Err("decisions: choice receipt selected undeclared option".into());
            }
            Ok((predicted == expected["value"].as_str().unwrap(), None))
        }
        "ordinal" => {
            let expected_index = expected["ordinal_index"].as_u64().unwrap() as usize;
            let predicted_position = if let Some(distribution) =
                result.get("distribution").and_then(Value::as_object)
            {
                let mut expected_position = 0.0;
                for (label, probability) in distribution {
                    let index = label.parse::<usize>().map_err(|_| {
                        "decisions: ordinal distribution labels must be numeric level indexes"
                            .to_string()
                    })?;
                    if index >= levels.len() {
                        return Err(
                            "decisions: ordinal distribution level index is out of range".into(),
                        );
                    }
                    expected_position += index as f64
                        * probability
                            .as_f64()
                            .ok_or("decisions: ordinal distribution probability must be numeric")?;
                }
                expected_position
            } else if let Some(value) = result.get("value").and_then(Value::as_f64) {
                if !value.is_finite() {
                    return Err("decisions: ordinal result value must be finite".into());
                }
                value
            } else if let Some(level) = result.get("value").and_then(Value::as_str) {
                levels
                    .iter()
                    .position(|candidate| candidate == level)
                    .ok_or("decisions: ordinal result level is undeclared")? as f64
            } else {
                return Err("decisions: ordinal result requires numeric/string value".into());
            };
            let nearest = predicted_position.round();
            let correct = (nearest - expected_index as f64).abs() < 0.5;
            Ok((
                correct,
                Some((predicted_position - expected_index as f64).abs()),
            ))
        }
        _ => Err("decisions: unsupported evaluation decision kind".into()),
    }
}

fn distribution_scores(
    decision_kind: &str,
    receipt: &Value,
    expected: &Map<String, Value>,
    options: &[String],
    levels: &[String],
) -> Result<Option<(f64, f64)>, String> {
    let Some(distribution) = receipt
        .get("result")
        .and_then(|result| result.get("distribution"))
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };
    let expected_label = expected_label(decision_kind, expected, options)?;
    let probability_true = distribution
        .get(&expected_label)
        .and_then(Value::as_f64)
        .ok_or_else(|| {
            format!("decisions: distribution is missing expected label {expected_label}")
        })?
        .clamp(EPSILON, 1.0);

    let brier = if decision_kind == "boolean" {
        let p_true = distribution
            .get("true")
            .and_then(Value::as_f64)
            .ok_or("decisions: boolean distribution requires true probability")?;
        let target = if expected["value"].as_bool().unwrap() {
            1.0
        } else {
            0.0
        };
        (p_true - target).powi(2)
    } else {
        let labels: Vec<String> = if decision_kind == "choice" {
            options.to_vec()
        } else {
            (0..levels.len()).map(|index| index.to_string()).collect()
        };
        let mut sum = 0.0;
        for label in labels {
            let probability = distribution
                .get(&label)
                .and_then(Value::as_f64)
                .ok_or_else(|| format!("decisions: distribution missing label {label}"))?;
            let target = if label == expected_label { 1.0 } else { 0.0 };
            sum += (probability - target).powi(2);
        }
        sum
    };
    Ok(Some((brier, -probability_true.ln())))
}

fn calibration_id(dataset: &Value, generated_at: &str) -> String {
    let provider = &dataset["cases"][0]["receipt"]["provider"];
    let seed = json!({
        "dataset": dataset["id"],
        "revision": dataset["revision"],
        "provider": provider,
        "generated_at": generated_at
    });
    format!(
        "cal-{}",
        crate::check_inputs::hash(serde_json::to_vec(&seed).unwrap().as_slice())
            .trim_start_matches("sha256:")
    )
}

pub(crate) fn calibrate_dataset(
    dataset: &Value,
    options: &CalibrationOptions,
) -> Result<Value, String> {
    validate_dataset(dataset)?;
    if !(2..=100).contains(&options.bins) {
        return Err("decisions: --bins must be between 2 and 100".into());
    }
    if options.minimum_samples == 0 {
        return Err("decisions: --min-samples must be positive".into());
    }
    if options.generated_at.is_empty() || !options.generated_at.contains('T') {
        return Err("decisions: --generated-at must be an RFC3339 timestamp".into());
    }
    if let Some(value) = options.target_accuracy {
        finite_ratio(value, "target accuracy")?;
    }
    if let Some(value) = options.minimum_coverage {
        finite_ratio(value, "minimum coverage")?;
    }
    let split = dataset["split"].as_str().unwrap();
    let wants_tuning = options.target_accuracy.is_some() || options.minimum_coverage.is_some();
    if wants_tuning && split != "calibration" {
        return Err(
            "decisions: threshold tuning flags are only valid for calibration split".into(),
        );
    }
    if options.target_accuracy.is_some() != options.minimum_coverage.is_some() {
        return Err(
            "decisions: threshold tuning requires both --target-accuracy and --min-coverage".into(),
        );
    }

    let decision = dataset["decision"].as_object().unwrap();
    let decision_kind = decision["decision_kind"].as_str().unwrap();
    let options_list = decision
        .get("options")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let levels = decision
        .get("levels")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let cases = dataset["cases"].as_array().unwrap();
    let total = cases.len();
    let mut produced = 0_usize;
    let mut correct = 0_usize;
    let mut abstained = 0_usize;
    let mut failed = 0_usize;
    let mut brier_values = Vec::new();
    let mut log_losses = Vec::new();
    let mut ordinal_errors = Vec::new();
    let mut confidence_cases = Vec::<(f64, bool)>::new();
    let mut latencies = Vec::new();
    let mut all_latency = true;
    let mut costs = Vec::new();
    let mut all_cost = true;

    for case in cases {
        let case = case.as_object().unwrap();
        let receipt = &case["receipt"];
        let expected = case["expected"].as_object().unwrap();
        match receipt["status"].as_str().unwrap() {
            "produced" => {
                produced += 1;
                let (is_correct, ordinal_error) =
                    produced_correct(decision_kind, receipt, expected, &options_list, &levels)?;
                if is_correct {
                    correct += 1;
                }
                if let Some(error) = ordinal_error {
                    ordinal_errors.push(error);
                }
                if let Some((brier, log_loss)) =
                    distribution_scores(decision_kind, receipt, expected, &options_list, &levels)?
                {
                    brier_values.push(brier);
                    log_losses.push(log_loss);
                }
                if let Some(confidence) = receipt["uncertainty"]["provider_confidence"].as_f64() {
                    finite_ratio(confidence, "provider confidence")?;
                    confidence_cases.push((confidence, is_correct));
                }
            }
            "provider-failure" => failed += 1,
            _ => abstained += 1,
        }

        if let Some(latency) = receipt
            .get("timing")
            .and_then(|timing| timing.get("latency_ms"))
            .and_then(Value::as_f64)
        {
            if latency.is_finite() && latency >= 0.0 {
                latencies.push(latency);
            } else {
                return Err("decisions: receipt latency_ms must be non-negative finite".into());
            }
        } else {
            all_latency = false;
        }

        match case.get("cost_usd") {
            Some(Value::Number(number)) => {
                let cost = number
                    .as_f64()
                    .ok_or("decisions: cost_usd must be numeric")?;
                if !cost.is_finite() || cost < 0.0 {
                    return Err("decisions: cost_usd must be non-negative finite".into());
                }
                costs.push(cost);
            }
            _ => all_cost = false,
        }
    }

    let coverage = produced as f64 / total as f64;
    let accuracy = (produced > 0).then_some(correct as f64 / produced as f64);
    let mean = |values: &[f64]| {
        (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
    };

    let mut bins = Vec::with_capacity(options.bins);
    let mut ece = 0.0;
    for index in 0..options.bins {
        let lower = index as f64 / options.bins as f64;
        let upper = (index + 1) as f64 / options.bins as f64;
        let members = confidence_cases
            .iter()
            .filter(|(confidence, _)| {
                *confidence >= lower
                    && if index + 1 == options.bins {
                        *confidence <= upper
                    } else {
                        *confidence < upper
                    }
            })
            .collect::<Vec<_>>();
        let count = members.len();
        let mean_confidence = (!members.is_empty()).then(|| {
            members
                .iter()
                .map(|(confidence, _)| *confidence)
                .sum::<f64>()
                / count as f64
        });
        let bin_accuracy = (!members.is_empty())
            .then(|| members.iter().filter(|(_, correct)| *correct).count() as f64 / count as f64);
        if let (Some(mean_confidence), Some(bin_accuracy)) = (mean_confidence, bin_accuracy) {
            ece += count as f64 / confidence_cases.len() as f64
                * (mean_confidence - bin_accuracy).abs();
        }
        bins.push(json!({
            "lower": lower,
            "upper": upper,
            "count": count,
            "mean_confidence": mean_confidence,
            "accuracy": bin_accuracy
        }));
    }
    let ece = (!confidence_cases.is_empty()).then_some(ece);

    let tuning_allowed = split == "calibration";
    let selected_threshold = if let (Some(target_accuracy), Some(minimum_coverage)) =
        (options.target_accuracy, options.minimum_coverage)
    {
        let mut thresholds = confidence_cases
            .iter()
            .map(|(confidence, _)| *confidence)
            .collect::<Vec<_>>();
        thresholds.sort_by(f64::total_cmp);
        thresholds.dedup_by(|left, right| (*left - *right).abs() < f64::EPSILON);

        let mut selected = None;
        for threshold in thresholds {
            let accepted = confidence_cases
                .iter()
                .filter(|(confidence, _)| *confidence >= threshold)
                .collect::<Vec<_>>();
            if accepted.len() < options.minimum_samples {
                continue;
            }
            let threshold_coverage = accepted.len() as f64 / total as f64;
            if threshold_coverage + f64::EPSILON < minimum_coverage {
                continue;
            }
            let threshold_accuracy = accepted.iter().filter(|(_, correct)| *correct).count() as f64
                / accepted.len() as f64;
            if threshold_accuracy + f64::EPSILON >= target_accuracy {
                selected = Some(json!({
                    "minimum_provider_confidence": threshold,
                    "accepted": accepted.len(),
                    "coverage": threshold_coverage,
                    "accuracy": threshold_accuracy
                }));
                break;
            }
        }
        selected
    } else {
        None
    };

    let provider = dataset["cases"][0]["receipt"]["provider"].clone();
    let report = json!({
        "format_version": 1,
        "kind": "decision-calibration",
        "id": calibration_id(dataset, &options.generated_at),
        "dataset": {
            "id": dataset["id"],
            "revision": dataset["revision"],
            "split": dataset["split"]
        },
        "decision": {
            "spec_id": decision["spec_id"],
            "spec_revision": decision["spec_revision"],
            "decision_kind": decision["decision_kind"]
        },
        "state_schema": dataset["state_schema"],
        "provider": provider,
        "metrics": {
            "total": total,
            "produced": produced,
            "correct": correct,
            "abstained": abstained,
            "failed": failed,
            "coverage": coverage,
            "accuracy": accuracy,
            "abstention_rate": abstained as f64 / total as f64,
            "failure_rate": failed as f64 / total as f64,
            "brier_score": mean(&brier_values),
            "log_loss": mean(&log_losses),
            "expected_calibration_error": ece,
            "ordinal_mae": mean(&ordinal_errors),
            "mean_latency_ms": (all_latency && latencies.len() == total).then(|| latencies.iter().sum::<f64>() / total as f64),
            "total_cost_usd": (all_cost && costs.len() == total).then(|| costs.iter().sum::<f64>())
        },
        "reliability": {
            "bin_count": options.bins,
            "confidence_case_count": confidence_cases.len(),
            "bins": bins
        },
        "threshold": {
            "tuning_allowed": tuning_allowed,
            "target_accuracy": options.target_accuracy,
            "minimum_coverage": options.minimum_coverage,
            "minimum_samples": options.minimum_samples,
            "selected": selected_threshold,
            "rationale": if options.target_accuracy.is_some() {
                "lowest observed provider confidence satisfying target accuracy, minimum coverage and minimum sample count on calibration split"
            } else {
                "threshold tuning not requested"
            }
        },
        "generated_at": options.generated_at,
        "side_effects": false,
        "consequence_authorized": false,
        "notes": "Empirical calibration evidence only; no application authorization is granted."
    });
    validate_calibration(&report)?;
    Ok(report)
}

fn metric(report: &Value, name: &str) -> Result<Option<f64>, String> {
    optional_metric(
        report
            .get("metrics")
            .and_then(|metrics| metrics.get(name))
            .ok_or_else(|| format!("decisions: calibration metric {name} is missing"))?,
        &format!("calibration metric {name}"),
    )
}

fn delta(candidate: Option<f64>, baseline: Option<f64>) -> Option<f64> {
    match (candidate, baseline) {
        (Some(candidate), Some(baseline)) => Some(candidate - baseline),
        (None, None) => None,
        _ => None,
    }
}

fn regression_id(baseline: &Value, candidate: &Value, generated_at: &str) -> String {
    let seed = json!({
        "baseline": baseline["id"],
        "candidate": candidate["id"],
        "generated_at": generated_at
    });
    format!(
        "reg-{}",
        crate::check_inputs::hash(serde_json::to_vec(&seed).unwrap().as_slice())
            .trim_start_matches("sha256:")
    )
}

pub(crate) fn compare_calibrations(
    baseline: &Value,
    candidate: &Value,
    budgets: &RegressionBudgets,
) -> Result<Value, String> {
    validate_calibration(baseline)?;
    validate_calibration(candidate)?;
    if baseline["dataset"] != candidate["dataset"] {
        return Err(
            "decisions: calibration regression requires the exact same dataset id/revision/split"
                .into(),
        );
    }
    if baseline["decision"] != candidate["decision"]
        || baseline["state_schema"] != candidate["state_schema"]
    {
        return Err(
            "decisions: calibration regression requires the same decision/state schema".into(),
        );
    }
    for (value, name) in [
        (budgets.max_accuracy_drop, "max accuracy drop"),
        (budgets.max_coverage_drop, "max coverage drop"),
        (budgets.max_brier_increase, "max Brier increase"),
        (budgets.max_ece_increase, "max ECE increase"),
        (budgets.max_ordinal_mae_increase, "max ordinal MAE increase"),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(format!("decisions: {name} must be non-negative finite"));
        }
    }
    for (value, name) in [
        (
            budgets.max_mean_latency_increase_ms,
            "max mean latency increase",
        ),
        (
            budgets.max_total_cost_increase_usd,
            "max total cost increase",
        ),
    ] {
        if let Some(value) = value.filter(|value| !value.is_finite() || *value < 0.0) {
            return Err(format!(
                "decisions: {name} must be non-negative finite, got {value}"
            ));
        }
    }
    if budgets.generated_at.is_empty() || !budgets.generated_at.contains('T') {
        return Err("decisions: --generated-at must be an RFC3339 timestamp".into());
    }

    let baseline_accuracy = metric(baseline, "accuracy")?;
    let candidate_accuracy = metric(candidate, "accuracy")?;
    let baseline_coverage = metric(baseline, "coverage")?.unwrap();
    let candidate_coverage = metric(candidate, "coverage")?.unwrap();
    let baseline_brier = metric(baseline, "brier_score")?;
    let candidate_brier = metric(candidate, "brier_score")?;
    let baseline_ece = metric(baseline, "expected_calibration_error")?;
    let candidate_ece = metric(candidate, "expected_calibration_error")?;
    let baseline_ordinal = metric(baseline, "ordinal_mae")?;
    let candidate_ordinal = metric(candidate, "ordinal_mae")?;
    let baseline_latency = metric(baseline, "mean_latency_ms")?;
    let candidate_latency = metric(candidate, "mean_latency_ms")?;
    let baseline_cost = metric(baseline, "total_cost_usd")?;
    let candidate_cost = metric(candidate, "total_cost_usd")?;

    let accuracy_delta = delta(candidate_accuracy, baseline_accuracy);
    let coverage_delta = candidate_coverage - baseline_coverage;
    let brier_delta = delta(candidate_brier, baseline_brier);
    let ece_delta = delta(candidate_ece, baseline_ece);
    let ordinal_delta = delta(candidate_ordinal, baseline_ordinal);
    let latency_delta = delta(candidate_latency, baseline_latency);
    let cost_delta = delta(candidate_cost, baseline_cost);

    let mut failures = Vec::new();
    match (baseline_accuracy, candidate_accuracy) {
        (Some(base), Some(candidate))
            if base - candidate > budgets.max_accuracy_drop + f64::EPSILON =>
        {
            failures.push(format!(
                "accuracy drop {:.6} exceeds budget {:.6}",
                base - candidate,
                budgets.max_accuracy_drop
            ));
        }
        (Some(_), None) => failures.push("candidate accuracy is unavailable".into()),
        (None, Some(_)) => failures.push("baseline accuracy is unavailable".into()),
        _ => {}
    }
    if baseline_coverage - candidate_coverage > budgets.max_coverage_drop + f64::EPSILON {
        failures.push(format!(
            "coverage drop {:.6} exceeds budget {:.6}",
            baseline_coverage - candidate_coverage,
            budgets.max_coverage_drop
        ));
    }

    for (name, base, candidate, budget) in [
        (
            "Brier score",
            baseline_brier,
            candidate_brier,
            Some(budgets.max_brier_increase),
        ),
        (
            "ECE",
            baseline_ece,
            candidate_ece,
            Some(budgets.max_ece_increase),
        ),
        (
            "ordinal MAE",
            baseline_ordinal,
            candidate_ordinal,
            Some(budgets.max_ordinal_mae_increase),
        ),
        (
            "mean latency",
            baseline_latency,
            candidate_latency,
            budgets.max_mean_latency_increase_ms,
        ),
        (
            "total cost",
            baseline_cost,
            candidate_cost,
            budgets.max_total_cost_increase_usd,
        ),
    ] {
        let Some(budget) = budget else {
            continue;
        };
        match (base, candidate) {
            (Some(base), Some(candidate)) if candidate - base > budget + f64::EPSILON => {
                failures.push(format!(
                    "{name} increase {:.6} exceeds budget {:.6}",
                    candidate - base,
                    budget
                ));
            }
            (Some(_), None) => failures.push(format!("candidate {name} is unavailable")),
            (None, Some(_)) => failures.push(format!("baseline {name} is unavailable")),
            (None, None) => {}
            _ => {}
        }
    }

    let report = json!({
        "format_version": 1,
        "kind": "decision-regression",
        "id": regression_id(baseline, candidate, &budgets.generated_at),
        "baseline": {
            "calibration_id": baseline["id"],
            "dataset_id": baseline["dataset"]["id"],
            "dataset_revision": baseline["dataset"]["revision"]
        },
        "candidate": {
            "calibration_id": candidate["id"],
            "dataset_id": candidate["dataset"]["id"],
            "dataset_revision": candidate["dataset"]["revision"]
        },
        "budgets": {
            "max_accuracy_drop": budgets.max_accuracy_drop,
            "max_coverage_drop": budgets.max_coverage_drop,
            "max_brier_increase": budgets.max_brier_increase,
            "max_ece_increase": budgets.max_ece_increase,
            "max_ordinal_mae_increase": budgets.max_ordinal_mae_increase,
            "max_mean_latency_increase_ms": budgets.max_mean_latency_increase_ms,
            "max_total_cost_increase_usd": budgets.max_total_cost_increase_usd
        },
        "deltas": {
            "accuracy": accuracy_delta,
            "coverage": coverage_delta,
            "brier_score": brier_delta,
            "expected_calibration_error": ece_delta,
            "ordinal_mae": ordinal_delta,
            "mean_latency_ms": latency_delta,
            "total_cost_usd": cost_delta
        },
        "failures": failures,
        "passed": failures.is_empty(),
        "generated_at": budgets.generated_at,
        "side_effects": false,
        "consequence_authorized": false
    });
    validate_regression(&report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(id: &str, provider: &str, value: bool, confidence: Option<f64>) -> Value {
        json!({
            "format_version":1,
            "kind":"decision-receipt",
            "id":id,
            "spec":{"id":"task.risk","revision":1},
            "state":{"schema_id":"task-state","schema_version":1,"fingerprint":format!("sha256:{id}")},
            "status":"produced",
            "result":{"value":value,"distribution":{"false": if value {0.1} else {0.8},"true":if value {0.9} else {0.2}}},
            "provider":{"type":"custom","id":provider,"model":"m1","version":"1"},
            "uncertainty":{
                "provider_confidence":confidence,
                "calibration":{"status":"unknown"},
                "evidence_coverage":{"value":1.0,"required_present":0,"required_total":0,"missing":[]},
                "evidence_reliability":null,
                "decision_certainty":null
            },
            "evidence":{"used":[],"missing":[]},
            "policy":{"id":"policy:test","revision":1,"disposition":"review","reasons":[]},
            "timing":{"decided_at":"2026-09-18T20:00:00Z","latency_ms":10}
        })
    }

    fn dataset() -> Value {
        json!({
            "format_version":1,
            "kind":"decision-eval-dataset",
            "id":"task-risk-calibration",
            "revision":1,
            "split":"calibration",
            "decision":{"spec_id":"task.risk","spec_revision":1,"decision_kind":"boolean"},
            "state_schema":{"id":"task-state","version":1},
            "cases":[
                {
                    "id":"a","receipt":receipt("receipt-a","provider-a",true,Some(0.9)),
                    "expected":{"value":true},
                    "truth":{"verification_type":"human","observed_at":"2026-09-19T00:00:00Z"},
                    "cost_usd":0.01
                },
                {
                    "id":"b","receipt":receipt("receipt-b","provider-a",false,Some(0.8)),
                    "expected":{"value":false},
                    "truth":{"verification_type":"human","observed_at":"2026-09-19T00:00:00Z"},
                    "cost_usd":0.01
                }
            ],
            "created_at":"2026-09-19T01:00:00Z"
        })
    }

    #[test]
    fn calibration_computes_metrics_and_tunes_only_from_calibration_split() {
        let dataset = dataset();
        let report = calibrate_dataset(
            &dataset,
            &CalibrationOptions {
                bins: 10,
                target_accuracy: Some(1.0),
                minimum_coverage: Some(1.0),
                minimum_samples: 2,
                generated_at: "2026-09-19T02:00:00Z".into(),
            },
        )
        .unwrap();
        assert_eq!(report["metrics"]["coverage"], 1.0);
        assert_eq!(report["metrics"]["accuracy"], 1.0);
        assert!(report["metrics"]["brier_score"].as_f64().unwrap() > 0.0);
        assert_eq!(
            report["threshold"]["selected"]["minimum_provider_confidence"],
            0.8
        );

        let mut test = dataset;
        test["split"] = json!("test");
        assert!(
            calibrate_dataset(
                &test,
                &CalibrationOptions {
                    bins: 10,
                    target_accuracy: Some(1.0),
                    minimum_coverage: Some(1.0),
                    minimum_samples: 2,
                    generated_at: "2026-09-19T02:00:00Z".into(),
                },
            )
            .unwrap_err()
            .contains("calibration split")
        );
    }

    #[test]
    fn missing_confidence_stays_missing_instead_of_becoming_zero() {
        let mut dataset = dataset();
        dataset["cases"][0]["receipt"]["uncertainty"]["provider_confidence"] = Value::Null;
        dataset["cases"][1]["receipt"]["uncertainty"]["provider_confidence"] = Value::Null;
        let report = calibrate_dataset(
            &dataset,
            &CalibrationOptions {
                bins: 10,
                target_accuracy: None,
                minimum_coverage: None,
                minimum_samples: 1,
                generated_at: "2026-09-19T02:00:00Z".into(),
            },
        )
        .unwrap();
        assert_eq!(report["metrics"]["expected_calibration_error"], Value::Null);
        assert_eq!(report["reliability"]["confidence_case_count"], 0);
    }

    #[test]
    fn regression_gate_detects_quality_drop_without_provider_calls() {
        let dataset = dataset();
        let baseline = calibrate_dataset(
            &dataset,
            &CalibrationOptions {
                bins: 10,
                target_accuracy: None,
                minimum_coverage: None,
                minimum_samples: 1,
                generated_at: "2026-09-19T02:00:00Z".into(),
            },
        )
        .unwrap();
        let mut candidate_dataset = dataset.clone();
        candidate_dataset["cases"][0]["receipt"]["result"]["value"] = json!(false);
        candidate_dataset["cases"][0]["receipt"]["result"]["distribution"] =
            json!({"false":0.9,"true":0.1});
        candidate_dataset["cases"][0]["receipt"]["uncertainty"]["provider_confidence"] = json!(0.9);
        let candidate = calibrate_dataset(
            &candidate_dataset,
            &CalibrationOptions {
                bins: 10,
                target_accuracy: None,
                minimum_coverage: None,
                minimum_samples: 1,
                generated_at: "2026-09-19T02:01:00Z".into(),
            },
        )
        .unwrap();
        let regression = compare_calibrations(
            &baseline,
            &candidate,
            &RegressionBudgets {
                max_accuracy_drop: 0.0,
                max_coverage_drop: 0.0,
                max_brier_increase: 0.0,
                max_ece_increase: 1.0,
                max_ordinal_mae_increase: 0.0,
                max_mean_latency_increase_ms: None,
                max_total_cost_increase_usd: None,
                generated_at: "2026-09-19T03:00:00Z".into(),
            },
        )
        .unwrap();
        assert_eq!(regression["passed"], false);
        assert!(!regression["failures"].as_array().unwrap().is_empty());
    }
}
