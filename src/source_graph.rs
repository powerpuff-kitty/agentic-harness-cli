use serde_json::{Value, json};
use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
