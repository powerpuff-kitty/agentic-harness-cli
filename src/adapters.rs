//! Context-only adapter installation. No replacement, removal, hooks or host execution.
use crate::check_inputs;
use serde_json::{Value, json};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

const INVENTORY: &[u8] = include_bytes!("../upstream/agentic-harness-agents/adapters/assets.json");
const LIMIT: usize = 65_536;

struct Asset {
    host: &'static str,
    profile: &'static str,
    source: &'static str,
    target: &'static str,
    bytes: &'static [u8],
}

const ASSETS: &[Asset] = &[
    Asset {
        host: "claude",
        profile: "base",
        source: "claude/files/CLAUDE.md",
        target: "CLAUDE.md",
        bytes: include_bytes!("../upstream/agentic-harness-agents/adapters/claude/files/CLAUDE.md"),
    },
    Asset {
        host: "claude",
        profile: "typed-ui",
        source: "claude/files/.claude/rules/agentic-typed-ui.md",
        target: ".claude/rules/agentic-typed-ui.md",
        bytes: include_bytes!("../upstream/agentic-harness-agents/adapters/claude/files/.claude/rules/agentic-typed-ui.md"),
    },
    Asset {
        host: "cursor",
        profile: "typed-ui",
        source: "cursor/files/.cursor/rules/agentic-typed-ui.mdc",
        target: ".cursor/rules/agentic-typed-ui.mdc",
        bytes: include_bytes!("../upstream/agentic-harness-agents/adapters/cursor/files/.cursor/rules/agentic-typed-ui.mdc"),
    },
];
const NOTICE: Asset = Asset {
    host: "",
    profile: "notice",
    source: "../LICENSE",
    target: ".agents/adapters/LICENSE",
    bytes: include_bytes!("../upstream/agentic-harness-agents/LICENSE"),
};

fn select(host: &str, profile: &str) -> Result<Vec<&'static Asset>, String> {
    if !["claude", "cursor", "codex"].contains(&host)
        || !["base", "typed-ui"].contains(&profile)
        || (host == "codex" && profile != "base")
    {
        return Err("adapters: unsupported host or profile".into());
    }
    // These are reviewed embedded inputs, never paths accepted from target metadata.
    let inventory: Value =
        serde_json::from_slice(INVENTORY).map_err(|_| "adapters: invalid bundled inventory")?;
    if inventory["format_version"].as_u64() != Some(1)
        || inventory["kind"] != "native-adapter-assets"
        || inventory["modifies_host_permissions"] != false
        || inventory["executes_commands"] != false
    {
        return Err("adapters: unsupported bundled inventory".into());
    }
    let adapters = inventory["adapters"]
        .as_array()
        .ok_or("adapters: missing bundled adapters")?;
    let matching: Vec<_> = adapters.iter().filter(|a| a["id"] == host).collect();
    if matching.len() != 1 || matching[0]["router"] != "AGENTS.md" {
        return Err("adapters: invalid bundled host".into());
    }
    let expected: Vec<_> = ASSETS.iter().filter(|a| a.host == host).collect();
    let files = matching[0]["files"]
        .as_array()
        .ok_or("adapters: invalid bundled files")?;
    if files.len() != expected.len()
        || expected.iter().any(|asset| {
            files
                .iter()
                .filter(|f| {
                    f["source"] == asset.source
                        && f["target"] == asset.target
                        && f["profile"] == asset.profile
                })
                .count()
                != 1
        })
    {
        return Err("adapters: bundled destinations differ from reviewed allowlist".into());
    }
    let mut chosen: Vec<_> = expected
        .into_iter()
        .filter(|a| a.profile == "base" || a.profile == profile)
        .collect();
    if !chosen.is_empty() {
        chosen.push(&NOTICE);
    }
    if chosen
        .iter()
        .any(|a| a.bytes.is_empty() || a.bytes.len() > LIMIT)
    {
        return Err("adapters: invalid bundled payload size".into());
    }
    Ok(chosen)
}

fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// Return absence without following a link or interpreting a file as a directory.
fn present(root: &Path, relative: &str) -> Result<Option<PathBuf>, String> {
    let parts: Vec<_> = relative.split('/').collect();
    let mut path = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if linked(&meta) || (index + 1 < parts.len() && !meta.is_dir()) {
                    return Err("adapters: linked or non-directory destination component".into());
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("adapters: unreadable destination".into()),
        }
    }
    Ok(Some(path))
}

