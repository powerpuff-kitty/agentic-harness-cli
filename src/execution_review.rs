//! Host-specific, non-executting review of tools and an explicit environment.
use crate::check_inputs;
use crate::checks::{number, shape, text};
use crate::execution_budget::Budget;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

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

const MAX_REVIEW_TOOL_BYTES: u64 = 536_870_912;

struct ToolReview<'a> {
    budget: &'a Budget,
    bytes: u64,
    cache: BTreeMap<PathBuf, (Value, fs::Metadata)>,
}
impl<'a> ToolReview<'a> {
    fn new(budget: &'a Budget) -> Self {
        Self {
            budget,
            bytes: 0,
            cache: BTreeMap::new(),
        }
    }
    fn charge(&mut self, bytes: u64) -> Result<(), String> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_REVIEW_TOOL_BYTES {
            return Err("checks: cumulative tool review byte limit exceeded".into());
        }
        self.budget.check()
    }
    fn fingerprint(&mut self, file: &mut impl Read) -> Result<(String, u64, Vec<u8>), String> {
        let mut digest = Sha256::new();
        let mut size = 0u64;
        let mut prefix = Vec::new();
        let mut buffer = [0u8; 65_536];
        loop {
            self.budget.check()?;
            let count = file
                .read(&mut buffer)
                .map_err(|_| "checks: cannot read tool")?;
            self.charge(count as u64)?;
            if count == 0 {
                break;
            }
            let needed = 4usize.saturating_sub(prefix.len());
            prefix.extend_from_slice(&buffer[..count.min(needed)]);
            size += count as u64;
            if size > 268_435_456 {
                return Err("checks: tool exceeds fingerprint limit".into());
            }
            digest.update(&buffer[..count]);
        }
        Ok((format!("sha256:{:x}", digest.finalize()), size, prefix))
    }
    fn tool(&mut self, path: &str) -> Result<Value, String> {
        self.budget.check()?;
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
        if let Some((cached, metadata)) = self.cache.get(&canonical) {
            if before.len() != metadata.len()
                || mode(&before) != mode(metadata)
                || before.modified().ok() != metadata.modified().ok()
            {
                return Err("checks: aliased tool changed during review".into());
            }
            let mut result = cached.clone();
            result["path"] = json!(path);
            self.budget.check()?;
            return Ok(result);
        }
        // Reject oversized cumulative work before starting this file; actual reads
        // are also charged to cover files that change after metadata observation.
        if self.bytes.saturating_add(before.len()) > MAX_REVIEW_TOOL_BYTES {
            return Err("checks: cumulative tool review byte limit exceeded".into());
        }
        let mut file = fs::File::open(&canonical).map_err(|_| "checks: cannot open tool")?;
        let (sha256, size, prefix) = self.fingerprint(&mut file)?;
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
            return Err(
                "checks: bind a native executable; script launchers are unsupported".into(),
            );
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
        let value = json!({
            "path":path,"canonical_path":canonical_text,"sha256":sha256,
            "size_bytes":size,"mode":mode(&after),"runtime_version":null
        });
        self.cache.insert(canonical.clone(), (value.clone(), after));
        self.budget.check()?;
        Ok(value)
    }
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
    prepare_with_budget(target, config, settings, &Budget::new())
}

pub(crate) fn prepare_with_budget(
    target: &Path,
    config: &str,
    settings: &str,
    budget: &Budget,
) -> Result<Value, String> {
    budget.check()?;
    let root = check_inputs::root(target)?;
    let settings_path = check_inputs::resolve(&root, settings, false)?;
    let bytes = check_inputs::read_with_budget(&settings_path, 262_144, budget)?;
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
    budget.limit(number(&options["max_total_ms"], 900_000)?)?;
    environment(&options["environment"])?;
    let plan = crate::checks::plan_with_budget(&root, config, budget)?;
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
        return Err(
            "checks: tool bindings must exactly match the requested executable names".into(),
        );
    }
    let mut tools = serde_json::Map::new();
    let mut tool_review = ToolReview::new(budget);
    for name in names {
        tools.insert(
            name.to_string(),
            tool_review.tool(text(&bindings[name], 4096)?)?,
        );
    }
    let executable =
        std::env::current_exe().map_err(|_| "checks: executor identity unavailable")?;
    let executor = tool_review.tool(
        executable
            .to_str()
            .ok_or("checks: unsupported executor path")?,
    )?;
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
    if check_inputs::read_with_budget(&settings_path, 262_144, budget)? != bytes {
        return Err("checks: execution settings changed during review".into());
    }
    budget.check()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cumulative_cap_is_not_a_per_binding_cap() {
        let budget = Budget::new();
        let mut review = ToolReview::new(&budget);
        review.charge(268_435_456).unwrap();
        review.charge(268_435_456).unwrap();
        assert!(review.charge(1).is_err());
    }
    #[test]
    fn aliases_hash_once_per_review_but_never_across_reviews() {
        let budget = Budget::new();
        let exe = std::env::current_exe().unwrap();
        let path = exe.to_str().unwrap();
        let mut review = ToolReview::new(&budget);
        let first = review.tool(path).unwrap();
        let read = review.bytes;
        assert_eq!(review.tool(path).unwrap(), first);
        assert_eq!(review.bytes, read);
        let mut next = ToolReview::new(&budget);
        next.tool(path).unwrap();
        assert_eq!(next.bytes, read);
    }
    #[test]
    fn slow_reader_cannot_return_a_valid_fingerprint_after_deadline() {
        struct Slow<'a>(&'a Budget);
        impl Read for Slow<'_> {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                bytes[..4].copy_from_slice(b"\x7fELF");
                let _ = self.0.limit(0); // deterministic clock/deadline injection
                Ok(4)
            }
        }
        let budget = Budget::new();
        let mut review = ToolReview::new(&budget);
        assert!(review.fingerprint(&mut Slow(&budget)).is_err());
    }
}
