use crate::architecture;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "vue"];
const SKIP_DIRECTORIES: &[&str] = &[
    ".git",
    "node_modules",
    "vendor",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "target",
    "coverage",
    "upstream",
];

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Edge {
    from: String,
    to: String,
    line: usize,
    specifier: String,
}

impl Edge {
    fn value(&self) -> Value {
        json!({
            "from": self.from,
            "to": self.to,
            "line": self.line,
            "specifier": self.specifier,
        })
    }
}

#[derive(Debug)]
enum Resolution {
    Local(PathBuf),
    UnresolvedLocal,
    External,
}

pub fn known_profile(profile: &str) -> bool {
    matches!(
        profile,
        "pattern/dependency-hygiene/1"
            | "pattern/feature-first/1"
            | "pattern/layered/1"
            | "framework/nuxt/4"
    )
}

fn is_source(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension))
}

fn source_files(root: &Path) -> Vec<PathBuf> {
    fn walk(path: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if path.is_dir() {
                if !SKIP_DIRECTORIES.contains(&name) {
                    walk(&path, out);
                }
            } else if is_source(&path) {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    files
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(value) => result.push(value),
            Component::RootDir => result.push(Path::new("/")),
            Component::Prefix(value) => result.push(value.as_os_str()),
        }
    }
    result
}

fn first_quoted(input: &str) -> Option<String> {
    let single = input.find('\'');
    let double = input.find('"');
    let (start, quote) = match (single, double) {
        (Some(a), Some(b)) if a < b => (a, '\''),
        (Some(_), Some(b)) => (b, '"'),
        (Some(a), None) => (a, '\''),
        (None, Some(b)) => (b, '"'),
        (None, None) => return None,
    };
    let rest = &input[start + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn marker_specifiers(line: &str, marker: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut rest = line;
    while let Some(index) = rest.find(marker) {
        let after = &rest[index + marker.len()..];
        if let Some(value) = first_quoted(after) {
            result.push(value);
        }
        rest = after;
    }
    result
}

fn import_specifiers(text: &str) -> Vec<(usize, String)> {
    let mut imports = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
            continue;
        }
        let mut line_imports = BTreeSet::new();
        if let Some(position) = line.rfind(" from ")
            && let Some(value) = first_quoted(&line[position + 6..])
        {
            line_imports.insert(value);
        } else if (trimmed.starts_with("import ") || trimmed.starts_with("export "))
            && let Some(value) = first_quoted(trimmed)
        {
            line_imports.insert(value);
        }
        for marker in ["require(", "import("] {
            line_imports.extend(marker_specifiers(line, marker));
        }
        imports.extend(
            line_imports
                .into_iter()
                .map(|specifier| (index + 1, specifier)),
        );
    }
    imports
}

fn clean_specifier(specifier: &str) -> &str {
    specifier
        .split(['?', '#'])
        .next()
        .unwrap_or(specifier)
}

