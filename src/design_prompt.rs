use serde_json::Value;
use std::collections::BTreeSet;

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn validate_genome(genome: &Value) -> Result<(), String> {
    if genome.get("format_version").and_then(Value::as_i64) != Some(1) {
        return Err("expected Design Genome format_version 1".to_string());
    }
    if genome.get("status").and_then(Value::as_str) != Some("approved") {
        return Err("Design Genome must have status 'approved' before deterministic implementation prompt compilation".to_string());
    }
    if !genome.get("identity").is_some_and(Value::is_object) {
        return Err("Design Genome is missing identity".to_string());
    }
    if !genome.get("rules").is_some_and(Value::is_array) {
        return Err("Design Genome is missing rules".to_string());
    }
    if !genome.get("sources").is_some_and(Value::is_array) {
        return Err("Design Genome is missing sources".to_string());
    }
    Ok(())
}

fn validate_task(task: &Value) -> Result<(), String> {
    if task.get("format_version").and_then(Value::as_i64) != Some(1) {
        return Err("expected Design Task format_version 1".to_string());
    }
    if task.get("id").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Err("Design Task is missing id".to_string());
    }
    if task.get("objective").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Err("Design Task is missing objective".to_string());
    }
    if !matches!(task.get("mode").and_then(Value::as_str), Some("explore" | "extend" | "reproduce" | "revise")) {
        return Err("Design Task mode must be explore, extend, reproduce, or revise".to_string());
    }
    Ok(())
}

fn intersects(rule_values: &[String], task_values: &[String]) -> bool {
    rule_values.is_empty() || task_values.iter().any(|value| rule_values.contains(value))
}

fn rule_matches(rule: &Value, task: &Value) -> bool {
    let Some(scope) = rule.get("scope").and_then(Value::as_object) else {
        return true;
    };

    let mode = string(task.get("mode")).into_iter().collect::<Vec<_>>();
    let surface = string(task.get("surface")).into_iter().collect::<Vec<_>>();
    let pages = strings(task.get("pages"));
    let components = strings(task.get("components"));
    let states = strings(task.get("states"));
    let breakpoints = strings(task.get("breakpoints"));

    intersects(&strings(scope.get("modes")), &mode)
        && intersects(&strings(scope.get("surfaces")), &surface)
        && intersects(&strings(scope.get("pages")), &pages)
        && intersects(&strings(scope.get("components")), &components)
        && intersects(&strings(scope.get("states")), &states)
        && intersects(&strings(scope.get("breakpoints")), &breakpoints)
}

fn importance_rank(value: &str) -> u8 {
    match value {
        "required" => 0,
        "recommended" => 1,
        "optional" => 2,
        _ => 3,
    }
}

fn selected_rules<'a>(genome: &'a Value, task: &Value) -> Vec<&'a Value> {
    let mut rules = genome
        .get("rules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|rule| rule_matches(rule, task))
        .collect::<Vec<_>>();

    rules.sort_by(|a, b| {
        let ai = a.get("importance").and_then(Value::as_str).unwrap_or("optional");
        let bi = b.get("importance").and_then(Value::as_str).unwrap_or("optional");
        importance_rank(ai)
            .cmp(&importance_rank(bi))
            .then_with(|| a.get("id").and_then(Value::as_str).unwrap_or("").cmp(b.get("id").and_then(Value::as_str).unwrap_or("")))
    });
    rules
}

fn selected_components<'a>(genome: &'a Value, task: &Value) -> (Vec<&'a Value>, Vec<String>) {
    let requested = strings(task.get("components"));
    if requested.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let requested_set = requested.iter().map(|value| value.to_ascii_lowercase()).collect::<BTreeSet<_>>();
    let mut found = BTreeSet::new();
    let mut components = genome
        .get("components")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|component| component.get("status").and_then(Value::as_str) == Some("approved"))
        .filter(|component| {
            let id = component.get("id").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
            let name = component.get("name").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
            let matches = requested_set.contains(&id) || requested_set.contains(&name);
            if matches {
                if !id.is_empty() { found.insert(id); }
                if !name.is_empty() { found.insert(name); }
            }
            matches
        })
        .collect::<Vec<_>>();
    components.sort_by_key(|component| component.get("id").and_then(Value::as_str).unwrap_or("").to_string());

    let unresolved = requested
        .into_iter()
        .filter(|item| !found.contains(&item.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    (components, unresolved)
}

fn push_list(output: &mut String, values: &[String], empty: &str) {
    if values.is_empty() {
        output.push_str(empty);
        output.push('\n');
    } else {
        for value in values {
            output.push_str("- ");
            output.push_str(value);
            output.push('\n');
        }
    }
}

fn json_scalar(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "<unserializable>".to_string()),
    }
}

