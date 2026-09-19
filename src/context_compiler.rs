//! Offline selection plans; no model calls, source disclosure or project writes.
mod requirements;

use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path, sync::OnceLock};

const OPTIONAL_READ_LIMIT: u64 = 512 * 1024;
const REQUIRED_READ_LIMIT: u64 = 2 * 1024 * 1024;
const TOTAL_READ_LIMIT: usize = 64 * 1024 * 1024;
const SCHEMA: &str = include_str!(
    "../upstream/agentic-harness/catalog/schema/compiled-context-plan.v1.schema.json"
);

struct Candidate {
    path: String,
    digest: String,
    source_kind: &'static str,
    mandatory: bool,
    relevance: u64,
    tokens: usize,
    path_hits: usize,
    content_hits: usize,
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn valid_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
}

fn words(value: &str) -> BTreeSet<String> {
    static CAMEL: OnceLock<Regex> = OnceLock::new();
    static ACRONYM: OnceLock<Regex> = OnceLock::new();
    let camel = CAMEL.get_or_init(|| Regex::new(r"([a-z0-9])([A-Z])").unwrap());
    let acronym = ACRONYM.get_or_init(|| Regex::new(r"([A-Z])([A-Z][a-z])").unwrap());
    let expanded = camel.replace_all(value, "$1 $2");
    let expanded = acronym.replace_all(&expanded, "$1 $2");
    expanded
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| {
            word.len() >= 2
                && ![
                    "the", "and", "for", "with", "from", "into", "this", "that", "add", "use",
                    "using", "change", "update", "create", "implement", "to", "of", "in", "is",
                    "an", "as", "at",
                ]
                .contains(&word.as_str())
        })
        .collect()
}

fn estimate(text: &str) -> usize {
    text.chars().count().div_ceil(4).max(1)
}

fn sensitive(path: &str) -> bool {
    path.split('/').any(|part| {
        let name = part.to_ascii_lowercase();
        name == ".env"
            || name.starts_with(".env.")
            || [".ssh", ".aws", ".npmrc", ".pypirc", ".netrc", "credentials.json", "secrets.json"]
                .contains(&name.as_str())
            || [".pem", ".key", ".p12", ".pfx"].iter().any(|ext| name.ends_with(ext))
    })
}

fn supported(path: &Path) -> bool {
    if ["Dockerfile", "Makefile", "Justfile", "Procfile"]
        .contains(&path.file_name().and_then(|x| x.to_str()).unwrap_or(""))
    {
        return true;
    }
    [
        "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte", "md", "txt", "json",
        "jsonc", "yaml", "yml", "toml", "css", "scss", "sass", "less", "html", "htm", "py",
        "go", "java", "kt", "kts", "swift", "rb", "php", "sh", "bash", "zsh", "sql",
        "graphql", "gql", "xml", "c", "cc", "cpp", "h", "hpp", "cs", "lock",
    ]
    .contains(&path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase().as_str())
}

fn source_kind(path: &str, mandatory: bool) -> &'static str {
    if path == "AGENTS.md" || path.ends_with("/AGENTS.md") || path == ".agentic/README.md" {
        "router"
    } else if path == ".agentic/manifest.yaml" || path == "agentic.yaml" {
        "manifest"
    } else if path.starts_with(".agentic/policies/") {
        "policy"
    } else if mandatory {
        "project-truth"
    } else if path.starts_with(".agentic/") {
        "project-context"
    } else {
        "repository-file"
    }
}

fn item(c: &Candidate, disposition: &str, reason: &str) -> Value {
    let mut reasons = vec![reason];
    if c.path_hits > 0 {
        reasons.push("task-term-in-path");
    }
    if c.content_hits > 0 {
        reasons.push("task-term-in-content");
    }
    let authority = if c.source_kind == "policy" {
        "mandatory-policy"
    } else if c.mandatory {
        "project-truth"
    } else {
        "repository-evidence"
    };
    json!({
        "id": c.path,
        "source": {"kind": c.source_kind, "path": c.path, "digest": c.digest, "revision": null},
        "authority": authority,
        "mandatory": c.mandatory,
        "stability": if c.mandatory { "stable" } else { "volatile" },
        "disclosure": if c.source_kind == "router" { "router" } else { "file" },
        "disposition": disposition,
        "estimated_tokens": c.tokens,
        "signals": {
            "relevance": {"score": c.relevance, "method": "lexical-hits-v2"},
            "confidence": null, "freshness": "current", "dependency_distance": null
        },
        "context_packs": [],
        "reasons": reasons
    })
}

