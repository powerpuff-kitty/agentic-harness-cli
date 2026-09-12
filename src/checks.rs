//! Check policy planning and explicit local execution dispatch.
use crate::check_inputs;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

pub(crate) fn shape<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let object = value.as_object().ok_or("checks: expected an object")?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err("checks: missing or unknown contract fields".into());
    }
    Ok(object)
}

pub(crate) fn text(value: &Value, max: usize) -> Result<&str, String> {
    let value = value.as_str().ok_or("checks: expected a string")?;
    if value.is_empty() || value.chars().count() > max || value.chars().any(char::is_control) {
        return Err("checks: empty, oversized or control-containing string".into());
    }
    Ok(value)
}

fn id(value: &Value) -> Result<&str, String> {
    let value = text(value, 128)?;
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return Err("checks: unsupported identifier".into());
    }
    Ok(value)
}

fn array(value: &Value, min: usize, max: usize) -> Result<&Vec<Value>, String> {
    let value = value.as_array().ok_or("checks: expected an array")?;
    if value.len() < min || value.len() > max {
        return Err("checks: array size outside supported bounds".into());
    }
    Ok(value)
}

pub(crate) fn number(value: &Value, max: u64) -> Result<u64, String> {
    let value = value
        .as_u64()
        .ok_or("checks: expected an unsigned integer")?;
    if value == 0 || value > max {
        return Err("checks: numeric limit outside supported bounds".into());
    }
    Ok(value)
}

pub(crate) fn validate_policy(policy: &Value) -> Result<Vec<String>, String> {
    shape(
        policy,
        &[
            "format_version",
            "kind",
            "inputs",
            "checks",
            "required_controls",
            "max_age_ms",
        ],
    )?;
    if policy["format_version"].as_u64() != Some(1) || policy["kind"] != "check-policy" {
        return Err("checks: unsupported policy format".into());
    }
    number(&policy["max_age_ms"], 86_400_000)?;
    let mut inputs = Vec::new();
    for input in array(&policy["inputs"], 1, 64)? {
        let input = text(input, 512)?;
        if !check_inputs::valid_path(input, false) || inputs.iter().any(|x| x == input) {
            return Err("checks: duplicate or invalid input path".into());
        }
        inputs.push(input.to_string());
    }
    let mut ids = BTreeSet::new();
    let mut has_required = false;
    for check in array(&policy["checks"], 1, 64)? {
        shape(
            check,
            &[
                "id",
                "argv",
                "cwd",
                "required",
                "timeout_ms",
                "max_output_bytes",
            ],
        )?;
        if !ids.insert(id(&check["id"])?.to_string()) {
            return Err("checks: duplicate check identifier".into());
        }
        for arg in array(&check["argv"], 1, 64)? {
            text(arg, 4096)?;
        }
        let executable = text(&check["argv"][0], 128)?;
        if !executable
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || executable.starts_with('.')
            || executable.starts_with('-')
        {
            return Err(
                "checks: argv[0] must be an executable name, not a path or shell expression".into(),
            );
        }
        if !check_inputs::valid_path(text(&check["cwd"], 512)?, true) {
            return Err("checks: invalid working directory".into());
        }
        has_required |= check["required"]
            .as_bool()
            .ok_or("checks: required must be boolean")?;
        number(&check["timeout_ms"], 300_000)?;
        number(&check["max_output_bytes"], 1_048_576)?;
    }
    if !has_required {
        return Err("checks: policy must contain at least one required check".into());
    }
    let mut requirements = BTreeSet::new();
    for requirement in array(&policy["required_controls"], 0, 64)? {
        shape(requirement, &["rule_id", "capability"])?;
        let rule = id(&requirement["rule_id"])?;
        let capability = text(&requirement["capability"], 16)?;
        if !["declared", "delivered", "checked", "enforced"].contains(&capability)
            || !requirements.insert((rule, capability))
        {
            return Err("checks: duplicate or unsupported control requirement".into());
        }
    }
    Ok(inputs)
}

