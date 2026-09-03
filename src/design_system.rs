use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SKIP: &[&str] = &[".git", "node_modules", "vendor", "dist", "build", ".next", ".nuxt", "target", ".venv", "venv", "coverage"];

fn files(root: &Path) -> Vec<PathBuf> {
    fn walk(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(&path);
            if rel.components().any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().as_ref())) { continue; }
            if path.is_dir() { walk(&path, root, out); } else { out.push(path); }
        }
    }
    let mut out = Vec::new(); walk(root, root, &mut out); out
}

fn corpus(root: &Path) -> String {
    let mut out = String::new();
    for path in files(root) {
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        out.push_str(&rel.to_ascii_lowercase()); out.push('\n');
        if fs::metadata(&path).map(|m| m.len() <= 1_000_000).unwrap_or(false) {
            if let Ok(text) = fs::read_to_string(&path) { out.push_str(&text.to_ascii_lowercase()); out.push('\n'); }
        }
    }
    out
}

fn has_any(text: &str, needles: &[&str]) -> bool { needles.iter().any(|n| text.contains(n)) }

pub fn component_plan(root: &Path) -> Value {
    let text = corpus(root);
    let user_facing = has_any(&text, &[".vue", ".tsx", ".jsx", ".svelte", "template>", "<body", "web-app", "frontend"]);
    let mut needed: BTreeSet<&str> = BTreeSet::new();
    if user_facing { needed.extend(["tokens","typography","icon","layout","surface","button","icon-button","alert","spinner","skeleton","empty-state","error-state","tooltip"]); }
    if has_any(&text, &["login","sign in","signup","register","password","authentication"]) { needed.extend(["form-field","input","checkbox","button","alert"]); }
    if has_any(&text, &["dashboard","admin","workspace","console"]) { needed.extend(["app-shell","header","sidebar","tabs","card","table","stat"]); }
    if has_any(&text, &["search","query","filter"]) { needed.extend(["search","filter","combobox"]); }
    if has_any(&text, &["upload","attachment","file storage","media"]) { needed.extend(["file-upload","progress"]); }
    if has_any(&text, &["calendar","date picker","booking","schedule"]) { needed.insert("date-picker"); }
    if has_any(&text, &["notification","toast","success message"]) { needed.extend(["toast","alert"]); }
    if has_any(&text, &["modal","dialog","confirm","drawer"]) { needed.extend(["dialog","drawer"]); }
    if has_any(&text, &["pagination","page size","next page"]) { needed.insert("pagination"); }
    if has_any(&text, &["chart","analytics","metric","timeseries"]) { needed.extend(["chart","stat"]); }
    if has_any(&text, &["table","data grid","datatable"]) { needed.extend(["table","pagination"]); }
    if has_any(&text, &["avatar","profile picture","user menu"]) { needed.extend(["avatar","menu","dropdown"]); }
    if has_any(&text, &["select","dropdown field"]) { needed.insert("select"); }
    if has_any(&text, &["textarea","description field","comment"]) { needed.insert("textarea"); }
    if has_any(&text, &["switch","toggle"]) { needed.insert("switch"); }

    let active = root.join(".agentic/packs/design-system").exists()
        || has_any(&text, &["design-system","design system","design tokens","component library"])
        || root.join("packages/design-system").exists();
    json!({"active":active,"user_facing":user_facing,"needed_components":needed.into_iter().collect::<Vec<_>>()})
}

fn component_present(root: &Path, name: &str) -> bool {
    let compact = name.replace('-', "");
    files(root).iter().any(|p| {
        let rel = p.strip_prefix(root).unwrap_or(p).to_string_lossy().to_ascii_lowercase();
        if !(rel.contains("component") || rel.contains("design-system") || rel.contains("ui/")) { return false; }
        let normalized: String = rel.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        normalized.contains(&compact)
    })
}

