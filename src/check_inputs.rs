//! Exact, bounded input snapshots for non-executing check previews.
//! Unlike advisory analysis scans, declared inputs never use implicit ignore rules.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_FILE: usize = 2_000_000;
const MAX_TOTAL: usize = 64_000_000;
const MAX_ENTRIES: usize = 10_000;

pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub(crate) fn framed_hash(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    for field in fields {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field);
    }
    format!("sha256:{:x}", digest.finalize())
}

pub(crate) fn valid_path(value: &str, allow_dot: bool) -> bool {
    if allow_dot && value == "." {
        return true;
    }
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_. /".contains(&b))
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.starts_with(' ')
                && !part.ends_with(' ')
                && !part.ends_with('.')
                && !matches!(part.to_ascii_lowercase().as_str(), ".git" | ".env")
                && !part.to_ascii_lowercase().starts_with(".env.")
        })
}

pub(crate) fn root(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "checks: unreadable target")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("checks: target must be a real directory".into());
    }
    path.canonicalize()
        .map_err(|_| "checks: invalid target".into())
}

pub(crate) fn resolve(root: &Path, value: &str, allow_dot: bool) -> Result<PathBuf, String> {
    if !valid_path(value, allow_dot) {
        return Err("checks: path must be portable, relative and normalized".into());
    }
    let mut result = root.to_path_buf();
    if value != "." {
        for part in value.split('/') {
            result.push(part);
            let metadata = fs::symlink_metadata(&result)
                .map_err(|_| "checks: declared path does not exist or cannot be read")?;
            if metadata.file_type().is_symlink() {
                return Err("checks: symlinks are not supported in declared paths".into());
            }
        }
    }
    Ok(result)
}

pub(crate) fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let before = fs::symlink_metadata(path).map_err(|_| "checks: unreadable file")?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err("checks: expected a regular non-symlink file".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "checks: cannot open file")?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "checks: cannot read file")?;
    if bytes.len() > limit {
        return Err("checks: declared file exceeds read limit".into());
    }
    let after = fs::symlink_metadata(path).map_err(|_| "checks: input changed during read")?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.modified().ok() != after.modified().ok()
    {
        return Err("checks: input changed during read".into());
    }
    Ok(bytes)
}

pub(crate) fn snapshot(root: &Path, inputs: &[String]) -> Result<(String, Vec<Value>), String> {
    fn visit(
        root: &Path,
        relative: &str,
        depth: usize,
        total: &mut usize,
        entries: &mut BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if entries.contains_key(relative) {
            return Ok(());
        }
        if depth > 64 || entries.len() >= MAX_ENTRIES {
            return Err("checks: declared input tree exceeds traversal limits".into());
        }
        let path = resolve(root, relative, false)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| "checks: unreadable input")?;
        if metadata.is_dir() {
            entries.insert(
                relative.into(),
                json!({"path":relative,"kind":"directory","digest":null}),
            );
            for child in fs::read_dir(path).map_err(|_| "checks: unreadable input directory")? {
                let name = child
                    .map_err(|_| "checks: incomplete directory traversal")?
                    .file_name()
                    .into_string()
                    .map_err(|_| "checks: input filename is not supported UTF-8")?;
                visit(root, &format!("{relative}/{name}"), depth + 1, total, entries)?;
            }
        } else {
            let bytes = read(&path, MAX_FILE)?;
            *total += bytes.len();
            if *total > MAX_TOTAL {
                return Err("checks: declared inputs exceed total read limit".into());
            }
            entries.insert(
                relative.into(),
                json!({"path":relative,"kind":"file","digest":hash(&bytes)}),
            );
        }
        Ok(())
    }

    let mut entries = BTreeMap::new();
    let mut total = 0;
    for input in inputs {
        visit(root, input, 0, &mut total, &mut entries)?;
    }
    let values: Vec<Value> = entries.into_values().collect();
    let mut fields = Vec::new();
    for value in &values {
        fields.push(value["path"].as_str().unwrap().as_bytes());
        fields.push(value["kind"].as_str().unwrap().as_bytes());
        fields.push(value["digest"].as_str().unwrap_or("").as_bytes());
    }
    Ok((framed_hash(b"ah-check-inputs-v1\0", &fields), values))
}