pub(crate) fn plan(target: &Path, config: &str) -> Result<Value, String> {
    let root = check_inputs::root(target)?;
    let config_path = check_inputs::resolve(&root, config, false)?;
    let bytes = check_inputs::read(&config_path, 262_144)?;
    let policy = crate::strict_json::decode(&bytes)?;
    let inputs = validate_policy(&policy)?;
    for check in policy["checks"].as_array().unwrap() {
        let cwd = check_inputs::resolve(&root, check["cwd"].as_str().unwrap(), true)?;
        if !cwd.is_dir() {
            return Err("checks: working directory is not a directory".into());
        }
    }
    let (source_digest, entries) = check_inputs::snapshot(&root, &inputs)?;
    let second = check_inputs::snapshot(&root, &inputs)?;
    if second.0 != source_digest || check_inputs::read(&config_path, 262_144)? != bytes {
        return Err("checks: policy or inputs changed during planning".into());
    }
    let policy_digest = check_inputs::hash(&bytes);
    let planner_digest = check_inputs::framed_hash(
        b"ah-check-planner-v1\0",
        &[
            include_bytes!("checks.rs"),
            include_bytes!("check_inputs.rs"),
            include_bytes!("../upstream.lock.json"),
        ],
    );
    let review_digest = check_inputs::framed_hash(
        b"ah-check-plan-v1\0",
        &[
            policy_digest.as_bytes(),
            source_digest.as_bytes(),
            planner_digest.as_bytes(),
        ],
    );
    let requirements: Vec<Value> = policy["required_controls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rule| {
            json!({"rule_id":rule["rule_id"],"capability":rule["capability"],"status":"unverified"})
        })
        .collect();
    Ok(json!({
        "format_version":1,"kind":"check-plan","policy":policy,
        "policy_digest":policy_digest,"source_digest":source_digest,
        "planner_digest":planner_digest,"review_digest":review_digest,
        "generator":crate::version(),"inputs":entries,"control_requirements":requirements,
        "execution_permitted":false,"checks_executed":false,"executable_identity_verified":false,
        "not_checked":["command execution","executable identity and version","host permissions and sandbox",
            "runtime enforcement","completion evidence and freshness","undeclared inputs"],
        "limits":{"max_file_bytes":2_000_000,"max_total_bytes":64_000_000,"max_entries":10_000,"max_depth":64}
    }))
}

pub(crate) fn run(args: Vec<String>) {
    if args.len() < 2 || ["--help", "-h"].contains(&args[1].as_str()) {
        println!(
            "usage: ah checks <plan|prepare|run> [TARGET] [--config PATH]\nplan and prepare are read-only. prepare/run accept --settings PATH.\nrun requires --approve-review DIGEST and --allow-unsandboxed. Linux/macOS execution only."
        );
        return;
    }
    let operation = &args[1];
    let mut target = ".";
    let mut config = ".agentic/checks.json";
    let mut settings = ".agentic/check-execution.json";
    let mut approval = None;
    let mut allow_unsandboxed = false;
    let mut seen = BTreeSet::new();
    let mut index = 2;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg.starts_with("--") {
            if !seen.insert(arg) {
                crate::fail("checks: repeated option is not allowed");
            }
            if arg == "--allow-unsandboxed" {
                allow_unsandboxed = true;
            } else {
                index += 1;
                let value = args[index].as_str();
                match arg {
                    "--config" => config = value,
                    "--settings" => settings = value,
                    "--approve-review" => approval = Some(value),
                    _ => crate::fail("checks: unsupported option"),
                }
            }
        } else {
            target = arg;
        }
        index += 1;
    }
    let result = match operation.as_str() {
        "plan" => plan(Path::new(target), config),
        "prepare" => crate::execution_review::prepare(Path::new(target), config, settings),
        "run" => crate::check_execution::run(
            Path::new(target),
            config,
            settings,
            approval.unwrap_or_else(|| crate::fail("checks: --approve-review is required")),
            allow_unsandboxed,
        ),
        _ => crate::fail("checks: unsupported operation"),
    };
    match result {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            if operation == "run" && value["checks_passed"] != true {
                crate::finish(1);
            }
        }
        Err(error) => crate::fail(error),
    }
}
