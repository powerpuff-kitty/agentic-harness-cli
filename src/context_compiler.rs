//! Deterministic task-scoped context planning. This module plans context; it never calls a model.
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const OPTIONAL_READ_LIMIT: u64 = 512 * 1024;
const REQUIRED_READ_LIMIT: u64 = 2 * 1024 * 1024;
const DEFAULT_MAX_TOKENS: usize = 18_000;

#[derive(Debug)]
struct Candidate {
    path: String,
    source_kind: &'static str,
    mandatory: bool,
    relevance: u64,
    estimated_tokens: usize,
    path_hits: usize,
    content_hits: usize,
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn words(value: &str) -> BTreeSet<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "from", "into", "this", "that", "then", "than", "add",
        "use", "using", "change", "update", "create", "implement",
    ];
    value
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .map(str::to_ascii_lowercase)
        .filter(|word| word.len() >= 3 && !STOP.contains(&word.as_str()))
        .collect()
}

fn token_estimate(text: &str) -> usize {
    // Portable deterministic heuristic only. Provider tokenizers/billing can differ.
    text.chars().count().saturating_add(3).div_ceil(4).max(1)
}

fn supported_text(path: &Path) -> bool {
    let name = path.file_name().and_then(|x| x.to_str()).unwrap_or("");
    if ["Dockerfile", "Makefile", "Justfile", "Procfile"].contains(&name) {
        return true;
    }
    matches!(
        path.extension().and_then(|x| x.to_str()).unwrap_or(""),
        "rs"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "vue"
            | "svelte"
            | "md"
            | "txt"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "css"
            | "scss"
            | "sass"
            | "less"
            | "html"
            | "htm"
            | "py"
            | "go"
            | "java"
            | "kt"
            | "kts"
            | "swift"
            | "rb"
            | "php"
            | "sh"
            | "bash"
            | "zsh"
            | "sql"
            | "graphql"
            | "gql"
            | "xml"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "cs"
    )
}

fn required_paths(root: &Path, files: &[PathBuf]) -> BTreeSet<String> {
    let mut required = BTreeSet::new();
    for path in ["AGENTS.md", ".agentic/manifest.yaml", "agentic.yaml"] {
        if root.join(path).is_file() {
            required.insert(path.to_owned());
        }
    }

    // Project-owned mandatory policy is always retained. Other packs/skills are task-scoped.
    for path in files {
        let relative = rel(root, path);
        if relative.starts_with(".agentic/policies/") {
            required.insert(relative);
        }
    }

    // Current core truth routes are required. Design/reference/decisions stay task-scoped
    // because they can be large and are not universally relevant.
    if let Ok(project) = crate::project::load(root) {
        for (key, default) in [
            ("product", "PRODUCT.md"),
            ("architecture", "ARCHITECTURE.md"),
            ("security", "SECURITY.md"),
        ] {
            if let Ok(path) = project.route(root, key, default) {
                if path.is_file() {
                    required.insert(rel(root, &path));
                }
            }
        }
    }
    required
}

fn source_kind(path: &str, mandatory: bool) -> &'static str {
    if path == "AGENTS.md" {
        "router"
    } else if path.ends_with("manifest.yaml") || path == "agentic.yaml" {
        "manifest"
    } else if path.starts_with(".agentic/policies/") {
        "policy"
    } else if path.starts_with(".agentic/") {
        "project-context"
    } else if mandatory {
        "project-truth"
    } else {
        "repository-file"
    }
}

fn item(candidate: &Candidate, disposition: &str, reason: &str) -> Value {
    let mut reasons = Vec::new();
    if candidate.mandatory {
        reasons.push("mandatory-project-context");
    }
    if candidate.path_hits > 0 {
        reasons.push("task-term-in-path");
    }
    if candidate.content_hits > 0 {
        reasons.push("task-term-in-content");
    }
    if reasons.is_empty() {
        reasons.push(reason);
    }
    json!({
        "path": candidate.path,
        "source_kind": candidate.source_kind,
        "mandatory": candidate.mandatory,
        "relevance": candidate.relevance,
        "estimated_tokens": candidate.estimated_tokens,
        "signals": {
            "path_term_hits": candidate.path_hits,
            "content_term_hits": candidate.content_hits
        },
        "disposition": disposition,
        "reasons": reasons
    })
}