pub(crate) fn plan(root: &Path, task: &str, max_tokens: usize) -> Value {
    if !valid_text(task, 16_384) || !valid_text(&root.to_string_lossy(), 16_384) {
        crate::fail("context task/target must be nonempty bounded text without control characters");
    }
    let terms = words(task);
    if terms.is_empty() {
        crate::fail("context task has no searchable terms");
    }
    let inventory = crate::scan::inventory(root);
    if inventory.files.len() > 200_000 {
        crate::fail("context inventory exceeds 200000 files");
    }
    let mut required = requirements::collect(root, &inventory.files);
    let mut errors = required.errors.clone();
    if !inventory.errors.is_empty() {
        errors.insert("repository-scan-incomplete".into());
    }
    let mut paths = inventory.files.clone();
    paths.sort_by_key(|p| (!required.paths.contains(&rel(root, p)), rel(root, p)));
    let mut candidates = Vec::new();
    let mut unsupported = 0;
    let mut unreadable = 0;
    let mut read_bytes = 0usize;
    for path in paths {
        let relative = rel(root, &path);
        let mandatory = required.paths.contains(&relative);
        if !valid_text(&relative, 4096) || path.to_str().is_none() || sensitive(&relative) {
            unsupported += 1;
            if mandatory {
                required.unavailable.insert(relative);
                errors.insert("required-context-path-disallowed".into());
            }
            continue;
        }
        if !mandatory && !supported(&path) {
            unsupported += 1;
            continue;
        }
        let limit = if mandatory { REQUIRED_READ_LIMIT } else { OPTIONAL_READ_LIMIT };
        let remaining = TOTAL_READ_LIMIT.saturating_sub(read_bytes);
        let text = crate::scan::read(root, &path, limit.min(remaining as u64));
        let text = match text {
            Ok(text) if !text.contains('\0') => text,
            _ => {
                unreadable += 1;
                errors.insert(format!("unreadable-or-bounded-text:{relative}"));
                if mandatory {
                    required.unavailable.insert(relative);
                }
                continue;
            }
        };
        read_bytes += text.len();
        let path_hits = terms.intersection(&words(&relative)).count();
        let content_hits = terms.intersection(&words(&text)).count();
        candidates.push(Candidate {
            source_kind: source_kind(&relative, mandatory),
            path: relative,
            digest: format!("sha256:{:x}", Sha256::digest(text.as_bytes())),
            mandatory,
            relevance: path_hits as u64 * 20 + content_hits as u64 * 5,
            tokens: estimate(&text),
            path_hits,
            content_hits,
        });
    }
    // Density is an explicit heuristic, not a quality probability or optimal knapsack solver.
    candidates.sort_by(|a, b| {
        b.mandatory.cmp(&a.mandatory)
            .then_with(|| {
                (b.relevance as u128 * a.tokens as u128)
                    .cmp(&(a.relevance as u128 * b.tokens as u128))
            })
            .then_with(|| b.relevance.cmp(&a.relevance))
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut selected = 0usize;
    let mut items = Vec::new();
    for candidate in &candidates {
        let (disposition, reason) = if candidate.mandatory {
            ("included", "mandatory-project-context")
        } else if candidate.relevance == 0 {
            ("deferred", "low-relevance")
        } else if selected.saturating_add(candidate.tokens) <= max_tokens {
            ("included", "within-budget")
        } else {
            ("deferred", "budget")
        };
        if disposition == "included" {
            selected = selected.saturating_add(candidate.tokens);
        }
        items.push(item(candidate, disposition, reason));
    }
    let schema_digest = format!("sha256:{:x}", Sha256::digest(SCHEMA.as_bytes()));
    json!({
        "format_version": 1,
        "kind": "compiled-context-plan",
        "target": root,
        "task": {"text": task, "intent": null},
        "compiler": {
            "id": "agentic-harness-cli/context-compiler",
            "version": format!("{}+lexical-density-v2+{}", env!("CARGO_PKG_VERSION"), schema_digest),
            "deterministic": true
        },
        "budget": {
            "estimator": {
                "id": "characters-div-4", "version": "1", "unit": "estimated-tokens",
                "disclaimer": "Source-text characters/4 only; excludes task, framing and report overhead; not provider billing."
            },
            "input": {"limit": max_tokens, "estimated": selected},
            "tool_output": {"limit": null, "estimated": null},
            "output": {"limit": null, "estimated": null},
            "over_budget": selected > max_tokens
        },
        "active_context_packs": [],
        "items": items,
        "coverage": {
            "complete": errors.is_empty() && required.unavailable.is_empty(),
            "files_discovered": inventory.files.len(),
            "supported_text_files": candidates.len(),
            "unsupported_files": unsupported,
            "unreadable_files": unreadable,
            "required_unavailable": required.unavailable.len(),
            "errors": errors.into_iter().take(1024).map(|message| {
                message.chars().map(|c| if c.is_control() { ' ' } else { c }).take(16_384).collect::<String>()
            }).collect::<Vec<_>>()
        },
        "not_checked": [
            "Host context injection and automatic Jev routing",
            "Symbol/dependency analysis, semantic sufficiency and active-pack selection",
            "Ignored undeclared context and semantic instruction precedence",
            "Provider tokenizer, billed usage, task/framing/report overhead and model quality",
            "Concurrent source changes after bounded reads; digests identify only observed bytes",
            "Complete secret detection; sensitive-path exclusion is conservative, not a public-export guarantee"
        ]
    })
}

pub(crate) fn default_max_tokens() -> usize {
    18_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_styles_have_matching_task_terms() {
        for source in ["validate_project", "validate-project", "validateProject", "ValidateProject"] {
            assert_eq!(words(source), words("validate project"));
        }
        assert!(words("XMLParser").contains("parser"));
    }

    #[test]
    fn estimates_are_not_provider_token_counts() {
        assert_eq!(estimate("12345"), 2);
        assert_eq!(estimate(""), 1);
        assert_eq!(estimate("日本語"), 1);
    }
}
