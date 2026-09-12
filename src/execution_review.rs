//! Host-specific, non-executting review of tools and an explicit environment.
use crate::check_inputs;
use crate::checks::{number, shape, text};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::Path;

pub(crate) fn supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

fn mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    }
    #[cfg(not(unix))]
    {
        u32::from(metadata.permissions().readonly())
    }
}

fn tool(path: &str) -> Result<Value, String> {
    let requested = Path::new(path);
    if !requested.is_absolute() {
        return Err("checks: tool bindings must be absolute paths".into());
    }
    let canonical = requested
        .canonicalize()
        .map_err(|_| "checks: tool binding cannot be resolved")?;
    let canonical_text = canonical.to_str().ok_or("checks: unsupported tool path")?;
    let before = fs::metadata(&canonical).map_err(|_| "checks: unreadable tool")?;
    if !before.is_file() || before.len() > 268_435_456 {
        return Err("checks: tool must be a regular file of at most 256 MiB".into());
    }
    #[cfg(unix)]
    if mode(&before) & 0o111 == 0 {
        return Err("checks: tool has no executable permission bits".into());
    }
    let mut file = fs::File::open(&canonical).map_err(|_| "checks: cannot open tool")?;
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut prefix = Vec::new();
    let mut buffer = [0u8; 65_536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "checks: cannot read tool")?;
        if count == 0 {
            break;
        }
        if size == 0 {
            prefix.extend_from_slice(&buffer[..count.min(4)]);
        }
        size += count as u64;
        if size > 268_435_456 {
            return Err("checks: tool exceeds fingerprint limit".into());
        }
        digest.update(&buffer[..count]);
    }
    let native = prefix.starts_with(b"\x7fELF")
        || prefix.starts_with(b"MZ")
        || matches!(
            prefix.as_slice(),
            [0xfe, 0xed, 0xfa, 0xce]
                | [0xce, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xcf, 0xfa, 0xed, 0xfe]
                | [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
                | [0xca, 0xfe, 0xba, 0xbf]
                | [0xbf, 0xba, 0xfe, 0xca]
        );
    if !native {
        return Err("checks: bind a native executable; script launchers are unsupported".into());
    }
    let after = fs::metadata(&canonical).map_err(|_| "checks: tool changed during review")?;
    if requested.canonicalize().ok().as_ref() != Some(&canonical)
        || !after.is_file()
        || before.len() != size
        || after.len() != size
        || mode(&before) != mode(&after)
        || before.modified().ok() != after.modified().ok()
    {
        return Err("checks: tool changed during review".into());
    }
    let sha256 = format!("sha256:{:x}", digest.finalize());
    Ok(json!({
        "path":path,"canonical_path":canonical_text,"sha256":sha256,
        "size_bytes":size,"mode":mode(&after),"runtime_version":null
    }))
}

fn environment(value: &Value) -> Result<(), String> {
    let map = value
        .as_object()
        .ok_or("checks: environment must be an object")?;
    if map.is_empty() || map.len() > 64 || !map.contains_key("PATH") {
        return Err("checks: provide an explicit PATH and at most 64 environment entries".into());
    }
    for (key, value) in map {
        if key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            || key.as_bytes()[0].is_ascii_digit()
        {
            return Err("checks: unsupported environment key".into());
        }
        let value = value
            .as_str()
            .ok_or("checks: environment values must be strings")?;
        if value.chars().count() > 4096 || value.chars().any(char::is_control) {
            return Err("checks: unsupported environment value".into());
        }
    }
    // Empty components mean the cwd. An empty PATH explicitly disables lookup.
    let path = map["PATH"].as_str().unwrap();
    if !path.is_empty() && std::env::split_paths(path).any(|entry| !entry.is_absolute()) {
        return Err("checks: PATH entries must be absolute, or PATH must be empty".into());
    }
    Ok(())
}

pub(crate) fn prepare(target: &Path, config: &str, settings: &str) -> Result<Value, String> {
    let root = check_inputs::root(target)?;
    let settings_path = check_inputs::resolve(&root, settings, false)?;
    let bytes = check_inputs::read(&settings_path, 262_144)?;
    let options = crate::strict_json::decode(&bytes)?;
    shape(
        &options,
        &[
            "format_version",
            "kind",
            "tools",
            "environment",
            "max_total_ms",
        ],
    )?;
    if options["format_version"].as_u64() != Some(1)
        || options["kind"] != "check-execution-settings"
    {
        return Err("checks: unsupported execution settings".into());
    }
    number(&options["max_total_ms"], 900_000)?;
    environment(&options["environment"])?;
    let plan = crate::checks::plan(&root, config)?;
    let bindings = options["tools"]
        .as_object()
        .ok_or("checks: tools must be an object")?;
    let names: BTreeSet<&str> = plan["policy"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|check| check["argv"][0].as_str().unwrap())
        .collect();
    if names != bindings.keys().map(String::as_str).collect() {
        return Err("checks: tool bindings must exactly match the requested executable names".into());
    }
    let mut tools = serde_json::Map::new();
    for name in names {
        tools.insert(name.to_string(), tool(text(&bindings[name], 4096)?)?);
    }
    let executable =
        std::env::current_exe().map_err(|_| "checks: executor identity unavailable")?;
    let executor = tool(executable.to_str().ok_or("checks: unsupported executor path")?)?;
    let settings_digest = check_inputs::hash(&bytes);
    let target_text = root.to_str().ok_or("checks: unsupported target path")?;
    let tools_bytes = serde_json::to_vec(&tools).map_err(|_| "checks: cannot encode tools")?;
    let approval_digest = check_inputs::framed_hash(
        b"ah-check-execution-review-v1\0",
        &[
            plan["review_digest"].as_str().unwrap().as_bytes(),
            settings_digest.as_bytes(),
            config.as_bytes(),
            settings.as_bytes(),
            target_text.as_bytes(),
            std::env::consts::OS.as_bytes(),
            std::env::consts::ARCH.as_bytes(),
            executor["sha256"].as_str().unwrap().as_bytes(),
            &tools_bytes,
        ],
    );
    if check_inputs::read(&settings_path, 262_144)? != bytes {
        return Err("checks: execution settings changed during review".into());
    }
    Ok(json!({
        "format_version":1,"kind":"check-execution-review","plan":plan,
        "config_path":config,"settings_path":settings,"settings_digest":settings_digest,
        "target":target_text,"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},
        "tools":tools,"executor_sha256":executor["sha256"],"environment":options["environment"],
        "inherit_environment":false,"max_total_ms":options["max_total_ms"],
        "approval_digest":approval_digest,"execution_supported":supported(),
        "execution_permitted":false,"checks_executed":false,
        "not_checked":["tool runtime versions","transitive executables and libraries",
            "network and filesystem sandboxing","hostile concurrent mutations","governance enforcement"]
    }))
}
