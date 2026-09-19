//! Required context discovery never overrides repository ignore or symlink policy.
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

#[derive(Default)]
pub(super) struct Requirements {
    pub paths: BTreeSet<String>,
    pub unavailable: BTreeSet<String>,
    pub errors: BTreeSet<String>,
}

impl Requirements {
    fn missing(&mut self, id: &str) {
        self.unavailable.insert(id.to_owned());
        self.errors.insert(format!("required-context-unavailable:{id}"));
    }
}

fn normalized(root: &Path, path: &Path) -> Option<String> {
    let mut current = root.to_path_buf();
    for component in path.strip_prefix(root).ok()?.components() {
        current.push(component.as_os_str());
        if current.symlink_metadata().ok()?.file_type().is_symlink() {
            return None;
        }
    }
    let canonical = root.canonicalize().ok()?;
    let resolved = path.canonicalize().ok()?;
    if !resolved.is_file() {
        return None;
    }
    Some(resolved.strip_prefix(canonical).ok()?.to_str()?.replace('\\', "/"))
}

pub(super) fn collect(root: &Path, files: &[PathBuf]) -> Requirements {
    let mut result = Requirements::default();
    let visible: BTreeSet<_> = files.iter().map(|p| super::rel(root, p)).collect();
    for path in &visible {
        if path == "AGENTS.md"
            || path.ends_with("/AGENTS.md")
            || path.starts_with(".agentic/policies/")
        {
            result.paths.insert(path.clone());
        }
    }
    for path in ["AGENTS.md", ".agentic/README.md"] {
        if root.join(path).symlink_metadata().is_ok() {
            result.paths.insert(path.into());
        }
    }
    let manifests = [".agentic/manifest.yaml", "agentic.yaml"];
    let declared: Vec<_> = manifests
        .iter()
        .filter(|p| root.join(p).symlink_metadata().is_ok())
        .copied()
        .collect();
    for path in &declared {
        result.paths.insert((*path).into());
    }
    if declared.is_empty() {
        for path in ["PRODUCT.md", "ARCHITECTURE.md", "SECURITY.md"] {
            if root.join(path).symlink_metadata().is_ok() {
                result.paths.insert(path.into());
            }
        }
    } else {
        result.paths.insert("AGENTS.md".into());
        // An ignored manifest must not be read through the explicit project loader.
        if declared.iter().any(|p| !visible.contains(*p)) {
            result.missing("manifest-not-in-inventory");
        } else {
            match crate::project::load(root) {
                Err(_) => result.missing("invalid-or-conflicting-manifest"),
                Ok(project) => {
                    for (key, default) in [
                        ("product", "PRODUCT.md"),
                        ("architecture", "ARCHITECTURE.md"),
                        ("security", "SECURITY.md"),
                    ] {
                        let path = project
                            .route(root, key, default)
                            .ok()
                            .and_then(|p| normalized(root, &p));
                        match path {
                            Some(path) => {
                                result.paths.insert(path);
                            }
                            None => result.missing(&format!("route-{key}")),
                        }
                    }
                    if let Some(policies) = project.manifest["modules"]["policies"].as_array() {
                        for name in policies {
                            let name = name.as_str().unwrap_or("");
                            if name.is_empty()
                                || !name.chars().all(|c| c.is_ascii_alphanumeric() || "_-/".contains(c))
                                || !Path::new(name).components().all(|c| matches!(c, Component::Normal(_)))
                            {
                                result.missing("invalid-policy-name");
                                continue;
                            }
                            result.paths.insert(format!(".agentic/policies/{name}.md"));
                        }
                    }
                }
            }
        }
    }
    for path in result.paths.clone() {
        if !visible.contains(&path) {
            result.missing(&path);
        }
    }
    result
}
