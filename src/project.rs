use crate::scan;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub struct Project {
    pub manifest: Value,
    pub path: PathBuf,
    pub modern: bool,
}
pub fn load(root: &Path) -> Result<Project, String> {
    scan::require_directory(root)?;
    let modern = root.join(".agentic/manifest.yaml").exists();
    if modern && root.join("agentic.yaml").exists() {
        return Err(
            "conflicting current and legacy manifests; reconcile explicitly before continuing"
                .into(),
        );
    }
    let path = root.join(if modern {
        ".agentic/manifest.yaml"
    } else {
        "agentic.yaml"
    });
    let text =
        scan::read(root, &path, 1_000_000).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest: Value = serde_yaml_ng::from_str(&text)
        .map_err(|e| format!("{}: invalid YAML: {e}", path.display()))?;
    let key = if modern { "format_version" } else { "version" };
    if manifest[key] != 1 {
        return Err(format!("{key} must be 1"));
    }
    for key in ["name", "maturity"] {
        if manifest["project"][key]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(format!("project.{key} must be a nonempty string"));
        }
    }
    if !["prototype", "startup", "production", "critical", "beta"]
        .contains(&manifest["project"]["maturity"].as_str().unwrap_or(""))
    {
        return Err("invalid project maturity".into());
    }
    let typ = &manifest["project"]["type"];
    if !(typ.as_str().is_some_and(|s| !s.is_empty())
        || typ.as_array().is_some_and(|a| {
            !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        }))
    {
        return Err("project.type must be a nonempty string or array of strings".into());
    }
    if modern {
        for key in ["context", "modules", "permissions", "adapters"] {
            if !manifest[key].is_object() {
                return Err(format!("{key} must be a mapping"));
            }
        }
        if !typ.is_string() {
            return Err("current project.type must be a string".into());
        }
        for key in ["product", "architecture", "security", "decisions"] {
            if !manifest["context"][key].is_string() {
                return Err(format!("context.{key} is required"));
            }
        }
        for (label, value) in [
            ("modules.packs", &manifest["modules"]["packs"]),
            ("modules.policies", &manifest["modules"]["policies"]),
            ("skills", &manifest["skills"]),
        ] {
            if value.as_array().is_none_or(|a| {
                a.iter().any(|v| !v.is_string())
                    || a.iter().collect::<std::collections::HashSet<_>>().len() != a.len()
            }) {
                return Err(format!("{label} must contain unique strings"));
            }
        }
        if !manifest["adapters"]["canonical_router"].is_string()
            || !manifest["adapters"]["vendor_files_must_be_thin"].is_boolean()
        {
            return Err("invalid adapters configuration".into());
        }
    }
    for section in if modern {
        &["context", "modules", "permissions"][..]
    } else {
        &["sources", "agent"][..]
    } {
        if manifest.get(section).is_some_and(|v| !v.is_object()) {
            return Err(format!("{section} must be a mapping"));
        }
    }
    Ok(Project {
        manifest,
        path,
        modern,
    })
}
impl Project {
    pub fn route(&self, root: &Path, key: &str, default: &str) -> Result<PathBuf, String> {
        let section = if self.modern { "context" } else { "sources" };
        let selected = self.manifest[section].get(key);
        let relative = match selected {
            Some(Value::String(s)) => s.as_str(),
            Some(Value::Null) | None => default,
            _ => return Err(format!("{section}.{key} must be a relative path")),
        };
        if Path::new(relative).is_absolute() {
            return Err(format!("absolute context route: {key}"));
        }
        let base = if self.modern {
            self.path.parent().unwrap()
        } else {
            root
        };
        let path = base.join(relative);
        let resolved = path
            .canonicalize()
            .map_err(|e| format!("context {key}: {e}"))?;
        if !resolved.starts_with(root.canonicalize().map_err(|e| e.to_string())?) {
            return Err(format!("context route escapes repository: {key}"));
        }
        Ok(path)
    }
}
pub fn validate(root: &Path) -> Value {
    let mut errors = Vec::new();
    match load(root) {
        Err(e) => errors.push(e),
        Ok(p) => {
            if scan::read(root, &root.join("AGENTS.md"), 1_000_000).is_err() {
                errors.push("missing or unreadable root AGENTS.md".into());
            }
            for (key, default) in [
                ("product", "PRODUCT.md"),
                ("architecture", "ARCHITECTURE.md"),
                ("security", "SECURITY.md"),
                ("design", "DESIGN.md"),
                ("reference", "REFERENCE.md"),
            ] {
                if p.modern
                    && ["design", "reference"].contains(&key)
                    && p.manifest["context"][key].is_null()
                {
                    continue;
                }
                match p.route(root, key, default) {
                    Err(e) => errors.push(e),
                    Ok(path) => {
                        if scan::read(root, &path, 2_000_000).is_err() {
                            errors.push(format!("unreadable context file: {key}"));
                        }
                    }
                }
            }
            let section = if p.modern { "context" } else { "sources" };
            if let Some(routes) = p.manifest[section].as_object() {
                for (key, value) in routes {
                    if value.is_null() {
                        continue;
                    }
                    if let Err(e) = p.route(root, key, "") {
                        errors.push(e);
                    }
                }
            }
        }
    }
    errors.sort();
    errors.dedup();
    json!({"format_version":1,"kind":"project-validation","valid":errors.is_empty(),"errors":errors,"target":root})
}
