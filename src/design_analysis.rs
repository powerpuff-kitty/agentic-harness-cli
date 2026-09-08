use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const SKIP: &[&str] = &[
    ".git", "node_modules", "vendor", "dist", "build", ".next", ".nuxt", "target", ".venv",
    "venv", "coverage", "upstream",
];

const TEXT_EXT: &[&str] = &[
    "css", "scss", "sass", "less", "html", "htm", "vue", "svelte", "js", "mjs", "cjs", "ts",
    "tsx", "jsx",
];

#[derive(Default)]
struct Frequency {
    total: usize,
    values: BTreeMap<String, usize>,
    paths: BTreeMap<String, BTreeMap<String, usize>>,
}

impl Frequency {
    fn add(&mut self, value: String, path: &str) {
        self.total += 1;
        *self.values.entry(value.clone()).or_default() += 1;
        *self.paths.entry(value).or_default().entry(path.to_string()).or_default() += 1;
    }

    fn rows(&self) -> Vec<Value> {
        self.values
            .iter()
            .map(|(value, count)| {
                let evidence = self
                    .paths
                    .get(value)
                    .into_iter()
                    .flat_map(|paths| paths.iter())
                    .map(|(path, occurrences)| json!({"path": path, "data": {"occurrences": occurrences}}))
                    .collect::<Vec<_>>();
                json!({"value": value, "count": count, "evidence": evidence})
            })
            .collect()
    }
}