fn state(root: &Path, relative: &str) -> Result<Value, String> {
    match present(root, relative)? {
        None => Ok(Value::Null),
        Some(path) => {
            let bytes = check_inputs::read(&path, LIMIT)?;
            Ok(json!(check_inputs::hash(&bytes)))
        }
    }
}

pub(crate) fn plan(target: &Path, host: &str, profile: &str) -> Result<Value, String> {
    let assets = select(host, profile)?;
    let root = check_inputs::root(target)?;
    let router = present(&root, "AGENTS.md")?.ok_or("adapters: AGENTS.md is required")?;
    let bytes = check_inputs::read(&router, LIMIT)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "adapters: AGENTS.md must be UTF-8")?;
    if text.trim().is_empty() || text.contains('\0') {
        return Err("adapters: AGENTS.md must contain project instructions".into());
    }
    let mut entries = Vec::new();
    for asset in assets {
        let existing = state(&root, asset.target)?;
        let desired = check_inputs::hash(asset.bytes);
        let action = if existing.is_null() {
            "create"
        } else if existing == desired {
            "unchanged"
        } else {
            "conflict"
        };
        entries.push(json!({"source":asset.source,"target":asset.target,
            "profile":asset.profile,"desired_sha256":desired,
            "existing_sha256":existing,"action":action}));
    }
    let conflict = entries.iter().any(|entry| entry["action"] == "conflict");
    let target_text = root.to_str().ok_or("adapters: unsupported target path")?;
    let mut report = json!({
        "format_version":1,"kind":"adapter-sync","operation":"preview",
        "host":host,"profile":profile,"target":target_text,
        "source":crate::version()["sources"]["agents"],
        "inventory_sha256":check_inputs::hash(INVENTORY),
        "router_sha256":check_inputs::hash(&bytes),"entries":entries,
        "status":if conflict {"conflict"} else {"ready"},
        "created_files":[],"created_directories":[],"error":null,
        "host_delivery_verified":false,"enforcement_verified":false,
        "limitations":["create-only; differing content requires manual reconciliation",
            "no host session or instruction adherence verified",
            "quiescent local worktree; not a hostile-filesystem sandbox",
            "per-file no-overwrite persistence; no multi-file crash transaction"]
    });
    // Stable internal JSON serialization, not a portable JSON canonicalization claim.
    let encoded = serde_json::to_vec(&report).map_err(|_| "adapters: cannot encode preview")?;
    report["plan_digest"] = json!(check_inputs::framed_hash(
        b"ah-adapter-sync-v1\0",
        &[&encoded]
    ));
    Ok(report)
}

fn create_parents(root: &Path, relative: &str, created: &mut Vec<String>) -> Result<(), String> {
    let parts: Vec<_> = relative.split('/').collect();
    let mut name = String::new();
    for part in &parts[..parts.len() - 1] {
        if !name.is_empty() {
            name.push('/');
        }
        name.push_str(part);
        let path = root.join(&name);
        match present(root, &name)? {
            Some(_) if path.is_dir() => {}
            Some(_) => return Err("adapters: destination parent is not a directory".into()),
            None => {
                fs::create_dir(&path).map_err(|_| "adapters: cannot create destination directory")?;
                created.push(name.clone());
            }
        }
    }
    Ok(())
}

fn persist(root: &Path, asset: &Asset) -> Result<(), String> {
    // Recheck immediately before staging; no replacement API is used.
    if present(root, asset.target)?.is_some() {
        return Err("adapters: destination appeared during application".into());
    }
    let destination = root.join(asset.target);
    let parent = destination
        .parent()
        .ok_or("adapters: missing destination parent")?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "adapters: cannot stage context file")?;
    staged
        .write_all(asset.bytes)
        .map_err(|_| "adapters: cannot write staged context")?;
    staged
        .as_file()
        .sync_all()
        .map_err(|_| "adapters: cannot sync staged context")?;
    staged
        .persist_noclobber(destination)
        .map_err(|_| "adapters: context creation failed; no overwrite attempted")?;
    Ok(())
}

