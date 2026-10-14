//! Explicitly scoped skill lifecycle; project composition retains its existing policy.
use include_dir::{Dir, DirEntry, include_dir};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

static SKILLS: Dir<'_> = include_dir!("$OUT_DIR/skills");
const STATE: &str = ".ah-skills/state.json";
fn err(s: impl Into<String>) -> io::Error {
    io::Error::other(s.into())
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn source() -> Value {
    crate::version()["sources"]["agents"].clone()
}
fn domains() -> BTreeMap<String, Vec<String>> {
    serde_json::from_str(include_str!("../catalog/skill-domains.json")).unwrap()
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}
fn safe_relative(s: &str) -> bool {
    !s.is_empty()
        && Path::new(s)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
        && !s.contains('\\')
}
fn regular_path(path: &Path) -> io::Result<()> {
    for p in path.ancestors() {
        match fs::symlink_metadata(p) {
            Ok(m) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if m.file_attributes() & 0x400 != 0 {
                        return Err(err("reparse destination"));
                    }
                }
                if m.file_type().is_symlink() {
                    return Err(err(format!("symlink destination: {}", p.display())));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    regular_path(path)?;
    match fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    regular_path(path)?;
    let parent = path.parent().ok_or_else(|| err("missing parent"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    if let Ok(m) = fs::metadata(path) {
        temp.as_file().set_permissions(m.permissions())?;
    }
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn state(target: &Path) -> io::Result<Value> {
    let Some(bytes) = read(&target.join(STATE))? else {
        return Ok(
            json!({"format_version":1,"repository":"powerpuff-kitty/skills","files":{},"skills":{}}),
        );
    };
    let value: Value = serde_json::from_slice(&bytes)?;
    if value["format_version"] != 1
        || value["repository"] != "powerpuff-kitty/skills"
        || !value["files"].is_object()
        || !value["skills"].is_object()
    {
        return Err(err("invalid skill management state"));
    }
    for (name, h) in value["files"].as_object().unwrap() {
        if !safe_relative(name)
            || !value["skills"]
                .as_object()
                .unwrap()
                .keys()
                .any(|s| name.starts_with(&format!("{s}/")))
            || !h
                .as_str()
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(err("invalid managed file record"));
        }
    }
    if value["skills"].as_object().unwrap().keys().any(|s| !id(s)) {
        return Err(err("invalid managed skill ID"));
    }
    Ok(value)
}
fn payload(dir: &Dir<'_>, result: &mut BTreeMap<String, Vec<u8>>) {
    for e in dir.entries() {
        match e {
            DirEntry::Dir(d) => payload(d, result),
            DirEntry::File(f) => {
                result.insert(
                    f.path().to_string_lossy().replace('\\', "/"),
                    f.contents().to_vec(),
                );
            }
        }
    }
}
fn select(
    names: &[String],
    groups: &[String],
    all: bool,
    installed: &Value,
    command: &str,
) -> io::Result<BTreeSet<String>> {
    let mut chosen: BTreeSet<_> = names.iter().cloned().collect();
    let catalog = domains();
    for group in groups {
        chosen.extend(
            catalog
                .get(group)
                .ok_or_else(|| err(format!("unknown domain: {group}")))?
                .clone(),
        );
    }
    if all {
        chosen.extend(
            SKILLS
                .dirs()
                .map(|d| d.path().to_string_lossy().to_string()),
        );
    }
    if chosen.is_empty() && ["update", "status"].contains(&command) {
        chosen.extend(installed["skills"].as_object().unwrap().keys().cloned());
    }
    if chosen.is_empty() && command != "status" {
        return Err(err("select --skill NAME, --domain NAME, or --all"));
    }
    for name in &chosen {
        if !id(name) || SKILLS.get_dir(name).is_none() {
            return Err(err(format!("unknown skill: {name}")));
        }
    }
    Ok(chosen)
}
fn apply(
    target: &Path,
    command: &str,
    names: &[String],
    groups: &[String],
    all: bool,
    dry: bool,
) -> io::Result<Value> {
    regular_path(target)?;
    let previous_state = read(&target.join(STATE))?;
    let mut installed = state(target)?;
    let selected = select(names, groups, all, &installed, command)?;
    let mut desired = BTreeMap::new();
    for name in &selected {
        payload(SKILLS.get_dir(name).unwrap(), &mut desired);
    }
    let mut before = BTreeMap::new();
    let mut changes = BTreeMap::new();
    let mut inventory = Vec::new();
    let mut conflicts = Vec::new();
    for (name, incoming) in &desired {
        let current = read(&target.join(name))?;
        let managed = installed["files"][name].as_str();
        let state = match &current {
            None => "missing",
            Some(b) if b == incoming => "current",
            Some(b) if managed == Some(hash(b).as_str()) => "update_available",
            Some(_) if managed.is_some() => "locally_modified",
            Some(_) => "unmanaged_conflict",
        };
        inventory.push(json!({"path":name,"status":state}));
        if ["locally_modified", "unmanaged_conflict"].contains(&state) {
            conflicts.push(name.clone());
        } else if state != "current" {
            changes.insert(name.clone(), incoming.clone());
            before.insert(name.clone(), current);
        }
    }
    let obsolete: Vec<_> = installed["files"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|p| {
            selected.iter().any(|n| p.starts_with(&format!("{n}/"))) && !desired.contains_key(*p)
        })
        .cloned()
        .collect();
    let mut report = json!({"format_version":1,"kind":"skill-lifecycle","command":command,"source":source(),"installed_sources":installed["skills"],"target":target,"selected":selected,"dry_run":dry,"files":inventory,"conflicts":conflicts,"obsolete_preserved":obsolete,"changed":changes.keys().collect::<Vec<_>>(),"backup":null});
    if command == "status" || dry {
        return Ok(report);
    }
    if !conflicts.is_empty() {
        return Err(err(format!(
            "skill conflicts; no changes applied: {}",
            conflicts.join(", ")
        )));
    }
    for name in &selected {
        installed["skills"][name] = source();
    }
    for (name, bytes) in &desired {
        installed["files"][name] = json!(hash(bytes));
    }
    let next_state = serde_json::to_vec_pretty(&installed)?;
    if changes.is_empty() && previous_state.as_deref() == Some(next_state.as_slice()) {
        return Ok(report);
    }
    before.insert(STATE.into(), previous_state.clone());
    changes.insert(STATE.into(), next_state);
    fs::create_dir_all(target.join(".ah-skills/backups"))?;
    let journal = tempfile::Builder::new()
        .prefix("revision-")
        .tempdir_in(target.join(".ah-skills/backups"))?;
    let backup = journal.keep();
    let mut entries = Vec::new();
    for (i, (name, old)) in before.iter().enumerate() {
        if let Some(b) = old {
            write(&backup.join(format!("{i}.bin")), b)?;
        }
        entries.push(json!({"path":name,"before":old.as_ref().map(|b|hash(b)),"backup":old.as_ref().map(|_|format!("{i}.bin")),"after":hash(&changes[name])}));
    }
    write(
        &backup.join("journal.json"),
        &serde_json::to_vec_pretty(&json!({"format_version":1,"entries":entries}))?,
    )?;
    let mut written = Vec::new();
    let result = (|| -> io::Result<()> {
        // Check every planned file again before applying; never overwrite concurrent edits.
        for (name, bytes) in &before {
            if read(&target.join(name))? != *bytes {
                return Err(err("destination changed during preflight"));
            }
        }
        for (name, bytes) in &changes {
            if read(&target.join(name))? != before[name] {
                return Err(err("destination changed during installation"));
            }
            write(&target.join(name), bytes)?;
            written.push(name.clone());
        }
        Ok(())
    })();
    if let Err(e) = result {
        for name in written.iter().rev() {
            // Restore only bytes this invocation wrote, preserving concurrent changes.
            if read(&target.join(name))?.as_deref() == Some(changes[name].as_slice()) {
                if let Some(b) = &before[name] {
                    write(&target.join(name), b)?;
                } else {
                    fs::remove_file(target.join(name))?;
                }
            }
        }
        return Err(err(format!("{e}; recovery journal: {}", backup.display())));
    }
    report["backup"] = json!(backup.file_name().unwrap().to_string_lossy());
    Ok(report)
}
fn rollback(target: &Path, revision: &str, dry: bool) -> io::Result<Value> {
    if !(id(revision)
        || (revision.starts_with("revision-")
            && revision
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')))
    {
        return Err(err("invalid backup ID"));
    }
    let backup = target.join(".ah-skills/backups").join(revision);
    let journal: Value = serde_json::from_slice(
        &read(&backup.join("journal.json"))?.ok_or_else(|| err("backup missing"))?,
    )?;
    if journal["format_version"] != 1 {
        return Err(err("invalid backup format"));
    }
    let entries = journal["entries"]
        .as_array()
        .ok_or_else(|| err("invalid backup entries"))?;
    let mut restores = BTreeMap::new();
    for entry in entries {
        let name = entry["path"]
            .as_str()
            .ok_or_else(|| err("invalid backup path"))?;
        if !safe_relative(name)
            || !(name == STATE || (!name.starts_with('.') && name.contains('/')))
        {
            return Err(err("invalid backup destination"));
        }
        let current = read(&target.join(name))?;
        if current.as_ref().map(|b| hash(b)).as_deref() != entry["after"].as_str() {
            return Err(err(format!("rollback conflict: {name}")));
        }
        let original = if let Some(file) = entry["backup"].as_str() {
            if !safe_relative(file) || file.contains('/') {
                return Err(err("invalid backup file"));
            }
            let b = read(&backup.join(file))?.ok_or_else(|| err("missing backup content"))?;
            if Some(hash(&b).as_str()) != entry["before"].as_str() {
                return Err(err("backup checksum mismatch"));
            }
            Some(b)
        } else {
            if !entry["before"].is_null() {
                return Err(err("invalid backup content"));
            }
            None
        };
        if restores
            .insert(name.to_string(), (current, original))
            .is_some()
        {
            return Err(err("duplicate backup entry"));
        }
    }
    if !dry {
        for (name, (expected, original)) in &restores {
            if read(&target.join(name))? != *expected {
                return Err(err("destination changed during rollback"));
            }
            if let Some(b) = original {
                write(&target.join(name), b)?;
            } else {
                regular_path(&target.join(name))?;
                fs::remove_file(target.join(name))?;
            }
        }
    }
    Ok(
        json!({"format_version":1,"kind":"skill-rollback","backup":revision,"dry_run":dry,"restored":restores.keys().collect::<Vec<_>>()}),
    )
}
pub fn run(args: Vec<String>) {
    let result = (|| -> io::Result<Value> {
        let command = args.get(1).map(String::as_str).unwrap_or("--help");
        if ["--help", "-h"].contains(&command) {
            return Ok(
                json!({"usage":"ah skills <list|install|update|status|rollback> --target DIRECTORY [--skill NAME] [--domain NAME] [--all] [--dry-run] [--backup ID]","scope":"target is an explicit host skill directory; no account/config changes; no automatic pruning"}),
            );
        }
        if command == "list" {
            return Ok(
                json!({"format_version":1,"source":source(),"skills":SKILLS.dirs().map(|d|d.path().to_string_lossy().to_string()).collect::<Vec<_>>(),"domains":domains()}),
            );
        }
        let (mut target, mut revision) = (None, None);
        let (mut names, mut groups) = (Vec::new(), Vec::new());
        let (mut all, mut dry) = (false, false);
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--all" => all = true,
                "--dry-run" => dry = true,
                flag @ ("--target" | "--backup" | "--skill" | "--domain") => {
                    i += 1;
                    let value = args
                        .get(i)
                        .ok_or_else(|| err("missing option value"))?
                        .clone();
                    match flag {
                        "--target" => target = Some(PathBuf::from(value)),
                        "--backup" => revision = Some(value),
                        "--skill" => names.push(value),
                        _ => groups.push(value),
                    }
                }
                _ => return Err(err("unknown skill lifecycle option")),
            }
            i += 1;
        }
        let target = target.ok_or_else(|| {
            err("--target is required; choose project or global scope explicitly")
        })?;
        if command == "rollback" {
            rollback(
                &target,
                &revision.ok_or_else(|| err("--backup is required"))?,
                dry,
            )
        } else {
            apply(&target, command, &names, &groups, all, dry)
        }
    })();
    match result {
        Ok(v) => println!("{v}"),
        Err(e) => crate::fail(e.to_string()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domain_catalog_resolves_and_covers_all_skills() {
        let mut covered = BTreeSet::new();
        for (name, skills) in domains() {
            assert!(id(&name));
            for skill in skills {
                assert!(SKILLS.get_dir(&skill).is_some(), "{skill}");
                covered.insert(skill);
            }
        }
        assert_eq!(covered.len(), SKILLS.dirs().count());
        for dir in SKILLS.dirs() {
            let entry = dir.get_file(dir.path().join("SKILL.md")).unwrap();
            let text = entry.contents_utf8().unwrap();
            let (front, body) = text
                .strip_prefix("---\n")
                .unwrap()
                .split_once("\n---\n")
                .unwrap();
            let metadata: Value = serde_yaml_ng::from_str(front).unwrap();
            assert_eq!(metadata["name"].as_str(), dir.path().to_str());
            let description = metadata["description"].as_str().unwrap();
            assert!(!description.trim().is_empty() && description.len() <= 1024);
            assert!(!body.trim().is_empty());
        }
    }
    #[test]
    fn lifecycle_preserves_edits_and_rolls_back_adoption() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().canonicalize().unwrap().join("skills");
        let names = vec!["seo-audit".into()];
        apply(&target, "install", &names, &[], false, true).unwrap();
        assert!(!target.exists());
        let first = apply(&target, "install", &names, &[], false, false).unwrap();
        assert!(apply(&target, "update", &[], &[], false, false).unwrap()["backup"].is_null());
        let skill = target.join("seo-audit/SKILL.md");
        fs::write(&skill, b"custom").unwrap();
        assert!(apply(&target, "update", &[], &[], false, false).is_err());
        assert!(rollback(&target, first["backup"].as_str().unwrap(), false).is_err());
        fs::write(
            &skill,
            SKILLS.get_file("seo-audit/SKILL.md").unwrap().contents(),
        )
        .unwrap();
        rollback(&target, first["backup"].as_str().unwrap(), false).unwrap();
        assert!(!skill.exists());
        assert!(!target.join(STATE).exists());
    }
    #[test]
    fn pristine_old_version_updates_with_recoverable_backup() {
        let root = tempfile::tempdir().unwrap();
        let canonical = root.path().canonicalize().unwrap();
        let target = canonical.as_path();
        let names = vec!["seo-audit".into()];
        apply(target, "install", &names, &[], false, false).unwrap();
        let path = target.join("seo-audit/SKILL.md");
        fs::write(&path, b"older managed version").unwrap();
        let mut st = state(target).unwrap();
        st["files"]["seo-audit/SKILL.md"] = json!(hash(b"older managed version"));
        write(
            &target.join(STATE),
            &serde_json::to_vec_pretty(&st).unwrap(),
        )
        .unwrap();
        let update = apply(target, "update", &[], &[], false, false).unwrap();
        assert_ne!(fs::read(&path).unwrap(), b"older managed version");
        rollback(target, update["backup"].as_str().unwrap(), false).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"older managed version");
    }
    #[test]
    fn invalid_selection_and_destination_do_not_write() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().canonicalize().unwrap().join("skills");
        assert!(apply(&target, "install", &[], &["invalid".into()], false, false).is_err());
        assert!(!target.exists());
        fs::create_dir_all(target.join("seo-audit/SKILL.md")).unwrap();
        assert!(apply(&target, "install", &["seo-audit".into()], &[], false, false).is_err());
        assert!(!target.join(STATE).exists());
    }
    #[test]
    fn partial_install_preserves_other_domains_and_restores_resources() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().canonicalize().unwrap();
        apply(
            &target,
            "install",
            &[],
            &["content-growth".into()],
            false,
            false,
        )
        .unwrap();
        let custom = target.join("content-brief/assets/brief-template.md");
        fs::write(&custom, b"local editorial template").unwrap();
        apply(
            &target,
            "install",
            &["financial-model-review".into()],
            &[],
            false,
            false,
        )
        .unwrap();
        assert_eq!(fs::read(&custom).unwrap(), b"local editorial template");
        let missing = target.join("financial-model-review/references/model-review.md");
        fs::remove_file(&missing).unwrap();
        apply(
            &target,
            "update",
            &["financial-model-review".into()],
            &[],
            false,
            false,
        )
        .unwrap();
        assert!(missing.is_file());
        assert_eq!(fs::read(&custom).unwrap(), b"local editorial template");
        let report = apply(&target, "status", &[], &[], false, false).unwrap();
        assert!(
            report["conflicts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "content-brief/assets/brief-template.md")
        );
    }
    #[test]
    fn conflicts_preflight_entire_selection_and_backup_integrity() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().canonicalize().unwrap();
        fs::create_dir_all(target.join("geo-measurement")).unwrap();
        fs::write(target.join("geo-measurement/SKILL.md"), b"unmanaged").unwrap();
        assert!(
            apply(
                &target,
                "install",
                &["geo-measurement".into(), "seo-audit".into()],
                &[],
                false,
                false
            )
            .is_err()
        );
        assert!(!target.join("seo-audit").exists());
        assert!(!target.join(STATE).exists());
        let report = apply(&target, "install", &["seo-audit".into()], &[], false, false).unwrap();
        let revision = report["backup"].as_str().unwrap();
        let path = target
            .join(".ah-skills/backups")
            .join(revision)
            .join("journal.json");
        let mut journal: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        journal["entries"][0]["after"] = json!("wrong hash");
        fs::write(path, serde_json::to_vec(&journal).unwrap()).unwrap();
        assert!(rollback(&target, revision, false).is_err());
        assert!(target.join("seo-audit/SKILL.md").exists());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_linked_ancestors() {
        let root = tempfile::tempdir().unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(root.path(), &link).unwrap();
        assert!(
            apply(
                &link.join("skills"),
                "install",
                &["seo-audit".into()],
                &[],
                false,
                false
            )
            .is_err()
        );
    }
}
