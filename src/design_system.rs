use serde_json::{Value, json};
use std::{collections::BTreeSet, io, path::Path, sync::LazyLock};
static TAGS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<([A-Za-z][A-Za-z0-9-]*)(?:\s|/?>)").unwrap());
static COMMENTS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)<!--.*?-->|/\*.*?\*/").unwrap());
static COLORS: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)(?:^|[\s:,(])#(?:[a-f0-9]{8}|[a-f0-9]{6}|[a-f0-9]{4}|[a-f0-9]{3})\b")
        .unwrap()
});
static TOKENS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"--[a-zA-Z][a-zA-Z0-9_-]*\s*:").unwrap());
fn section(text: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    if let Some(start) = text.find(&open)
        && let Some(offset) = text[start..].find('>')
        && let Some(end) = text.rfind(&close)
    {
        let start = start + offset + 1;
        if end >= start {
            return text[start..end].into();
        }
    }
    String::new()
}
fn capability(name: &str) -> String {
    let compact: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let name = compact
        .strip_prefix('s')
        .filter(|_| name.starts_with('S'))
        .unwrap_or(&compact);
    match name {
        "modal" | "confirmmodal" => "dialog",
        "dateinput" | "datetimeinput" | "datetimepicker" | "daterangefield" => "date-picker",
        "appheader" => "header",
        "applayout" | "workspaceshell" => "app-shell",
        "errorpanel" => "error-state",
        x => x,
    }
    .into()
}
fn configuration(root: &Path) -> Result<Value, String> {
    let path = root.join(".agentic/design-system.json");
    if !path.exists() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(&crate::scan::read(root, &path, 1_000_000)?)
        .map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("design-system configuration must be an object".into());
    }
    if value.get("format_version").is_some_and(|v| v != 1) {
        return Err("unsupported design-system configuration version".into());
    }
    if value.as_object().unwrap().keys().any(|k| {
        ![
            "format_version",
            "roots",
            "required_components",
            "exceptions",
            "aliases",
        ]
        .contains(&k.as_str())
    }) {
        return Err("unknown design-system configuration field".into());
    }
    if value.get("aliases").is_some_and(|v| {
        v.as_object()
            .is_none_or(|a| a.values().any(|v| v.as_str().is_none_or(|s| s.is_empty())))
    }) {
        return Err("aliases must map capability names to strings".into());
    }
    for key in ["roots", "required_components", "exceptions"] {
        if let Some(v) = value.get(key)
            && !v
                .as_array()
                .is_some_and(|a| a.iter().all(|v| v.as_str().is_some_and(|s| !s.is_empty())))
        {
            return Err(format!("{key} must be an array of strings"));
        }
    }
    Ok(value)
}
fn inspect(root: &Path) -> Value {
    let inventory = crate::scan::inventory(root);
    let config = match configuration(root) {
        Ok(v) => v,
        Err(e) => {
            return json!({"active":true,"score":null,"status":"invalid-configuration","violations":[{"severity":"high","message":e,"evidence":[]}],"plan":{},"missing_components":[]});
        }
    };
    let configured: Vec<_> = config["roots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let exceptions: BTreeSet<_> = config["exceptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut found = BTreeSet::new();
    let mut needed = BTreeSet::new();
    let mut raw = Vec::new();
    let mut colors = Vec::new();
    let mut user_facing = false;
    let mut shared = false;
    let mut reads = Vec::new();
    for path in &inventory.files {
        if !crate::scan::product(root, path) {
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ![
            "vue", "svelte", "html", "htm", "tsx", "jsx", "css", "scss", "sass", "less",
        ]
        .contains(&ext)
        {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let text = match crate::scan::read(root, path, 2_000_000) {
            Ok(t) => t,
            Err(reason) => {
                reads.push(json!({"path":rel,"reason":reason}));
                continue;
            }
        };
        let is_shared = rel.split('/').any(|s| s == "design-system")
            || rel.contains("/components/ui/")
            || rel.starts_with("components/ui/")
            || configured
                .iter()
                .any(|p| rel.starts_with(&format!("{}/", p.trim_end_matches('/'))));
        shared |= is_shared;
        let css = if ["vue", "svelte"].contains(&ext) {
            section(&text, "style")
        } else if ["css", "scss", "sass", "less"].contains(&ext) {
            text.clone()
        } else {
            String::new()
        };
        let css = COMMENTS.replace_all(&css, "");
        if is_shared {
            if TOKENS.is_match(&css) {
                found.insert("tokens".into());
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                found.insert(capability(stem));
            }
            if text.contains("role=\"dialog\"") || text.contains("role='dialog'") {
                found.insert("dialog".into());
            }
        }
        let template = match ext {
            "vue" => section(&text, "template"),
            "svelte" | "html" | "htm" => {
                static NON_MARKUP: LazyLock<regex::Regex> = LazyLock::new(|| {
                    regex::Regex::new(r"(?s)<(?:script|style)\b[^>]*>.*?</(?:script|style)\s*>")
                        .unwrap()
                });
                NON_MARKUP.replace_all(&text, "").into_owned()
            }
            _ => String::new(),
        };
        let template = COMMENTS.replace_all(&template, "");
        let mut tags: Vec<String> = TAGS
            .captures_iter(&template)
            .map(|c| c[1].to_string())
            .collect();
        if ["tsx", "jsx"].contains(&ext) {
            let (jsx_tags, gaps) = crate::syntax::jsx_tags(path, &text);
            tags.extend(jsx_tags);
            if !gaps.is_empty() {
                reads.push(json!({"path":rel,"lines":gaps,"reason":"parser could not fully interpret JSX; component coverage is partial"}));
            }
        }
        user_facing |= !tags.is_empty();
        let count = tags
            .iter()
            .filter(|t| ["button", "input", "select", "textarea"].contains(&t.as_str()))
            .count();
        for tag in &tags {
            if ["button", "input", "select", "textarea"].contains(&tag.as_str()) {
                needed.insert(capability(tag));
            }
        }
        if !is_shared && !exceptions.contains(rel.as_str()) {
            if count > 0 {
                raw.push(json!({"path":rel,"count":count}));
            }
            let count = COLORS.find_iter(&css).count();
            if count > 0 {
                colors.push(json!({"path":rel,"count":count}));
            }
        }
    }
    if let Some(aliases) = config["aliases"].as_object() {
        for (name, target) in aliases {
            if target
                .as_str()
                .is_some_and(|s| found.contains(&capability(s)))
            {
                found.insert(name.clone());
            }
        }
    }
    let required: BTreeSet<String> = config["required_components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let missing: Vec<_> = required.difference(&found).cloned().collect();
    needed.extend(required);
    let active = shared || !configured.is_empty() || !missing.is_empty();
    let mut violations = Vec::new();
    if active {
        if !raw.is_empty() {
            violations.push(json!({"severity":"medium","type":"raw-control-bypass","message":"Native controls outside shared component roots require review.","evidence":raw}));
        }
        if !colors.is_empty() {
            violations.push(json!({"severity":"medium","type":"hardcoded-visual-value","message":"Literal CSS colors outside shared component roots require review.","evidence":colors}));
        }
        if !missing.is_empty() {
            violations.push(json!({"severity":"medium","type":"missing-required-component","message":"Explicitly configured capabilities lack static evidence.","evidence":missing}));
        }
    }
    json!({"active":active,"score":null,"status":if !active{"not-required"}else if violations.is_empty(){"observed"}else{"needs-review"},"plan":{"active":active,"user_facing":user_facing,"needed_components":needed,"discovered_capabilities":found,"requirement_source":"explicit project configuration; native controls provide advisory suggestions"},"violations":violations,"missing_components":missing,"scan":inventory.report(),"not_checked":["runtime behavior","visual/accessibility compliance","component semantic equivalence beyond declared aliases","CSS-in-JS"],"skipped":reads})
}
pub fn component_plan(root: &Path) -> Value {
    inspect(root)["plan"].clone()
}
pub fn audit(root: &Path) -> Value {
    inspect(root)
}
pub fn write_plan(root: &Path) -> io::Result<std::path::PathBuf> {
    let plan = component_plan(root);
    let mut body = String::from(
        "# Design System Components\n\nGenerated observations. Review inferred needs before declaring requirements.\n\n",
    );
    for name in plan["needed_components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        body.push_str(&format!("- [ ] `{name}`\n"));
    }
    let path = root.join("DESIGN_SYSTEM_COMPONENTS.md");
    crate::scan::write(root, "DESIGN_SYSTEM_COMPONENTS.md", body.as_bytes())?;
    Ok(path)
}
