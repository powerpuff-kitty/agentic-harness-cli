//! Pinned catalog-owned file selection. Existing targets are reconciled by compose.
use include_dir::Dir;
use serde_json::{Value, json};
use std::{fs, io, path::Path};

const CONTRACT: &str = include_str!("../upstream/agentic-harness/catalog/context/profiles.v1.json");
const MAP: &str = include_str!("../upstream/agentic-harness/catalog/context/minimal-map.md");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Profile {
    Full,
    Minimal,
}

impl Profile {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "full" => Ok(Self::Full),
            "minimal" => Ok(Self::Minimal),
            _ => Err("context profile must be full|minimal".into()),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Minimal => "minimal",
        }
    }

    pub fn from_manifest(manifest: &Value) -> Result<Self, String> {
        let Some(composition) = manifest.get("composition") else {
            return Ok(Self::Full);
        };
        let fields = composition
            .as_object()
            .ok_or("composition must be a mapping")?;
        if fields.len() != 1 {
            return Err("composition must contain only context_profile".into());
        }
        Self::parse(
            fields
                .get("context_profile")
                .and_then(Value::as_str)
                .ok_or("composition.context_profile must be a string")?,
        )
    }
}

fn copy_file(source: &Dir<'_>, target: &Path, relative: &str) -> io::Result<()> {
    if relative.contains(['\\', ':'])
        || !Path::new(relative)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        return Err(io::Error::other("invalid embedded context path"));
    }
    let file = source
        .get_file(relative)
        .ok_or_else(|| io::Error::other(format!("missing selected context: {relative}")))?;
    let destination = target.join(relative);
    fs::create_dir_all(destination.parent().unwrap())?;
    fs::write(destination, file.contents())
}

pub(crate) fn stage_minimal(
    source: &Dir<'_>,
    variant: &str,
    target: &Path,
    modules: &[(&str, Vec<String>)],
    variant_source: impl Fn(&str) -> &'static Dir<'static>,
) -> io::Result<()> {
    let contract: Value = serde_json::from_str(CONTRACT).map_err(io::Error::other)?;
    if contract["format_version"] != 1
        || contract["kind"] != "context-profiles"
        || contract["default"] != "full"
    {
        return Err(io::Error::other("unsupported embedded context contract"));
    }
    let minimal = &contract["minimal"];
    let list = |key: &str| -> io::Result<Vec<&str>> {
        minimal[key]
            .as_array()
            .ok_or_else(|| io::Error::other("invalid context selection list"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| io::Error::other("invalid context selection entry"))
            })
            .collect()
    };
    for relative in list("core_files")? {
        copy_file(source, target, relative)?;
    }
    let design_packs = list("design_packs")?;
    let design = list("design_variants")?.contains(&variant)
        || modules.iter().any(|(kind, names)| {
            *kind == "packs"
                && names
                    .iter()
                    .any(|name| design_packs.contains(&name.as_str()))
        });
    if design {
        let path = ".agentic/DESIGN.md";
        if source.get_file(path).is_some() {
            copy_file(source, target, path)?;
        } else {
            let fallback = minimal["design_fallback_variant"]
                .as_str()
                .ok_or_else(|| io::Error::other("missing design fallback"))?;
            copy_file(variant_source(fallback), target, path)?;
        }
    }
    for (kind, names) in modules {
        if !names.is_empty() {
            let path = minimal["module_routers"][kind]
                .as_str()
                .ok_or_else(|| io::Error::other("missing module router"))?;
            copy_file(source, target, path)?;
        }
    }
    Ok(())
}

pub(crate) fn configure_staged(root: &Path, profile: Profile) -> io::Result<()> {
    let path = root.join(".agentic/manifest.yaml");
    let mut manifest: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(&path)?).map_err(io::Error::other)?;
    manifest["composition"] = json!({"context_profile": profile.name()});
    if profile == Profile::Minimal {
        let routes = manifest["context"]
            .as_object_mut()
            .ok_or_else(|| io::Error::other("missing context routes"))?;
        // The staged manifest is a fresh template. Existing project routes are merged separately.
        for (key, route) in routes.iter_mut() {
            if !["product", "architecture", "security", "decisions"].contains(&key.as_str())
                && !route
                    .as_str()
                    .is_some_and(|relative| root.join(".agentic").join(relative).exists())
            {
                *route = Value::Null;
            }
        }
        if root.join(".agentic/DESIGN.md").is_file() {
            routes.insert("design".into(), json!("DESIGN.md"));
        }
        let descriptions = [
            (
                "product",
                "Changing behavior, scope or unresolved product decisions",
            ),
            (
                "architecture",
                "Changing components, dependencies or data flow",
            ),
            ("security", "Touching data, permissions or trust boundaries"),
            (
                "design",
                "Changing controls, visual intent or interaction states",
            ),
            (
                "decisions",
                "Reconsidering durable choices and their rationale",
            ),
        ];
        let rows: Vec<_> = descriptions
            .iter()
            .filter_map(|(key, description)| {
                routes
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(|route| format!("| `{route}` | {description} |"))
            })
            .collect();
        let modules: Vec<_> = [
            ("packs/", "A task touches an installed domain"),
            (
                "policies/",
                "Before consequential changes; mandatory installed rules",
            ),
            (
                "../.agents/skills/",
                "A selected procedure is relevant to the task",
            ),
        ]
        .iter()
        .filter(|(relative, _)| root.join(".agentic").join(relative).exists())
        .map(|(relative, description)| format!("| `{relative}` | {description} |"))
        .collect();
        fs::write(
            root.join(".agentic/README.md"),
            MAP.replace("{{routes}}", &rows.join("\n"))
                .replace("{{modules}}", &modules.join("\n")),
        )?;
    }
    fs::write(
        path,
        serde_yaml_ng::to_string(&manifest).map_err(io::Error::other)?,
    )
}