pub(crate) fn plan(root: &Path, task: &str, max_tokens: usize) -> Value {
    let inventory = crate::scan::inventory(root);
    let task_terms = words(task);
    let required = required_paths(root, &inventory.files);
    let mut candidates = Vec::new();
    let mut unsupported = Vec::new();
    let mut unreadable = Vec::new();

    for path in &inventory.files {
        let relative = rel(root, path);
        let mandatory = required.contains(&relative);
        if !supported_text(path) {
            unsupported.push(relative);
            continue;
        }
        let limit = if mandatory {
            REQUIRED_READ_LIMIT
        } else {
            OPTIONAL_READ_LIMIT
        };
        let text = match crate::scan::read(root, path, limit) {
            Ok(text) => text,
            Err(error) => {
                unreadable.push(json!({
                    "path": relative,
                    "mandatory": mandatory,
                    "error": error
                }));
                continue;
            }
        };
        let path_terms = words(&relative);
        let content_terms = words(&text);
        let path_hits = task_terms.intersection(&path_terms).count();
        let content_hits = task_terms.intersection(&content_terms).count();
        let relevance = (path_hits as u64 * 20) + (content_hits as u64 * 5);
        candidates.push(Candidate {
            source_kind: source_kind(&relative, mandatory),
            path: relative,
            mandatory,
            relevance,
            estimated_tokens: token_estimate(&text),
            path_hits,
            content_hits,
        });
    }

    candidates.sort_by(|a, b| {
        b.mandatory
            .cmp(&a.mandatory)
            .then_with(|| b.relevance.cmp(&a.relevance))
            .then_with(|| a.estimated_tokens.cmp(&b.estimated_tokens))
            .then_with(|| a.path.cmp(&b.path))
    });

    let mut included = Vec::new();
    let mut deferred = Vec::new();
    let mut estimated_included_tokens = 0usize;
    for candidate in &candidates {
        if candidate.mandatory {
            estimated_included_tokens =
                estimated_included_tokens.saturating_add(candidate.estimated_tokens);
            included.push(item(candidate, "included", "mandatory-project-context"));
            continue;
        }
        if candidate.relevance == 0 {
            deferred.push(item(candidate, "deferred", "low-relevance"));
            continue;
        }
        if estimated_included_tokens.saturating_add(candidate.estimated_tokens) <= max_tokens {
            estimated_included_tokens =
                estimated_included_tokens.saturating_add(candidate.estimated_tokens);
            included.push(item(candidate, "included", "within-budget"));
        } else {
            deferred.push(item(candidate, "deferred", "budget"));
        }
    }

    let required_unavailable = unreadable
        .iter()
        .filter(|value| value["mandatory"] == true)
        .count();
    let over_budget = estimated_included_tokens > max_tokens;
    json!({
        "format_version": 1,
        "kind": "compiled-context-plan",
        "target": root,
        "task": task,
        "budget": {
            "max_tokens": max_tokens,
            "estimated_included_tokens": estimated_included_tokens,
            "over_budget": over_budget,
            "estimation": "characters/4 heuristic; not provider billing"
        },
        "complete": inventory.errors.is_empty() && required_unavailable == 0,
        "included": included,
        "deferred": deferred,
        "coverage": {
            "scan": inventory.report(),
            "supported_text_files": candidates.len(),
            "unsupported_files": unsupported.len(),
            "unsupported_examples": unsupported.into_iter().take(25).collect::<Vec<_>>(),
            "unreadable": unreadable,
            "required_unavailable": required_unavailable,
            "note": "Binary/unsupported files are not ranked in the initial deterministic text planner."
        }
    })
}

pub(crate) fn default_max_tokens() -> usize {
    DEFAULT_MAX_TOKENS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn put(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn task_relevance_beats_unrelated_optional_files() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "AGENTS.md", "Load only relevant project context.");
        put(
            dir.path(),
            "src/github_project.rs",
            "fn validate_github_project_creation() {}",
        );
        put(dir.path(), "src/unrelated.rs", "fn render_cat_gallery() {}");

        crate::scan::begin();
        let result = plan(dir.path(), "validate GitHub project creation", 10_000);
        let included = result["included"].as_array().unwrap();
        assert_eq!(included[0]["path"], "AGENTS.md");
        assert!(
            included
                .iter()
                .any(|item| item["path"] == "src/github_project.rs")
        );
        assert!(
            result["deferred"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["path"] == "src/unrelated.rs")
        );
    }

    #[test]
    fn mandatory_context_is_not_dropped_to_fit_budget() {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "AGENTS.md",
            &"mandatory context ".repeat(100),
        );

        crate::scan::begin();
        let result = plan(dir.path(), "anything", 1);
        assert_eq!(result["included"][0]["path"], "AGENTS.md");
        assert_eq!(result["budget"]["over_budget"], true);
        assert!(
            result["budget"]["estimated_included_tokens"]
                .as_u64()
                .unwrap()
                > 1
        );
    }

    #[test]
    fn unchanged_inputs_produce_identical_plan() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), "AGENTS.md", "Relevant router.");
        put(dir.path(), "src/context.rs", "context planner budget");

        crate::scan::begin();
        let first = plan(dir.path(), "context budget", 1000);
        crate::scan::begin();
        let second = plan(dir.path(), "context budget", 1000);
        assert_eq!(first, second);
    }
}
