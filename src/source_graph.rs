use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Support {
    Supported,
    Partial,
    Unsupported,
}

impl Support {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Partial => "partial",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capability {
    pub name: &'static str,
    pub support: Support,
    pub note: Option<&'static str>,
}

pub trait LanguageFrontend {
    fn language(&self) -> &'static str;
    fn implementation(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn capabilities(&self) -> &'static [Capability];

    fn accepts(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| self.extensions().contains(&extension))
    }

    fn descriptor(&self) -> Value {
        json!({
            "language": self.language(),
            "implementation": self.implementation(),
            "version": self.version(),
            "extensions": self.extensions(),
            "capabilities": self.capabilities().iter().map(|capability| json!({
                "capability": capability.name,
                "support": capability.support.as_str(),
                "note": capability.note,
            })).collect::<Vec<_>>(),
        })
    }
}

pub struct JsTsFrontend;

const JS_TS_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "vue", "svelte",
];

const JS_TS_CAPABILITIES: &[Capability] = &[
    Capability {
        name: "parse",
        support: Support::Supported,
        note: Some(
            "Oxc parses JS/TS/JSX/TSX; Vue/Svelte script blocks are extracted before parsing",
        ),
    },
    Capability {
        name: "imports",
        support: Support::Supported,
        note: None,
    },
    Capability {
        name: "packages",
        support: Support::Partial,
        note: Some("package.json workspace/package exports and selected aliases are resolved"),
    },
    Capability {
        name: "type-edges",
        support: Support::Supported,
        note: Some("type-only imports remain distinct from runtime edges"),
    },
    Capability {
        name: "dynamic-edges",
        support: Support::Partial,
        note: Some(
            "static dynamic-import targets are extracted; computed targets are not resolved",
        ),
    },
    Capability {
        name: "workspace-resolution",
        support: Support::Partial,
        note: Some(
            "basic workspace package exports are resolved; full toolchain resolution is not claimed",
        ),
    },
    Capability {
        name: "framework-extraction",
        support: Support::Supported,
        note: Some("Vue and Svelte script blocks are supported"),
    },
];

impl LanguageFrontend for JsTsFrontend {
    fn language(&self) -> &'static str {
        "javascript-typescript"
    }

    fn implementation(&self) -> &'static str {
        "oxc"
    }

    fn version(&self) -> &'static str {
        "0.139.0"
    }

    fn extensions(&self) -> &'static [&'static str] {
        JS_TS_EXTENSIONS
    }

    fn capabilities(&self) -> &'static [Capability] {
        JS_TS_CAPABILITIES
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlannedLanguage {
    Python,
    Rust,
    Go,
}

impl PlannedLanguage {
    pub fn id(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Python => &["py"],
            Self::Rust => &["rs"],
            Self::Go => &["go"],
        }
    }

    pub fn capability_descriptor(self) -> Value {
        json!({
            "language": self.id(),
            "implementation": "not-implemented",
            "version": "0",
            "extensions": self.extensions(),
            "capabilities": [
                {"capability":"parse","support":"unsupported","note":"frontend not implemented"},
                {"capability":"imports","support":"unsupported","note":"frontend not implemented"},
                {"capability":"packages","support":"unsupported","note":"frontend not implemented"}
            ]
        })
    }
}

pub fn frontend_for(path: &Path) -> Option<Box<dyn LanguageFrontend>> {
    let frontend = JsTsFrontend;
    frontend
        .accepts(path)
        .then(|| Box::new(frontend) as Box<dyn LanguageFrontend>)
}