fn files(root: &Path) -> Vec<PathBuf> {
    fn walk(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(path) else { return };
        let mut entries = entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(&path);
            if rel
                .components()
                .any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().as_ref()))
            {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else if path
                .extension()
                .and_then(|x| x.to_str())
                .map(|ext| TEXT_EXT.contains(&ext.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }

    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn hex_colors(line: &str) -> Vec<String> {
    let chars = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != b'#' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let digits = i;
        while i < chars.len() && chars[i].is_ascii_hexdigit() && i - digits < 8 {
            i += 1;
        }
        let len = i - digits;
        let boundary = i == chars.len() || !chars[i].is_ascii_hexdigit();
        if boundary && matches!(len, 3 | 6 | 8) {
            out.push(line[start..i].to_ascii_lowercase());
        }
        if i == start + 1 {
            i += 1;
        }
    }
    out
}

fn px_values(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 <= bytes.len() {
        if i + 1 < bytes.len() && bytes[i].eq_ignore_ascii_case(&b'p') && bytes[i + 1].eq_ignore_ascii_case(&b'x') {
            let mut start = i;
            while start > 0 {
                let c = bytes[start - 1];
                if c.is_ascii_digit() || c == b'.' || c == b'-' {
                    start -= 1;
                } else {
                    break;
                }
            }
            if start < i {
                let raw = line[start..i].trim();
                if raw.parse::<f64>().is_ok() {
                    out.push(format!("{raw}px"));
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn css_variable_names(line: &str) -> (Vec<String>, Vec<String>) {
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'-' && bytes[i + 1] == b'-' {
            let start = i;
            i += 2;
            while i < bytes.len() {
                let c = bytes[i];
                if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_') {
                    i += 1;
                } else {
                    break;
                }
            }
            if i > start + 2 {
                let name = line[start..i].to_string();
                let prefix = &line[..start];
                let suffix = &line[i..];
                if suffix.trim_start().starts_with(':') && !prefix.ends_with("var(") {
                    definitions.push(name);
                } else if prefix.ends_with("var(") || line[..start].trim_end().ends_with("var(") {
                    references.push(name);
                }
            }
        } else {
            i += 1;
        }
    }
    (definitions, references)
}

fn property_line(lower: &str, properties: &[&str]) -> bool {
    properties.iter().any(|property| lower.contains(property))
}

fn measurement(id: &str, metric: &str, value: Value) -> Value {
    json!({
        "id": id,
        "metric": metric,
        "value": value,
        "source_type": "static",
        "confidence": 1.0
    })
}

fn domain(rows: Vec<Value>, summary: Value) -> Value {
    json!({"measurements": rows, "summary": summary})
}

pub fn analyze_static(root: &Path) -> Value {
    let mut colors = Frequency::default();
    let mut font_sizes = Frequency::default();
    let mut spacing = Frequency::default();
    let mut radii = Frequency::default();
    let mut token_definitions = BTreeSet::new();
    let mut token_references = BTreeSet::new();
    let mut analyzed_files = Vec::new();

    for path in files(root) {
        let Ok(meta) = fs::metadata(&path) else { continue };
        if meta.len() > 2_000_000 {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        analyzed_files.push(rel.clone());

        for line in text.lines() {
            let lower = line.to_ascii_lowercase();

            if property_line(&lower, &["color", "background", "border", "fill", "stroke", "shadow"]) {
                for value in hex_colors(line) {
                    colors.add(value, &rel);
                }
            }

            if property_line(&lower, &["font-size"]) {
                for value in px_values(line) {
                    font_sizes.add(value, &rel);
                }
            }

            if property_line(&lower, &["margin", "padding", "gap", "space-"]) {
                for value in px_values(line) {
                    spacing.add(value, &rel);
                }
            }

            if property_line(&lower, &["border-radius", "radius"]) {
                for value in px_values(line) {
                    radii.add(value, &rel);
                }
            }

            let (definitions, references) = css_variable_names(line);
            token_definitions.extend(definitions);
            token_references.extend(references);
        }
    }

    let color_rows = colors.rows();
    let font_rows = font_sizes.rows();
    let spacing_rows = spacing.rows();
    let radius_rows = radii.rows();

    let mut findings = Vec::new();
    if colors.total > 0 {
        findings.push(json!({
            "id": "color.hex-values",
            "domain": "color",
            "classification": "observation",
            "severity": "info",
            "statement": format!("Detected {} hexadecimal color occurrences across {} unique values.", colors.total, colors.values.len()),
            "source_type": "static",
            "confidence": 1.0,
            "measurement_refs": ["color.hex-frequency"]
        }));
    }
    if !token_definitions.is_empty() && colors.total > 0 {
        findings.push(json!({
            "id": "tokens.mixed-color-usage",
            "domain": "tokens",
            "classification": "observation",
            "severity": "low",
            "statement": "CSS custom properties and literal hexadecimal colors are both present; review whether literal values intentionally bypass semantic tokens.",
            "source_type": "static",
            "confidence": 1.0,
            "measurement_refs": ["tokens.css-custom-properties", "color.hex-frequency"]
        }));
    }

    json!({
        "format_version": 1,
        "source": {
            "kind": "repository",
            "uri": root.to_string_lossy(),
            "analyzer": "agentic-harness-cli",
            "analyzer_version": env!("CARGO_PKG_VERSION")
        },
        "domains": {
            "color": domain(
                vec![measurement("color.hex-frequency", "hex-color-frequency", json!(color_rows))],
                json!({"occurrences": colors.total, "unique_values": colors.values.len()})
            ),
            "typography": domain(
                vec![measurement("typography.font-size-frequency", "font-size-px-frequency", json!(font_rows))],
                json!({"occurrences": font_sizes.total, "unique_values": font_sizes.values.len()})
            ),
            "spacing": domain(
                vec![measurement("spacing.px-frequency", "spacing-px-frequency", json!(spacing_rows))],
                json!({"occurrences": spacing.total, "unique_values": spacing.values.len()})
            ),
            "geometry": domain(
                vec![measurement("geometry.radius-frequency", "border-radius-px-frequency", json!(radius_rows))],
                json!({"occurrences": radii.total, "unique_values": radii.values.len()})
            ),
            "tokens": domain(
                vec![measurement(
                    "tokens.css-custom-properties",
                    "css-custom-properties",
                    json!({
                        "defined": token_definitions.iter().collect::<Vec<_>>(),
                        "referenced": token_references.iter().collect::<Vec<_>>()
                    })
                )],
                json!({
                    "defined": token_definitions.len(),
                    "referenced": token_references.len()
                })
            )
        },
        "findings": findings,
        "checks": {
            "performed": [
                "static.hex-colors",
                "static.font-size-px",
                "static.spacing-px",
                "static.border-radius-px",
                "static.css-custom-properties"
            ],
            "not_checked": [
                "runtime.computed-styles",
                "runtime.contrast",
                "runtime.responsive-layout",
                "runtime.accessibility",
                "visual.identity-quality",
                "visual.originality"
            ]
        },
        "metadata": {
            "analyzed_files": analyzed_files,
            "deterministic": true
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_analysis_is_sorted_and_deterministic() {
        let root = std::env::temp_dir().join(format!("ah-design-analysis-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("app.css"),
            ":root { --color-accent: #ff5500; --space-md: 16px; }\n.card { color: #ff5500; padding: 16px; border-radius: 4px; font-size: 14px; background: var(--color-accent); }\n",
        )
        .unwrap();

        let first = analyze_static(&root);
        let second = analyze_static(&root);
        assert_eq!(first, second);
        assert_eq!(first["format_version"], 1);
        assert_eq!(first["domains"]["color"]["summary"]["unique_values"], 1);
        assert_eq!(first["domains"]["spacing"]["summary"]["occurrences"], 1);
        assert!(first["domains"]["tokens"]["summary"]["defined"].as_u64().unwrap() >= 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_non_design_files_and_large_build_directories() {
        let root = std::env::temp_dir().join(format!("ah-design-skip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::write(root.join("README.md"), "color: #ffffff").unwrap();
        fs::write(root.join("node_modules/theme.css"), "color: #000000").unwrap();
        fs::write(root.join("app.css"), "color: #123456").unwrap();
        let report = analyze_static(&root);
        assert_eq!(report["domains"]["color"]["summary"]["unique_values"], 1);
        let _ = fs::remove_dir_all(root);
    }
}