fn apply_with(
    target: &Path,
    host: &str,
    profile: &str,
    reviewed: &str,
    mut write: impl FnMut(&Path, &Asset) -> Result<(), String>,
) -> Result<Value, String> {
    let mut report = plan(target, host, profile)?;
    if report["plan_digest"].as_str() != Some(reviewed) {
        return Err("adapters: stale or mismatched preview; review again".into());
    }
    report["operation"] = json!("apply");
    if report["status"] == "conflict" {
        return Ok(report);
    }
    let root = check_inputs::root(target)?;
    let assets = select(host, profile)?;
    let mut created_files = Vec::new();
    let mut created_directories = Vec::new();
    let mut failure: Option<String> = None;
    for (asset, entry) in assets.iter().zip(report["entries"].as_array().unwrap()) {
        let result: Result<(), String> = (|| {
            if state(&root, asset.target)? != entry["existing_sha256"] {
                return Err("adapters: destination changed during application".into());
            }
            if state(&root, "AGENTS.md")? != report["router_sha256"] {
                return Err("adapters: router changed during application".into());
            }
            if entry["action"] == "create" {
                create_parents(&root, asset.target, &mut created_directories)?;
                write(&root, asset)?;
                created_files.push(asset.target.to_string());
            }
            Ok(())
        })();
        if let Err(error) = result {
            failure = Some(error);
            break;
        }
    }
    if failure.is_none() {
        match plan(&root, host, profile) {
            Ok(after)
                if after["router_sha256"] == report["router_sha256"]
                    && after["entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|entry| entry["action"] == "unchanged") => {}
            _ => failure = Some("adapters: post-install verification failed".into()),
        }
    }
    report["status"] = json!(if failure.is_none() { "applied" } else { "partial" });
    report["created_files"] = json!(created_files);
    report["created_directories"] = json!(created_directories);
    report["error"] = json!(failure);
    Ok(report)
}

pub(crate) fn run(args: Vec<String>) {
    if args.len() < 2 || ["--help", "-h"].contains(&args[1].as_str()) {
        println!("usage: ah adapters sync [TARGET] --host claude|cursor|codex [--profile base|typed-ui]\nPreview by default. Add --apply --review PLAN_DIGEST to create missing files only.");
        return;
    }
    let mut target = ".";
    let mut host = None;
    let mut profile = "base";
    let mut review = None;
    let mut apply = false;
    let mut seen = std::collections::BTreeSet::new();
    let mut index = 2;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg.starts_with("--") {
            if !seen.insert(arg) {
                crate::fail("adapters: repeated option");
            }
            if arg == "--apply" {
                apply = true;
            } else {
                index += 1;
                match arg {
                    "--host" => host = Some(args[index].as_str()),
                    "--profile" => profile = &args[index],
                    "--review" => review = Some(args[index].as_str()),
                    _ => crate::fail("adapters: unsupported option"),
                }
            }
        } else {
            target = arg;
        }
        index += 1;
    }
    let host = host.unwrap_or_else(|| crate::fail("adapters: --host is required"));
    if apply != review.is_some() {
        crate::fail("adapters: --apply and --review must be supplied together");
    }
    let result = if apply {
        apply_with(Path::new(target), host, profile, review.unwrap(), persist)
    } else {
        plan(Path::new(target), host, profile)
    };
    match result {
        Err(error) => crate::fail(error),
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            if ["conflict", "partial"].contains(&value["status"].as_str().unwrap()) {
                crate::finish(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_batch_retains_complete_files_without_rollback() {
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("AGENTS.md"), "# Synthetic router").unwrap();
        let preview = plan(target.path(), "claude", "typed-ui").unwrap();
        let mut calls = 0;
        let report = apply_with(
            target.path(),
            "claude",
            "typed-ui",
            preview["plan_digest"].as_str().unwrap(),
            |root, asset| {
                calls += 1;
                if calls == 2 {
                    return Err("synthetic staging failure".into());
                }
                persist(root, asset)
            },
        )
        .unwrap();
        assert_eq!(report["status"], "partial");
        assert_eq!(report["created_files"], json!(["CLAUDE.md"]));
        assert_eq!(
            fs::read(target.path().join("CLAUDE.md")).unwrap(),
            b"@AGENTS.md\n"
        );
        assert!(
            !target
                .path()
                .join(".claude/rules/agentic-typed-ui.md")
                .exists()
        );
        let fresh = plan(target.path(), "claude", "typed-ui").unwrap();
        let result = apply_with(
            target.path(),
            "claude",
            "typed-ui",
            fresh["plan_digest"].as_str().unwrap(),
            persist,
        )
        .unwrap();
        assert_eq!(result["status"], "applied");
    }
}