pub fn support_matrix() -> Value {
    let js = JsTsFrontend;
    json!({
        "format_version": 1,
        "kind": "language-support-matrix",
        "frontends": [
            js.descriptor(),
            PlannedLanguage::Python.capability_descriptor(),
            PlannedLanguage::Rust.capability_descriptor(),
            PlannedLanguage::Go.capability_descriptor()
        ]
    })
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn file_id(path: &str) -> String {
    format!("file:{:x}", Sha256::digest(path.as_bytes()))
}

fn source_snapshot(
    root: &Path,
    frontend: &dyn LanguageFrontend,
) -> (Vec<(PathBuf, String)>, String, BTreeMap<String, String>) {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut files: Vec<_> = crate::scan::files(&canonical_root)
        .into_iter()
        .filter(|path| frontend.accepts(path) && crate::scan::product(&canonical_root, path))
        .collect();
    files.sort_by_key(|path| relative(&canonical_root, path));

    let mut hasher = Sha256::new();
    let mut readable = Vec::new();
    let mut failures = BTreeMap::new();
    for path in files {
        let rel = relative(&canonical_root, &path);
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        match crate::scan::read(&canonical_root, &path, 2_000_000) {
            Ok(text) => {
                hasher.update(text.as_bytes());
                readable.push((path, rel));
            }
            Err(reason) => {
                hasher.update(b"<unreadable>");
                hasher.update(reason.as_bytes());
                failures.insert(rel, reason);
            }
        }
        hasher.update([0xff]);
    }
    (readable, format!("sha256:{:x}", hasher.finalize()), failures)
}

pub fn from_architecture_analysis(root: &Path, analysis: &Value) -> Result<Value, String> {
    let frontend = JsTsFrontend;
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let (readable, source_digest, mut file_failures) = source_snapshot(&canonical_root, &frontend);
    let all_files: Vec<_> = crate::scan::files(&canonical_root)
        .into_iter()
        .filter(|path| frontend.accepts(path) && crate::scan::product(&canonical_root, path))
        .collect();

    let mut ids = BTreeMap::new();
    let mut nodes = Vec::new();
    for path in &all_files {
        let rel = relative(&canonical_root, path);
        let id = file_id(&rel);
        ids.insert(rel.clone(), id.clone());
        nodes.push(json!({
            "id": id,
            "path": rel,
            "language": frontend.language(),
            "kind": "file",
            "generated": false,
            "metadata": {}
        }));
    }
    nodes.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));

    let mut edges = Vec::new();
    for edge in analysis["graph"]["edges"]
        .as_array()
        .ok_or_else(|| "architecture analysis graph.edges is missing".to_string())?
    {
        let from_path = edge["from"]
            .as_str()
            .ok_or_else(|| "architecture edge.from is missing".to_string())?;
        let to_path = edge["to"]
            .as_str()
            .ok_or_else(|| "architecture edge.to is missing".to_string())?;
        let from = ids
            .get(from_path)
            .ok_or_else(|| format!("architecture edge source is outside source snapshot: {from_path}"))?;
        let to = ids
            .get(to_path)
            .ok_or_else(|| format!("architecture edge target is outside source snapshot: {to_path}"))?;
        edges.push(json!({
            "from": from,
            "to": to,
            "kind": edge["kind"],
            "resolution": "local",
            "specifier": edge["specifier"],
            "line": edge["line"]
        }));
    }

    let unresolved = analysis["graph"]["unresolved_local_imports"]
        .as_array()
        .ok_or_else(|| "architecture unresolved-local-import evidence is missing".to_string())?;
    for item in unresolved {
        let Some(from_path) = item["from"].as_str() else {
            continue;
        };
        if let Some(specifier) = item["specifier"].as_str() {
            let from = ids.get(from_path).ok_or_else(|| {
                format!("unresolved architecture edge source is outside source snapshot: {from_path}")
            })?;
            edges.push(json!({
                "from": from,
                "to": null,
                "kind": "unknown",
                "resolution": "unresolved",
                "specifier": specifier,
                "line": item["line"].as_u64()
            }));
        } else {
            let reason = item["reason"]
                .as_str()
                .unwrap_or("source parsing or reading was incomplete");
            file_failures
                .entry(from_path.to_string())
                .or_insert_with(|| reason.to_string());
        }
    }

    edges.sort_by(|left, right| {
        (
            left["from"].as_str().unwrap_or_default(),
            left["line"].as_u64().unwrap_or_default(),
            left["specifier"].as_str().unwrap_or_default(),
        )
            .cmp(&(
                right["from"].as_str().unwrap_or_default(),
                right["line"].as_u64().unwrap_or_default(),
                right["specifier"].as_str().unwrap_or_default(),
            ))
    });

    let unresolved_edges = edges
        .iter()
        .filter(|edge| edge["resolution"] == "unresolved")
        .count();
    let resolved_edges = edges.len().saturating_sub(unresolved_edges);
    let discovered = all_files.len();
    let failed = file_failures.len().min(discovered);
    let parsed = discovered.saturating_sub(failed);
    let external_imports = analysis["graph"]["external_imports"].as_u64().unwrap_or(0);
    let resource_imports = analysis["graph"]["resource_imports"].as_u64().unwrap_or(0);
    let scan_errors = analysis["scan"]["errors"]
        .as_array()
        .map(|errors| !errors.is_empty())
        .unwrap_or(true);

    let mut not_checked = analysis["checks"]["not_checked"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if external_imports > 0 {
        not_checked.insert(
            "legacy JS/TS analysis retained external-import counts but not per-edge provenance; canonical external edges are not reconstructed"
                .to_string(),
        );
    }
    if resource_imports > 0 {
        not_checked.insert(
            "legacy JS/TS analysis retained resource-import counts but not per-edge provenance; canonical resource edges are not reconstructed"
                .to_string(),
        );
    }
    if !file_failures.is_empty() {
        not_checked.insert(format!(
            "{} in-scope source file(s) had incomplete read or parse evidence",
            file_failures.len()
        ));
    }
    let complete = failed == 0
        && unresolved_edges == 0
        && external_imports == 0
        && resource_imports == 0
        && !scan_errors;

    let _ = readable;
    Ok(json!({
        "format_version": 1,
        "kind": "source-graph",
        "source_digest": source_digest,
        "frontends": [frontend.descriptor()],
        "nodes": nodes,
        "edges": edges,
        "coverage": {
            "files_discovered": discovered,
            "files_parsed": parsed,
            "files_failed": failed,
            "edges_resolved": resolved_edges,
            "edges_unresolved": unresolved_edges,
            "complete": complete
        },
        "not_checked": not_checked.into_iter().collect::<Vec<_>>()
    }))
}