fn existing_source(base: &Path) -> Option<PathBuf> {
    if base.is_file() && is_source(base) {
        return Some(base.to_path_buf());
    }
    if base.extension().is_none() {
        for extension in SOURCE_EXTENSIONS {
            let mut candidate = base.to_path_buf();
            candidate.set_extension(extension);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for extension in SOURCE_EXTENSIONS {
        let candidate = base.join(format!("index.{extension}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_import(root: &Path, from: &Path, specifier: &str) -> Resolution {
    let specifier = clean_specifier(specifier);
    let base = if specifier.starts_with("./") || specifier.starts_with("../") {
        from.parent().unwrap_or(root).join(specifier)
    } else if let Some(rest) = specifier.strip_prefix("@/") {
        root.join("src").join(rest)
    } else if let Some(rest) = specifier.strip_prefix("~/") {
        if root.join("app").is_dir() {
            root.join("app").join(rest)
        } else {
            root.join(rest)
        }
    } else if let Some(rest) = specifier.strip_prefix("#shared/") {
        root.join("shared").join(rest)
    } else if let Some(rest) = specifier.strip_prefix('/') {
        root.join(rest)
    } else {
        return Resolution::External;
    };

    let base = lexical_normalize(&base);
    let Some(candidate) = existing_source(&base) else {
        return Resolution::UnresolvedLocal;
    };
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let canonical_candidate = candidate.canonicalize().unwrap_or(candidate);
    if canonical_candidate.starts_with(&canonical_root) {
        Resolution::Local(canonical_candidate)
    } else {
        Resolution::UnresolvedLocal
    }
}

fn build_graph(root: &Path) -> (BTreeSet<String>, Vec<Edge>, Vec<Value>, usize) {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let files = source_files(&canonical_root);
    let mut nodes = BTreeSet::new();
    let mut edges = BTreeSet::new();
    let mut unresolved = Vec::new();
    let mut external_imports = 0usize;

    for path in files {
        let from = relative(&canonical_root, &path);
        nodes.insert(from.clone());
        let text = fs::read_to_string(&path).unwrap_or_default();
        for (line, specifier) in import_specifiers(&text) {
            match resolve_import(&canonical_root, &path, &specifier) {
                Resolution::Local(target) => {
                    let to = relative(&canonical_root, &target);
                    nodes.insert(to.clone());
                    edges.insert(Edge {
                        from: from.clone(),
                        to,
                        line,
                        specifier,
                    });
                }
                Resolution::UnresolvedLocal => unresolved.push(json!({
                    "from": from,
                    "line": line,
                    "specifier": specifier,
                })),
                Resolution::External => external_imports += 1,
            }
        }
    }

    (nodes, edges.into_iter().collect(), unresolved, external_imports)
}

fn adjacency(nodes: &BTreeSet<String>, edges: &[Edge], reverse: bool) -> BTreeMap<String, Vec<String>> {
    let mut graph = BTreeMap::new();
    for node in nodes {
        graph.insert(node.clone(), Vec::new());
    }
    for edge in edges {
        let (from, to) = if reverse {
            (&edge.to, &edge.from)
        } else {
            (&edge.from, &edge.to)
        };
        graph.entry(from.clone()).or_default().push(to.clone());
    }
    for values in graph.values_mut() {
        values.sort();
        values.dedup();
    }
    graph
}

fn visit_order(
    node: &str,
    graph: &BTreeMap<String, Vec<String>>,
    visited: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) {
    if !visited.insert(node.to_string()) {
        return;
    }
    if let Some(next) = graph.get(node) {
        for child in next {
            visit_order(child, graph, visited, order);
        }
    }
    order.push(node.to_string());
}

fn collect_component(
    node: &str,
    graph: &BTreeMap<String, Vec<String>>,
    visited: &mut BTreeSet<String>,
    component: &mut Vec<String>,
) {
    if !visited.insert(node.to_string()) {
        return;
    }
    component.push(node.to_string());
    if let Some(next) = graph.get(node) {
        for child in next {
            collect_component(child, graph, visited, component);
        }
    }
}

fn cycles(nodes: &BTreeSet<String>, edges: &[Edge]) -> Vec<Vec<String>> {
    let forward = adjacency(nodes, edges, false);
    let reverse = adjacency(nodes, edges, true);
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for node in nodes {
        visit_order(node, &forward, &mut visited, &mut order);
    }

    let self_edges: BTreeSet<_> = edges
        .iter()
        .filter(|edge| edge.from == edge.to)
        .map(|edge| edge.from.clone())
        .collect();
    visited.clear();
    let mut result = Vec::new();
    for node in order.into_iter().rev() {
        if visited.contains(&node) {
            continue;
        }
        let mut component = Vec::new();
        collect_component(&node, &reverse, &mut visited, &mut component);
        component.sort();
        if component.len() > 1 || self_edges.contains(&node) {
            result.push(component);
        }
    }
    result.sort();
    result
}

fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|part| !part.is_empty()).collect()
}

fn root_role<'a>(path: &'a str, roles: &[&str]) -> Option<&'a str> {
    let parts = segments(path);
    let first = parts.first().copied()?;
    if roles.contains(&first) {
        return Some(first);
    }
    if first == "src" {
        let second = parts.get(1).copied()?;
        if roles.contains(&second) {
            return Some(second);
        }
    }
    None
}

fn feature_owner(path: &str) -> Option<(&str, &str, usize)> {
    let parts = segments(path);
    for (index, part) in parts.iter().enumerate() {
        if ["features", "domains", "modules"].contains(part)
            && let Some(owner) = parts.get(index + 1)
        {
            return Some((part, owner, index));
        }
    }
    None
}

fn public_feature_target(path: &str) -> bool {
    let parts = segments(path);
    let Some((_, _, index)) = feature_owner(path) else {
        return false;
    };
    if parts.len() != index + 3 {
        return false;
    }
    matches!(
        parts[index + 2],
        "index.ts" | "index.js" | "index.mts" | "index.mjs" | "public.ts" | "public.js"
    )
}

fn edge_finding(
    rule_id: &str,
    severity: &str,
    authority: &str,
    enforceability: &str,
    profile: &str,
    message: String,
    edge: &Edge,
) -> Value {
    json!({
        "rule_id": rule_id,
        "severity": severity,
        "authority": authority,
        "enforceability": enforceability,
        "profile": profile,
        "message": message,
        "evidence": edge.value(),
    })
}

fn contains_profile(profiles: &BTreeSet<String>, profile: &str) -> bool {
    profiles.contains(profile)
}

fn selected_profiles(detection: &Value, requested: &[String]) -> BTreeSet<String> {
    let mut profiles = BTreeSet::new();
    profiles.insert("pattern/dependency-hygiene/1".to_string());
    if requested.is_empty() {
        if let Some(candidates) = detection.get("candidate_profiles").and_then(Value::as_array) {
            profiles.extend(candidates.iter().filter_map(Value::as_str).filter(|profile| known_profile(profile)).map(str::to_string));
        }
        if detection["frameworks"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["name"] == "nuxt"))
        {
            profiles.insert("framework/nuxt/4".to_string());
        }
        if detection["structure"]["feature_buckets"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            profiles.insert("pattern/feature-first/1".to_string());
        }
        if detection["structure"]["layer_buckets"]
            .as_array()
            .is_some_and(|items| items.len() >= 3)
        {
            profiles.insert("pattern/layered/1".to_string());
        }
    } else {
        profiles.extend(requested.iter().cloned());
    }
    profiles
}