pub fn compile_prompt(genome: &Value, task: &Value) -> Result<String, String> {
    validate_genome(genome)?;
    validate_task(task)?;

    let mut output = String::new();
    let task_id = task.get("id").and_then(Value::as_str).expect("validated");
    let objective = task.get("objective").and_then(Value::as_str).expect("validated");
    let mode = task.get("mode").and_then(Value::as_str).expect("validated");
    let genome_id = genome.get("id").and_then(Value::as_str).unwrap_or("unnamed-design-genome");
    let genome_version = genome.get("version").and_then(Value::as_str).unwrap_or("unversioned");

    output.push_str("# Agentic Harness Design Implementation Brief\n\n");
    output.push_str("This brief is compiled deterministically from approved project design context. Do not treat it as permission to redesign the identity unless the task mode explicitly says `revise` or `explore`.\n\n");

    output.push_str("## Task\n\n");
    output.push_str(&format!("- ID: `{task_id}`\n- Objective: {objective}\n- Design mode: `{mode}`\n"));
    if let Some(operation) = task.get("operation").and_then(Value::as_str) {
        output.push_str(&format!("- Operation: `{operation}`\n"));
    }
    if let Some(surface) = task.get("surface").and_then(Value::as_str) {
        output.push_str(&format!("- Surface: `{surface}`\n"));
    }
    output.push('\n');

    output.push_str("## Identity\n\n");
    let identity = genome.get("identity").and_then(Value::as_object).expect("validated");
    push_list(&mut output, &strings(identity.get("principles")), "- No identity principles recorded.");
    let personality = strings(identity.get("personality"));
    if !personality.is_empty() {
        output.push_str("\nPersonality:\n");
        push_list(&mut output, &personality, "");
    }
    let differentiation = strings(identity.get("differentiation"));
    if !differentiation.is_empty() {
        output.push_str("\nDifferentiation:\n");
        push_list(&mut output, &differentiation, "");
    }
    output.push('\n');

    output.push_str("## Applicable design rules\n\n");
    let rules = selected_rules(genome, task);
    if rules.is_empty() {
        output.push_str("- No scoped rules matched this task. Do not invent missing rules; report the gap.\n");
    } else {
        for rule in rules {
            let id = rule.get("id").and_then(Value::as_str).unwrap_or("unnamed-rule");
            let importance = rule.get("importance").and_then(Value::as_str).unwrap_or("optional");
            let kind = rule.get("kind").and_then(Value::as_str).unwrap_or("info");
            let statement = rule.get("statement").and_then(Value::as_str).unwrap_or("");
            output.push_str(&format!("- **{importance} / {kind}** `{id}` — {statement}"));
            if let Some(reason) = rule.get("reason").and_then(Value::as_str) {
                output.push_str(&format!(" Reason: {reason}"));
            }
            output.push('\n');
        }
    }
    output.push('\n');

    output.push_str("## Approved component contracts\n\n");
    let (components, unresolved) = selected_components(genome, task);
    if components.is_empty() {
        output.push_str("- No approved component contract was selected for this task. Prefer existing project primitives and report any missing contract before creating a duplicate primitive.\n");
    } else {
        for component in components {
            let id = component.get("id").and_then(Value::as_str).unwrap_or("unnamed-component");
            let name = component.get("name").and_then(Value::as_str).unwrap_or(id);
            output.push_str(&format!("### {name} (`{id}`)\n\n"));
            let variants = strings(component.get("variants"));
            let states = strings(component.get("states"));
            let usage = strings(component.get("usage"));
            if !variants.is_empty() { output.push_str(&format!("Variants: {}\n\n", variants.join(", "))); }
            if !states.is_empty() { output.push_str(&format!("States: {}\n\n", states.join(", "))); }
            if !usage.is_empty() { push_list(&mut output, &usage, ""); output.push('\n'); }
        }
    }
    if !unresolved.is_empty() {
        output.push_str("Unresolved requested component names (do not fabricate contracts):\n");
        push_list(&mut output, &unresolved, "");
        output.push('\n');
    }

    output.push_str("## Task requirements\n\n");
    push_list(&mut output, &strings(task.get("requirements")), "- No additional requirements recorded.");
    output.push('\n');

    output.push_str("## Non-goals\n\n");
    push_list(&mut output, &strings(task.get("non_goals")), "- Do not expand scope beyond the stated objective and approved product requirements.");
    output.push('\n');

    output.push_str("## Required states and responsive targets\n\n");
    let mut states = strings(task.get("states"));
    if let Some(validation_states) = task.get("validation").and_then(|v| v.get("required_states")) {
        states.extend(strings(Some(validation_states)));
    }
    states.sort();
    states.dedup();
    output.push_str("States:\n");
    push_list(&mut output, &states, "- No explicit states recorded; preserve all states required by existing component contracts.");
    let mut breakpoints = strings(task.get("breakpoints"));
    if let Some(viewports) = task.get("validation").and_then(|v| v.get("required_viewports")) {
        breakpoints.extend(strings(Some(viewports)));
    }
    breakpoints.sort();
    breakpoints.dedup();
    output.push_str("\nResponsive targets:\n");
    push_list(&mut output, &breakpoints, "- No explicit viewport/breakpoint targets recorded; preserve existing responsive behavior.");
    output.push('\n');

    output.push_str("## Implementation constraints\n\n");
    if let Some(implementation) = task.get("implementation").and_then(Value::as_object) {
        let mut entries = implementation.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(key, _)| *key);
        for (key, value) in entries {
            output.push_str(&format!("- `{key}`: {}\n", json_scalar(value)));
        }
    } else {
        output.push_str("- Use the project's existing stack, token sources, and component architecture.\n");
    }
    output.push('\n');

    let anti_patterns = strings(identity.get("anti_patterns"));
    output.push_str("## Identity anti-patterns\n\n");
    push_list(&mut output, &anti_patterns, "- No global anti-patterns recorded; do not invent stylistic prohibitions.");
    output.push('\n');

    output.push_str("## Validation and completion\n\n");
    let checks = task
        .get("validation")
        .and_then(|validation| validation.get("required_checks"))
        .map(|value| strings(Some(value)))
        .unwrap_or_default();
    push_list(&mut output, &checks, "- Run the project's normal validation and report exactly what was not checked.");
    output.push_str("- Report files changed, approved components/tokens reused, deviations, unresolved design-contract gaps, and checks performed.\n");
    output.push_str("- Do not update visual baselines merely to make tests pass.\n");
    output.push_str("- Do not invent product features, testimonials, metrics, or unsupported claims.\n\n");

    output.push_str("## Provenance\n\n");
    output.push_str(&format!("- Design Genome: `{genome_id}` version `{genome_version}`\n"));
    output.push_str(&format!("- Compiler: `agentic-harness-cli` {}\n", env!("CARGO_PKG_VERSION")));
    output.push_str("- Prompt profile: `generic-model-neutral-v1`\n");

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn genome() -> Value {
        json!({
            "format_version": 1,
            "id": "demo",
            "version": "1.2.0",
            "status": "approved",
            "identity": {
                "principles": ["Technical clarity before decoration"],
                "personality": ["editorial", "precise"],
                "differentiation": ["product-specific hierarchy"],
                "anti_patterns": ["Do not introduce decorative gradients without an approved reason"]
            },
            "components": [
                {"id": "button", "name": "Button", "status": "approved", "variants": ["primary", "ghost"], "states": ["default", "focus", "disabled"]}
            ],
            "rules": [
                {"id": "global.required", "kind": "constraint", "importance": "required", "statement": "Reuse canonical tokens"},
                {"id": "settings.rule", "kind": "do", "importance": "recommended", "statement": "Keep controls compact", "scope": {"surfaces": ["settings"]}},
                {"id": "marketing.rule", "kind": "do", "importance": "recommended", "statement": "Use expressive display type", "scope": {"surfaces": ["marketing"]}}
            ],
            "sources": [{"id": "project", "type": "project"}]
        })
    }

    fn task() -> Value {
        json!({
            "format_version": 1,
            "id": "security-settings",
            "objective": "Add account security settings",
            "operation": "create",
            "mode": "extend",
            "surface": "settings",
            "components": ["Button"],
            "states": ["default", "loading", "error"],
            "requirements": ["Preserve existing navigation"],
            "non_goals": ["Do not redesign authentication"],
            "implementation": {"framework": "vue", "language": "typescript"},
            "validation": {"required_checks": ["typecheck", "accessibility"]}
        })
    }

    #[test]
    fn prompt_is_deterministic_and_scope_filtered() {
        let first = compile_prompt(&genome(), &task()).unwrap();
        let second = compile_prompt(&genome(), &task()).unwrap();
        assert_eq!(first, second);
        assert!(first.contains("global.required"));
        assert!(first.contains("settings.rule"));
        assert!(!first.contains("marketing.rule"));
        assert!(first.contains("Button (`button`)"));
        assert!(first.contains("generic-model-neutral-v1"));
    }

    #[test]
    fn refuses_candidate_genome() {
        let mut candidate = genome();
        candidate["status"] = json!("candidate");
        assert!(compile_prompt(&candidate, &task()).unwrap_err().contains("approved"));
    }

    #[test]
    fn unresolved_component_is_reported_not_invented() {
        let mut task = task();
        task["components"] = json!(["MissingWidget"]);
        let prompt = compile_prompt(&genome(), &task).unwrap();
        assert!(prompt.contains("MissingWidget"));
        assert!(prompt.contains("do not fabricate contracts"));
    }
}