pub fn analyze_js_ts(root: &Path) -> Result<Value, String> {
    let analysis = crate::architecture_analysis::analyze(root, &[]);
    from_architecture_analysis(root, &analysis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn routes_existing_js_ts_family_to_oxc() {
        for path in [
            "src/a.ts",
            "src/a.tsx",
            "src/a.js",
            "src/A.vue",
            "src/A.svelte",
        ] {
            let frontend = frontend_for(Path::new(path)).expect(path);
            assert_eq!(frontend.implementation(), "oxc");
        }
        assert!(frontend_for(Path::new("src/main.py")).is_none());
    }

    #[test]
    fn preserves_partial_capabilities_instead_of_overclaiming() {
        let frontend = JsTsFrontend;
        let descriptor = frontend.descriptor();
        assert_eq!(descriptor["language"], "javascript-typescript");
        let capabilities = descriptor["capabilities"].as_array().unwrap();
        assert!(
            capabilities
                .iter()
                .any(|value| value["capability"] == "workspace-resolution"
                    && value["support"] == "partial")
        );
        assert!(capabilities.iter().any(
            |value| value["capability"] == "type-edges" && value["support"] == "supported"
        ));
    }

    #[test]
    fn planned_languages_are_explicitly_unsupported() {
        let matrix = support_matrix();
        for language in ["python", "rust", "go"] {
            let frontend = matrix["frontends"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["language"] == language)
                .unwrap();
            assert!(
                frontend["capabilities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|capability| capability["support"] == "unsupported")
            );
        }
    }

    #[test]
    fn conversion_keeps_isolated_files_and_local_edge_semantics() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/main.ts"), "import './service';\n").unwrap();
        fs::write(root.path().join("src/service.ts"), "export const value = 1;\n").unwrap();
        fs::write(root.path().join("src/isolated.ts"), "export const isolated = true;\n").unwrap();
        let analysis = json!({
            "graph": {
                "edges": [{"from":"src/main.ts","to":"src/service.ts","line":1,"specifier":"./service","kind":"runtime"}],
                "unresolved_local_imports": [],
                "external_imports": 0,
                "resource_imports": 0
            },
            "scan": {"errors": []},
            "checks": {"not_checked": ["compiler-grade type semantics"]}
        });
        let graph = from_architecture_analysis(root.path(), &analysis).unwrap();
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(graph["edges"].as_array().unwrap().len(), 1);
        assert_eq!(graph["edges"][0]["kind"], "runtime");
        assert_eq!(graph["coverage"]["files_discovered"], 3);
        assert_eq!(graph["coverage"]["edges_resolved"], 1);
        assert_eq!(graph["coverage"]["complete"], true);
        assert!(graph["source_digest"].as_str().unwrap().starts_with("sha256:"));
    }

    #[test]
    fn unresolved_legacy_edge_is_unknown_and_never_complete() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/main.ts"), "import './missing';\n").unwrap();
        let analysis = json!({
            "graph": {
                "edges": [],
                "unresolved_local_imports": [{"from":"src/main.ts","line":1,"specifier":"./missing"}],
                "external_imports": 0,
                "resource_imports": 0
            },
            "scan": {"errors": []},
            "checks": {"not_checked": []}
        });
        let graph = from_architecture_analysis(root.path(), &analysis).unwrap();
        assert_eq!(graph["edges"][0]["kind"], "unknown");
        assert_eq!(graph["edges"][0]["resolution"], "unresolved");
        assert!(graph["edges"][0]["to"].is_null());
        assert_eq!(graph["coverage"]["edges_unresolved"], 1);
        assert_eq!(graph["coverage"]["complete"], false);
    }

    #[test]
    fn legacy_external_counts_force_incomplete_export() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/main.ts"), "import 'vue';\n").unwrap();
        let analysis = json!({
            "graph": {
                "edges": [],
                "unresolved_local_imports": [],
                "external_imports": 1,
                "resource_imports": 0
            },
            "scan": {"errors": []},
            "checks": {"not_checked": []}
        });
        let graph = from_architecture_analysis(root.path(), &analysis).unwrap();
        assert_eq!(graph["coverage"]["complete"], false);
        assert!(graph["not_checked"].as_array().unwrap().iter().any(|value| value.as_str().unwrap().contains("external-import counts")));
    }
}