fn boundary_findings(edges: &[Edge], profiles: &BTreeSet<String>) -> Vec<Value> {
    let mut findings = Vec::new();
    for edge in edges {
        if contains_profile(profiles, "framework/nuxt/4") {
            let from = root_role(&edge.from, &["app", "server", "shared"]);
            let to = root_role(&edge.to, &["app", "server", "shared"]);
            if matches!((from, to), (Some("app"), Some("server")) | (Some("server"), Some("app"))) {
                findings.push(edge_finding(
                    "nuxt.boundary.app-server",
                    "error",
                    "official",
                    "deterministic",
                    "framework/nuxt/4",
                    "Nuxt app and server code must not import each other directly.".to_string(),
                    edge,
                ));
            }
            if from == Some("shared") && matches!(to, Some("app" | "server")) {
                findings.push(edge_finding(
                    "nuxt.shared.neutral-boundary",
                    "error",
                    "official",
                    "deterministic",
                    "framework/nuxt/4",
                    "Nuxt shared code must remain runtime-neutral and cannot import app/server code.".to_string(),
                    edge,
                ));
            }
        }

        if contains_profile(profiles, "pattern/feature-first/1") {
            let from_feature = feature_owner(&edge.from);
            let to_feature = feature_owner(&edge.to);
            if let (Some((_, from_owner, _)), Some((_, to_owner, _))) = (from_feature, to_feature)
                && from_owner != to_owner
                && !public_feature_target(&edge.to)
            {
                findings.push(edge_finding(
                    "feature-first.no-cross-feature-internals",
                    "error",
                    "harness",
                    "deterministic",
                    "pattern/feature-first/1",
                    format!("Feature `{from_owner}` imports internal code from feature `{to_owner}` instead of its public interface."),
                    edge,
                ));
            }
            let from_shared = root_role(&edge.from, &["shared", "common"]);
            if from_shared.is_some() && to_feature.is_some() && !public_feature_target(&edge.to) {
                findings.push(edge_finding(
                    "feature-first.shared-not-dumping-ground",
                    "warning",
                    "harness",
                    "heuristic",
                    "pattern/feature-first/1",
                    "Shared/common code imports feature-owned internals, which makes shared code ownership-specific.".to_string(),
                    edge,
                ));
            }
        }

        if contains_profile(profiles, "pattern/layered/1") {
            let from = root_role(
                &edge.from,
                &["presentation", "application", "domain", "infrastructure"],
            );
            let to = root_role(
                &edge.to,
                &["presentation", "application", "domain", "infrastructure"],
            );
            let violation = match (from, to) {
                (Some("presentation"), Some("infrastructure")) => Some((
                    "layered.presentation-direction",
                    "Presentation code imports infrastructure implementation details.",
                )),
                (Some("application"), Some("presentation" | "infrastructure")) => Some((
                    "layered.application-direction",
                    "Application code imports an outer presentation/infrastructure layer.",
                )),
                (Some("domain"), Some("presentation" | "application" | "infrastructure")) => Some((
                    "layered.domain-independent",
                    "Domain code imports an outer application, presentation or infrastructure layer.",
                )),
                (Some("infrastructure"), Some("presentation" | "application")) => Some((
                    "layered.infrastructure-inward",
                    "Infrastructure code imports presentation/application orchestration instead of inward contracts.",
                )),
                _ => None,
            };
            if let Some((rule_id, message)) = violation {
                findings.push(edge_finding(
                    rule_id,
                    "error",
                    "harness",
                    "deterministic",
                    "pattern/layered/1",
                    message.to_string(),
                    edge,
                ));
            }
        }
    }
    findings
}