fn hardcoded_color_count(text: &str) -> usize {
    text.split_whitespace().filter(|token| {
        let t = token.trim_matches(|c: char| !c.is_ascii_hexdigit() && c != '#');
        t.starts_with('#') && matches!(t.len(), 4 | 7 | 9) && t[1..].chars().all(|c| c.is_ascii_hexdigit())
    }).count()
}

pub fn audit(root: &Path) -> Value {
    let plan = component_plan(root);
    if !plan["active"].as_bool().unwrap_or(false) { return json!({"active":false,"score":null,"status":"not-required","plan":plan,"violations":[],"missing_components":[]}); }
    let mut raw_controls = Vec::new(); let mut hardcoded_colors = Vec::new();
    for path in files(root) {
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let lower = rel.to_ascii_lowercase();
        if lower.contains("component") || lower.contains("design-system") || lower.contains("storybook") { continue; }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let lower_text = text.to_ascii_lowercase();
        let raw = ["<button","<input","<select","<textarea"].iter().filter(|n| lower_text.contains(**n)).count();
        if raw > 0 { raw_controls.push(json!({"path":rel,"count":raw})); }
        let colors = hardcoded_color_count(&text); if colors > 0 { hardcoded_colors.push(json!({"path":rel,"count":colors})); }
    }
    let required = plan["needed_components"].as_array().cloned().unwrap_or_default();
    let missing: Vec<String> = required.iter().filter_map(Value::as_str).filter(|name| !component_present(root, name)).map(str::to_string).collect();
    let raw_count: i64 = raw_controls.iter().map(|x| x["count"].as_i64().unwrap_or(0)).sum();
    let color_count: i64 = hardcoded_colors.iter().map(|x| x["count"].as_i64().unwrap_or(0)).sum();
    let score = (100 - (raw_count * 8).min(40) - (color_count * 2).min(20) - (missing.len() as i64 * 4).min(40)).max(0);
    let mut violations = Vec::new();
    if raw_count > 0 { violations.push(json!({"severity":"medium","type":"raw-control-bypass","message":"Product code contains raw form/action controls outside recognized design-system component paths.","evidence":raw_controls})); }
    if color_count > 0 { violations.push(json!({"severity":"medium","type":"hardcoded-visual-value","message":"Product code contains hard-coded color values outside recognized design-system paths.","evidence":hardcoded_colors})); }
    if !missing.is_empty() { violations.push(json!({"severity":"medium","type":"missing-required-component","message":"Components inferred from product flows are not represented in discovered design-system component paths.","evidence":missing})); }
    json!({"active":true,"score":score,"status":if score>=85{"pass"}else if score>=65{"needs-attention"}else{"fail"},"plan":plan,"violations":violations,"missing_components":missing})
}

pub fn write_plan(root: &Path) -> io::Result<PathBuf> {
    let plan = component_plan(root);
    let components = plan["needed_components"].as_array().cloned().unwrap_or_default();
    let mut body = String::from("# Design System Components\n\nGenerated by `ah design-system-components --write`. Validate this list against actual product flows before treating it as canonical.\n\n## Required / likely components\n\n");
    for component in components.iter().filter_map(Value::as_str) { body.push_str(&format!("- [ ] `{component}`\n")); }
    body.push_str("\n## Enforcement\n\nWhen a design system is active, product code should consume its components and tokens. Exceptions should be documented. `ah audit` reports deterministic design-system compliance evidence.\n");
    let path = root.join("DESIGN_SYSTEM_COMPONENTS.md"); fs::write(&path, body)?; Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_dashboard_components() {
        let root = std::env::temp_dir().join(format!("ah-ds-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root); fs::create_dir_all(&root).unwrap();
        fs::write(root.join("app.tsx"), "dashboard search analytics login").unwrap();
        let plan = component_plan(&root); let items = plan["needed_components"].as_array().unwrap();
        assert!(items.iter().any(|x| x == "sidebar")); assert!(items.iter().any(|x| x == "search"));
        let _ = fs::remove_dir_all(root);
    }
}