pub fn analyze(root: &Path, requested_profiles: &[String]) -> Value {
    let detection = architecture::detect(root);
    let profiles = selected_profiles(&detection, requested_profiles);
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let (nodes, edges, unresolved, external_imports) = build_graph(&canonical_root);
    let components = cycles(&nodes, &edges);
    let mut findings = boundary_findings(&edges, &profiles);

    if contains_profile(&profiles, "pattern/dependency-hygiene/1") {
        for component in &components {
            findings.push(json!({
                "rule_id": "dependency.no-import-cycles",
                "severity": "error",
                "authority": "harness",
                "enforceability": "deterministic",
                "profile": "pattern/dependency-hygiene/1",
                "message": format!("Import cycle contains {} local source modules.", component.len()),
                "evidence": {"modules": component},
            }));
        }
    }

    findings.sort_by(|a, b| {
        let left = (
            a["rule_id"].as_str().unwrap_or_default(),
            a["evidence"]["from"].as_str().unwrap_or_default(),
            a["evidence"]["line"].as_u64().unwrap_or_default(),
        );
        let right = (
            b["rule_id"].as_str().unwrap_or_default(),
            b["evidence"]["from"].as_str().unwrap_or_default(),
            b["evidence"]["line"].as_u64().unwrap_or_default(),
        );
        left.cmp(&right)
    });

    let deterministic_errors = findings
        .iter()
        .filter(|finding| finding["severity"] == "error" && finding["enforceability"] == "deterministic")
        .count();
    let warnings = findings
        .iter()
        .filter(|finding| finding["severity"] == "warning")
        .count();
    let edge_values: Vec<_> = edges.iter().map(Edge::value).collect();

    json!({
        "format_version": 1,
        "target": root.to_string_lossy(),
        "detection": detection,
        "profiles": profiles,
        "graph": {
            "source_files": nodes.len(),
            "local_edges": edge_values.len(),
            "external_imports": external_imports,
            "unresolved_local_imports": unresolved,
            "edges": edge_values,
            "cycles": components,
        },
        "compliance": {
            "passed": deterministic_errors == 0,
            "deterministic_errors": deterministic_errors,
            "warnings": warnings,
        },
        "findings": findings,
        "checks": {
            "performed": [
                "JS/TS/Vue local import extraction",
                "relative and supported alias resolution (@/, ~/, #shared/)",
                "local source dependency graph",
                "strongly-connected import cycle detection",
                "Nuxt app/server/shared boundaries when selected or detected",
                "feature-first cross-feature internal imports when selected or detected",
                "layered dependency direction when selected or detected"
            ],
            "not_checked": [
                "Python/Rust/Go/Java import graphs",
                "custom tsconfig/vite alias maps beyond built-in aliases",
                "runtime-generated or computed dynamic import targets",
                "semantic business-logic placement",
                "project-local rule exceptions/expiry",
                "external package dependency cycles"
            ]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn fixture(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "ah-architecture-analysis-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(root: &Path, path: &str, content: &str) {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(target, content).unwrap();
    }

    fn has_rule(result: &Value, rule: &str) -> bool {
        result["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["rule_id"] == rule)
    }

    #[test]
    fn detects_import_cycle() {
        let root = fixture("cycle");
        write(&root, "src/a.ts", "import { b } from './b'; export const a = b;");
        write(&root, "src/b.ts", "import { a } from './a'; export const b = a;");
        let result = analyze(&root, &[]);
        assert_eq!(result["graph"]["cycles"].as_array().unwrap().len(), 1);
        assert!(has_rule(&result, "dependency.no-import-cycles"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_nuxt_app_server_violation() {
        let root = fixture("nuxt-boundary");
        write(&root, "package.json", r#"{"dependencies":{"nuxt":"^4.0.0"}}"#);
        write(&root, "nuxt.config.ts", "export default defineNuxtConfig({});");
        write(&root, "app/pages/index.ts", "import { secret } from '../../server/utils/secret'; export { secret };");
        write(&root, "server/utils/secret.ts", "export const secret = 1;");
        let result = analyze(&root, &[]);
        assert!(has_rule(&result, "nuxt.boundary.app-server"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_cross_feature_internal_import() {
        let root = fixture("feature-boundary");
        write(&root, "src/features/auth/index.ts", "import { total } from '../billing/internal'; export { total };");
        write(&root, "src/features/billing/internal.ts", "export const total = 1;");
        let result = analyze(&root, &[]);
        assert!(has_rule(
            &result,
            "feature-first.no-cross-feature-internals"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn permits_cross_feature_public_interface() {
        let root = fixture("feature-public");
        write(&root, "src/features/auth/index.ts", "import { total } from '../billing'; export { total };");
        write(&root, "src/features/billing/index.ts", "export const total = 1;");
        let result = analyze(&root, &[]);
        assert!(!has_rule(
            &result,
            "feature-first.no-cross-feature-internals"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn detects_layer_direction_violation() {
        let root = fixture("layered");
        for layer in ["presentation", "application", "domain", "infrastructure"] {
            fs::create_dir_all(root.join("src").join(layer)).unwrap();
        }
        write(&root, "src/domain/model.ts", "import { db } from '../infrastructure/db'; export { db };");
        write(&root, "src/infrastructure/db.ts", "export const db = 1;");
        let result = analyze(&root, &[]);
        assert!(has_rule(&result, "layered.domain-independent"));
        let _ = fs::remove_dir_all(root);
    }
}
