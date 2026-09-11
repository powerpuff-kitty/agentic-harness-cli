use crate::{architecture_score, design_system};
use include_dir::{Dir, DirEntry, include_dir};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

static BASE: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/variants/base/files");
static WEB_APP: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/variants/web-app/files");
static BACKEND_API: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/variants/backend-api/files");
static SAAS: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/variants/saas/files");
static MONOREPO: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/variants/monorepo/files");
static LIBRARY_SDK: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/variants/library-sdk/files");
static PACKS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/packs");
static POLICIES: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/policies");
static PROFILES: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/profiles");
static PRESETS: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/presets");
static VARIANTS: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/catalog/variants");
static SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness-agents/skills");

const CODE_EXT: &[&str] = &[
    "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "vue", "rs", "go", "java", "kt", "swift", "rb",
    "php", "cs", "c", "cc", "cpp", "h", "hpp",
];
const MANIFESTS: &[&str] = &[
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "Cargo.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "Gemfile",
    "composer.json",
];
const LOCKFILES: &[&str] = &[
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "uv.lock",
    "poetry.lock",
    "Cargo.lock",
    "go.sum",
    "Gemfile.lock",
    "composer.lock",
];

fn die(msg: impl AsRef<str>) -> ! {
    crate::fail(msg)
}

fn pretty(value: Value) {
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}

fn embedded_text(dir: &Dir<'_>, path: &str) -> Option<String> {
    dir.get_file(path)
        .and_then(|f| f.contents_utf8())
        .map(str::to_string)
}

fn embedded_dir<'a>(dir: &'a Dir<'a>, path: &str) -> Option<&'a Dir<'a>> {
    dir.get_dir(path)
}

fn boilerplate_dir(name: &str) -> &'static Dir<'static> {
    match name {
        "base" => &BASE,
        "web-app" => &WEB_APP,
        "backend-api" => &BACKEND_API,
        "saas" => &SAAS,
        "monorepo" => &MONOREPO,
        "library-sdk" => &LIBRARY_SDK,
        _ => die(format!("unknown boilerplate: {name}")),
    }
}

fn copy_embedded(dir: &Dir<'_>, dst: &Path, preserve: bool) -> io::Result<Vec<String>> {
    fn walk(
        dir: &Dir<'_>,
        root: &Path,
        dst: &Path,
        preserve: bool,
        out: &mut Vec<String>,
    ) -> io::Result<()> {
        for entry in dir.entries() {
            match entry {
                DirEntry::Dir(child) => walk(child, root, dst, preserve, out)?,
                DirEntry::File(file) => {
                    let rel = file.path().strip_prefix(root).unwrap_or(file.path());
                    if matches!(
                        rel.file_name().and_then(|x| x.to_str()),
                        Some("boilerplate.json" | "template.json")
                    ) {
                        continue;
                    }
                    let target = dst.join(rel);
                    if preserve && target.exists() {
                        continue;
                    }
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&target, file.contents())?;
                    out.push(rel.to_string_lossy().into_owned());
                }
            }
        }
        Ok(())
    }

    let mut out = Vec::new();
    walk(dir, dir.path(), dst, preserve, &mut out)?;
    Ok(out)
}

fn dedupe(items: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    items.retain(|x| seen.insert(x.clone()));
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn boilerplate_meta(name: &str) -> Value {
    let _ = boilerplate_dir(name);
    let text = embedded_text(&VARIANTS, &format!("{name}/variant.json"))
        .unwrap_or_else(|| die(format!("{name} missing boilerplate.json")));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| die(format!("invalid {name}/boilerplate.json: {e}")))
}

#[derive(Default)]
struct ComposeOpts {
    boilerplate: String,
    preset: Option<String>,
    profile: Option<String>,
    packs: Vec<String>,
    skills: Vec<String>,
    policies: Vec<String>,
    name: Option<String>,
    maturity: Option<String>,
    #[cfg(test)]
    fail_after_writes: Option<usize>,
}

fn resolve(mut o: ComposeOpts) -> ComposeOpts {
    if let Some(preset) = &o.preset {
        let path = format!("{preset}.json");
        let text = embedded_text(&PRESETS, &path)
            .unwrap_or_else(|| die(format!("unknown preset: {preset}")));
        let data: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| die(format!("invalid preset {preset}: {e}")));
        let selected = data
            .get("boilerplate")
            .or_else(|| data.get("template"))
            .and_then(Value::as_str);
        if let Some(name) = selected {
            o.boilerplate = name.to_string();
        }
        o.packs.extend(strings(data.get("packs")));
        o.skills.extend(strings(data.get("skills")));
        o.policies.extend(strings(data.get("policies")));
        if o.profile.is_none() {
            o.profile = data
                .get("profile")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }

    if let Some(profile) = &o.profile {
        let path = format!("{profile}/profile.json");
        let text = embedded_text(&PROFILES, &path)
            .unwrap_or_else(|| die(format!("unknown profile: {profile}")));
        let data: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| die(format!("invalid profile {profile}: {e}")));
        if o.maturity.is_none() {
            o.maturity = data
                .get("maturity")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        o.packs.extend(strings(data.get("packs")));
        o.skills.extend(strings(data.get("skills")));
        o.policies.extend(strings(data.get("policies")));
    }

    let meta = boilerplate_meta(&o.boilerplate);
    if o.packs.is_empty() {
        o.packs = strings(meta.get("default_packs"));
    }
    if o.skills.is_empty() {
        o.skills = strings(meta.get("default_skills"));
    }
    if o.policies.is_empty() {
        o.policies = strings(meta.get("default_policies"));
    }
    dedupe(&mut o.packs);
    dedupe(&mut o.skills);
    dedupe(&mut o.policies);
    o
}

fn patch_manifest(
    path: &Path,
    name: Option<&str>,
    maturity: Option<&str>,
    packs: &[String],
    skills: &[String],
    policies: &[String],
) -> io::Result<()> {
    let mut value: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(path)?).map_err(io::Error::other)?;
    if let Some(name) = name {
        value["project"]["name"] = json!(name);
    }
    if let Some(maturity) = maturity {
        value["project"]["maturity"] = json!(maturity);
    }
    value["modules"]["packs"] = json!(packs);
    value["modules"]["policies"] = json!(policies);
    value["skills"] = json!(skills);
    fs::write(
        path,
        serde_yaml_ng::to_string(&value).map_err(io::Error::other)?,
    )
}

fn install_modules(
    target: &Path,
    packs: &[String],
    skills: &[String],
    policies: &[String],
) -> io::Result<()> {
    for (kind, names, root, dstroot) in [
        ("pack", packs, &PACKS, target.join(".agentic/packs")),
        ("skill", skills, &SKILLS, target.join(".agents/skills")),
    ] {
        for name in names {
            let dir =
                embedded_dir(root, name).unwrap_or_else(|| die(format!("unknown {kind}: {name}")));
            let dst = dstroot.join(name);
            if dst.exists() {
                fs::remove_dir_all(&dst)?;
            }
            fs::create_dir_all(&dst)?;
            copy_embedded(dir, &dst, false)?;
        }
    }

    for name in policies {
        let path = format!("{name}.md");
        let file = POLICIES
            .get_file(&path)
            .unwrap_or_else(|| die(format!("unknown policy: {name}")));
        let dst = target.join(".agentic/policies").join(&path);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dst, file.contents())?;
    }
    Ok(())
}

fn compose(target: &Path, o: ComposeOpts, preserve: bool) -> io::Result<Value> {
    let explicit_modules = !o.packs.is_empty()
        || !o.skills.is_empty()
        || !o.policies.is_empty()
        || o.profile.is_some()
        || o.preset.is_some();
    let requested_name = o.name.clone();
    let requested_maturity = o.maturity.clone();
    let mut o = resolve(o);
    if preserve && !explicit_modules {
        o.packs.clear();
        o.skills.clear();
        o.policies.clear();
    }
    // Resolve all selections before creating even a temporary target.
    for (kind, names, root) in [("pack", &o.packs, &PACKS), ("skill", &o.skills, &SKILLS)] {
        for name in names {
            if embedded_dir(root, name).is_none() {
                return Err(io::Error::other(format!("unknown {kind}: {name}")));
            }
        }
    }
    for name in &o.policies {
        if POLICIES.get_file(format!("{name}.md")).is_none() {
            return Err(io::Error::other(format!("unknown policy: {name}")));
        }
    }
    if target
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(io::Error::other("symlink target is not supported"));
    }
    let existing = target.exists();
    let current = existing && target.join(".agentic/manifest.yaml").exists();
    let legacy = existing && target.join("agentic.yaml").exists();
    if current && legacy {
        return Err(io::Error::other("conflicting current and legacy manifests"));
    }
    if legacy {
        return Err(io::Error::other(
            "legacy project: explicit layout migration is required before upgrade; no files changed",
        ));
    }
    if current || legacy {
        crate::project::load(target).map_err(io::Error::other)?;
    }
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".ah-stage-")
        .tempdir_in(parent)?;
    copy_embedded(boilerplate_dir(&o.boilerplate), staging.path(), false)?;
    install_modules(staging.path(), &o.packs, &o.skills, &o.policies)?;
    patch_manifest(
        &staging.path().join(".agentic/manifest.yaml"),
        o.name.as_deref(),
        o.maturity.as_deref(),
        &o.packs,
        &o.skills,
        &o.policies,
    )?;
    if current || legacy {
        let project = crate::project::load(target).map_err(io::Error::other)?;
        for (key, name) in [
            ("product", "PRODUCT.md"),
            ("architecture", "ARCHITECTURE.md"),
            ("security", "SECURITY.md"),
            ("design", "DESIGN.md"),
            ("reference", "REFERENCE.md"),
        ] {
            let section = if current { "context" } else { "sources" };
            if project.manifest[section]
                .get(key)
                .is_some_and(|v| !v.is_null())
            {
                let destination = project.route(target, key, name).map_err(io::Error::other)?;
                let canonical = destination.canonicalize()?;
                let relative = canonical
                    .strip_prefix(target.canonicalize()?)
                    .map_err(io::Error::other)?
                    .to_path_buf();
                let default = if current {
                    PathBuf::from(".agentic").join(name)
                } else {
                    PathBuf::from(name)
                };
                if relative != default && staging.path().join(&default).exists() {
                    let staged = staging.path().join(&relative);
                    if let Some(parent) = staged.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::rename(staging.path().join(default), staged)?;
                }
            }
        }
    }
    let mut managed = BTreeMap::<String, Vec<u8>>::new();
    if current || legacy {
        let project = crate::project::load(target).map_err(io::Error::other)?;
        let original = project.manifest.clone();
        let mut value = original.clone();
        if let Some(name) = requested_name {
            value["project"]["name"] = json!(name);
        }
        if let Some(maturity) = requested_maturity.or_else(|| {
            if o.profile.is_some() {
                o.maturity.clone()
            } else {
                None
            }
        }) {
            value["project"]["maturity"] = json!(maturity);
        }
        fn merge(value: &mut Value, names: &[String]) {
            if names.is_empty() {
                return;
            }
            let mut all: Vec<String> = value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            for name in names {
                if !all.contains(name) {
                    all.push(name.clone());
                }
            }
            *value = json!(all);
        }
        if current {
            if value.get("modules").is_none() {
                value["modules"] = json!({});
            }
            merge(&mut value["modules"]["packs"], &o.packs);
            merge(&mut value["modules"]["policies"], &o.policies);
            merge(&mut value["skills"], &o.skills);
        } else {
            merge(&mut value["packs"], &o.packs);
        }
        let relative = if current {
            ".agentic/manifest.yaml"
        } else {
            "agentic.yaml"
        };
        let original_manifest = fs::read(&project.path)?;
        let bytes = if value != original {
            managed.insert(relative.to_string(), original_manifest.clone());
            serde_yaml_ng::to_string(&value)
                .map_err(io::Error::other)?
                .into_bytes()
        } else {
            original_manifest.clone()
        };
        fs::write(staging.path().join(relative), bytes)?;
    }
    write_lock(staging.path(), &o)?;
    if current {
        let relative = ".agentic/lock.json";
        let lockpath = target.join(relative);
        if lockpath.exists() {
            let original = crate::scan::read(target, &lockpath, 4_000_000)
                .map_err(io::Error::other)?
                .into_bytes();
            let mut lock: Value = serde_json::from_slice(&original).map_err(io::Error::other)?;
            if lock["format_version"] != 1
                || ["packs", "policies", "skills", "checksums"]
                    .iter()
                    .any(|k| !lock[k].is_object())
            {
                return Err(io::Error::other("invalid project lockfile"));
            }
            let previous = lock.clone();
            let incoming: Value = serde_json::from_slice(&fs::read(staging.path().join(relative))?)
                .map_err(io::Error::other)?;
            for key in ["packs", "policies", "skills"] {
                for (name, value) in incoming[key].as_object().unwrap() {
                    if lock[key].get(name).is_none() {
                        lock[key][name] = value.clone();
                    }
                }
            }
            // Keep original upstream checksums for preserved/customized files. Track additions and managed manifest only.
            for (path, hash) in incoming["checksums"].as_object().unwrap() {
                if !target.join(path).exists() || managed.contains_key(path) {
                    lock["checksums"][path] = hash.clone();
                }
            }
            let bytes = if lock != previous {
                managed.insert(relative.into(), original.clone());
                serde_json::to_vec_pretty(&lock).map_err(io::Error::other)?
            } else {
                original
            };
            fs::write(staging.path().join(relative), bytes)?;
        }
    }
    let mut incoming = Vec::new();
    collect_staged(staging.path(), staging.path(), &mut incoming)?;
    let mut created = Vec::new();
    let mut retained = Vec::new();
    let mut conflicts = Vec::new();
    if !existing {
        let staged = staging.keep();
        if let Err(e) = fs::rename(&staged, target) {
            let _ = fs::remove_dir_all(staged);
            return Err(e);
        }
        created = incoming;
    } else {
        // Existing files are never replaced implicitly. Preflight all destination ancestors.
        for relative in &incoming {
            let destination = target.join(relative);
            for ancestor in destination.ancestors().take_while(|p| *p != target) {
                if ancestor
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink())
                {
                    return Err(io::Error::other(format!(
                        "symlink destination: {}",
                        ancestor.display()
                    )));
                }
            }
            if destination.exists() {
                if !destination.is_file() {
                    return Err(io::Error::other(format!(
                        "destination is not a file: {relative}"
                    )));
                }
                if managed.contains_key(relative) {
                    continue;
                }
                retained.push(relative.clone());
                if fs::read(&destination)? != fs::read(staging.path().join(relative))? {
                    conflicts.push(relative.clone());
                }
            }
        }
        let mut dirs = Vec::new();
        let mut replaced = Vec::new();
        let write_result = (|| -> io::Result<()> {
            use std::io::Write;
            for relative in &incoming {
                if retained.contains(relative) || managed.contains_key(relative) {
                    continue;
                }
                #[cfg(test)]
                if o.fail_after_writes == Some(created.len() + replaced.len()) {
                    return Err(io::Error::other("injected write failure"));
                }
                let destination = target.join(relative);
                let mut missing: Vec<_> = destination
                    .parent()
                    .unwrap()
                    .ancestors()
                    .take_while(|p| !p.exists())
                    .map(Path::to_path_buf)
                    .collect();
                missing.reverse();
                for dir in missing {
                    fs::create_dir(&dir)?;
                    dirs.push(dir);
                }
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination)?;
                created.push(relative.clone());
                file.write_all(&fs::read(staging.path().join(relative))?)?;
                file.sync_all()?;
            }
            for (relative, original) in &managed {
                if fs::read(target.join(relative))? != *original {
                    return Err(io::Error::other(
                        "managed metadata changed concurrently; retry after inspection",
                    ));
                }
            }
            for relative in managed.keys() {
                #[cfg(test)]
                if o.fail_after_writes == Some(created.len() + replaced.len()) {
                    return Err(io::Error::other("injected write failure"));
                }
                crate::scan::write(target, relative, &fs::read(staging.path().join(relative))?)?;
                replaced.push(relative.clone());
            }
            Ok(())
        })();
        if let Err(e) = write_result {
            let mut restoration_errors = Vec::new();
            for relative in replaced.iter().rev() {
                if let Err(error) = crate::scan::write(target, relative, &managed[relative]) {
                    restoration_errors.push(format!("{relative}: {error}"));
                }
            }
            for path in created.iter().rev() {
                let _ = fs::remove_file(target.join(path));
            }
            for dir in dirs.iter().rev() {
                let _ = fs::remove_dir(dir);
            }
            if !restoration_errors.is_empty() {
                return Err(io::Error::other(format!(
                    "{e}; metadata restoration failed: {restoration_errors:?}"
                )));
            }
            return Err(e);
        }
    }
    Ok(
        json!({"format_version":1,"kind":"composition","boilerplate":o.boilerplate,"preset":o.preset,"profile":o.profile,"created":created,"preserved":retained,"conflicts":conflicts,"removed":[],"updated":managed.keys().collect::<Vec<_>>(),"packs":o.packs,"skills":o.skills,"policies":o.policies,"maturity":o.maturity,"preserve_requested":preserve,"conflict_policy":"existing content retained; reconcile conflicts explicitly"}),
    )
}
fn collect_staged(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_staged(root, &path, out)?;
        } else {
            out.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    out.sort();
    Ok(())
}
fn write_lock(root: &Path, options: &ComposeOpts) -> io::Result<()> {
    let sources: Value =
        serde_json::from_str(include_str!("../upstream.lock.json")).map_err(io::Error::other)?;
    let modules = |names: &[String], source: &str| -> Value {
        Value::Object(
            names
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        json!({"source_commit":sources[source]["commit"]}),
                    )
                })
                .collect(),
        )
    };
    use sha2::{Digest, Sha256};
    let mut paths = Vec::new();
    collect_staged(root, root, &mut paths)?;
    let mut checksums = serde_json::Map::new();
    for path in paths {
        if path != ".agentic/lock.json" {
            checksums.insert(
                path.clone(),
                json!(format!(
                    "sha256:{:x}",
                    Sha256::digest(fs::read(root.join(path))?)
                )),
            );
        }
    }
    let lock = json!({"format_version":1,"canonical_source":sources["canonical"],"agents_source":sources["agents"],"variant":options.boilerplate,"packs":modules(&options.packs,"canonical"),"policies":modules(&options.policies,"canonical"),"skills":modules(&options.skills,"agents"),"checksums":checksums});
    fs::write(
        root.join(".agentic/lock.json"),
        serde_json::to_vec_pretty(&lock).map_err(io::Error::other)?,
    )
}

fn is_test_path(s: &str) -> bool {
    let x = s.to_ascii_lowercase();
    ["test", "tests", "spec", "specs"]
        .iter()
        .any(|n| x.split(|c: char| "/_.-".contains(c)).any(|p| p == *n))
}

fn codebase_audit(root: &Path) -> Value {
    let mut inventory = crate::scan::inventory(root);
    let files = inventory.files.clone();
    let mut code = 0;
    let mut loc = 0i64;
    let mut large = Vec::new();
    let mut todos = 0i64;
    let mut tests = 0;
    let mut workflows = 0;
    let mut docs = 0;
    let mut manifests = Vec::new();
    let mut locks = Vec::new();
    let mut security = false;
    let mut discovered_checks = Vec::new();
    let mut agent = false;
    let mut ops = false;

    for path in &files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let ext = path.extension().unwrap_or_default().to_string_lossy();
        if ["md", "mdx", "rst", "txt"].contains(&ext.as_ref()) {
            docs += 1;
        }
        if MANIFESTS.contains(&name.as_ref()) {
            manifests.push(rel.clone());
        }
        if name == "package.json"
            && let Ok(text) = crate::scan::read(root, path, 1_000_000)
            && let Ok(package) = serde_json::from_str::<Value>(&text)
            && let Some(scripts) = package["scripts"].as_object()
        {
            for (name, command) in scripts {
                if ["test", "lint", "check", "typecheck", "bench", "benchmark"]
                    .iter()
                    .any(|prefix| name == prefix || name.starts_with(&format!("{prefix}:")))
                {
                    discovered_checks
                        .push(json!({"path":rel,"name":name,"command":command,"executed":false}));
                }
            }
        }
        if rel
            .split('/')
            .any(|s| ["scripts", "benchmarks", "benches"].contains(&s))
            && ["test", "check", "lint", "bench", "benchmark"]
                .iter()
                .any(|s| name.contains(s))
        {
            discovered_checks.push(json!({"path":rel,"executed":false}));
        }
        if LOCKFILES.contains(&name.as_ref()) {
            locks.push(rel.clone());
        }
        if is_test_path(&rel) {
            tests += 1;
        }
        if rel.starts_with(".github/workflows/") {
            workflows += 1;
        }
        if rel.to_ascii_lowercase().contains("security") {
            security = true;
        }
        if ["AGENTS.md", "CLAUDE.md", "GEMINI.md", "agentic.yaml"].contains(&name.as_ref())
            || rel.contains("/skills/")
        {
            agent = true;
        }
        if [
            "runbook",
            "deploy",
            "rollback",
            "observability",
            "monitor",
            "incident",
            "backup",
        ]
        .iter()
        .any(|x| rel.to_ascii_lowercase().contains(x))
        {
            ops = true;
        }
        if CODE_EXT.contains(&ext.as_ref()) && crate::scan::product(root, path) {
            code += 1;
            match crate::scan::read(root, path, 2_000_000) {
                Err(reason) => inventory.errors.push(format!("{rel}: {reason}")),
                Ok(text) => {
                    let n = text.lines().count() as i64;
                    loc += n;
                    if n > 800 {
                        large.push(rel.clone());
                    }
                    let upper = text.to_ascii_uppercase();
                    for marker in ["TODO", "FIXME", "HACK", "XXX"] {
                        todos += upper.matches(marker).count() as i64;
                    }
                }
            }
        }
    }

    let ci = workflows > 0;
    let has_tests = tests > 0;
    let has_docs = docs > 0 || root.join("README.md").exists() || root.join("docs").exists();
    let has_lock = manifests.is_empty() || !locks.is_empty();
    let architecture = architecture_score::audit(root);
    let measured_architecture_score = architecture["score"].clone();
    let mut scores = BTreeMap::<&str, Value>::new();
    for dimension in [
        "code_quality",
        "maintainability",
        "testing",
        "security",
        "performance",
        "dependency_health",
        "documentation",
        "agent_docs",
        "operations",
    ] {
        scores.insert(dimension, Value::Null);
    }
    scores.insert(
        "architecture",
        if architecture["graph"]["source_files"].as_u64().unwrap_or(0) > 0 {
            measured_architecture_score
        } else {
            Value::Null
        },
    );
    let mut findings = Vec::new();
    if !inventory.errors.is_empty() {
        findings.push(json!({"severity":"high","dimension":"scan","message":"Repository evidence is incomplete.","evidence":inventory.errors}));
    }
    if !has_tests {
        findings.push(json!({"severity":"high","dimension":"testing","message":"No tests/spec files detected."}));
    }
    if !ci {
        findings.push(json!({"severity":"high","dimension":"operations","message":"No GitHub Actions workflow detected."}));
    }
    if !security {
        findings.push(json!({"severity":"high","dimension":"security","message":"No security guidance/configuration detected."}));
    }
    if !ops {
        findings.push(json!({"severity":"medium","dimension":"operations","message":"No deployment/rollback/runbook/observability material detected."}));
    }
    findings.extend(architecture_score::codebase_findings(&architecture));

    let ds = design_system::audit(root);
    if ds["active"].as_bool().unwrap_or(false) {
        scores.insert("design_system", Value::Null);
        if let Some(violations) = ds["violations"].as_array() {
            for violation in violations {
                findings.push(json!({"severity":violation["severity"],"dimension":"design_system","message":violation["message"],"evidence":violation["evidence"]}));
            }
        }
    }

    json!({
        "format_version":2,"kind":"codebase-audit",
        "overall": null,
        "target_maturity": crate::project::load(root).ok().and_then(|p|p.manifest["project"]["maturity"].as_str().map(str::to_string)).unwrap_or_else(||"unknown".into()),
        "scores": scores,
        "readiness": {"prototype":null,"startup":null,"production":null,"critical":null},
        "discovered_checks":discovered_checks,
        "score_provenance":{"confidence":"unvalidated heuristic for architecture; unavailable for other dimensions","version":2,"overall":"not measured","unmeasured_dimensions":"null; file presence does not establish quality or readiness","architecture":"heuristic source-violation indicator; see coverage and findings"},
        "signals":{"has_tests":has_tests,"has_ci":ci,"has_documentation":has_docs,"has_lockfile":has_lock,"has_security_material":security,"has_agent_docs":agent,"has_operations_material":ops},
        "scan":inventory.report(),
        "profile": {"root":root,"files":files.len(),"code_files":code,"code_loc":loc,"doc_files":docs,"tests_detected":tests,"workflows":workflows,"manifests":manifests,"lockfiles":locks,"large_code_files":large,"todo_markers":todos},
        "architecture": architecture,
        "design_system": ds,
        "findings": findings,
        "checks": {"performed":["repository structure","file/LOC scan","test/CI presence","docs/security/agent/operations presence","manifest/lockfile presence","architecture source dependency and boundary analysis","design-system compliance when active"],"not_checked":["build execution","test execution","coverage","dependency vulnerabilities","runtime performance","branch protection","deployment environment","visual regression"]}
    })
}

fn secret_scan(root: &Path) -> Value {
    let inventory = crate::scan::inventory(root);
    let mut findings = Vec::new();
    let mut skipped = inventory.skipped.clone();
    let aws = regex::Regex::new(r"(?:^|[^A-Z0-9])AKIA[A-Z0-9]{16}(?:$|[^A-Z0-9])").unwrap();
    let headers = [
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
    ]
    .map(|s| format!("-----BEGIN {s}-----"));
    for path in &inventory.files {
        match crate::scan::read(root, path, 1_000_000) {
            Err(reason) => skipped
                .push(json!({"path":path.strip_prefix(root).unwrap_or(path),"reason":reason})),
            Ok(text) => {
                for (i, line) in text.lines().enumerate() {
                    for typ in [
                        if aws.is_match(line) {
                            Some("aws_access_key")
                        } else {
                            None
                        },
                        if headers.iter().any(|h| line.contains(h)) {
                            Some("private_key")
                        } else {
                            None
                        },
                    ]
                    .into_iter()
                    .flatten()
                    {
                        findings.push(json!({"severity":"high","type":typ,"path":path.strip_prefix(root).unwrap_or(path),"line":i+1}));
                    }
                }
            }
        }
    }
    json!({"format_version":1,"kind":"secret-scan","passed":findings.is_empty() && inventory.errors.is_empty(),"findings":findings,"scan":inventory.report(),"skipped":skipped,"note":"baseline marker scan of eligible files; ignored/generated/large/binary files are not verified; not a production security assessment"})
}

fn dir_has_file(dir: &Dir<'_>, name: &str) -> bool {
    dir.files()
        .any(|file| file.path().file_name().and_then(|x| x.to_str()) == Some(name))
}

fn validate_repo(root: &Path) -> Value {
    crate::project::validate(root)
}
fn catalog_check() -> Value {
    let mut errors = Vec::new();
    for name in [
        "base",
        "web-app",
        "backend-api",
        "saas",
        "monorepo",
        "library-sdk",
    ] {
        for file in [
            "AGENTS.md",
            ".agentic/manifest.yaml",
            ".agentic/PRODUCT.md",
            ".agentic/ARCHITECTURE.md",
            ".agentic/SECURITY.md",
        ] {
            if boilerplate_dir(name).get_file(file).is_none() {
                errors.push(format!("{name} missing {file}"));
            }
        }
    }
    for child in PACKS.dirs() {
        if !dir_has_file(child, "PACK.md") {
            errors.push(format!("{} missing PACK.md", child.path().display()));
        }
    }
    json!({"valid":errors.is_empty(),"errors":errors,"kind":"catalog-validation","format_version":1})
}

fn harness_audit(root: &Path) -> Value {
    let validation = crate::project::validate(root);
    let missing = validation["errors"].as_array().cloned().unwrap_or_default();
    json!({"format_version":1,"kind":"harness-audit","target":root,"score":if missing.is_empty(){Some(100)}else{None},"present":[],"weak":[],"missing":missing,"conflicting":[],"recommendations":validation["errors"],"coverage":"context presence and route validity only"})
}

fn usage(prog: &str) {
    println!(
        "Agentic Harness\n\nusage: {prog} <command> [options]\n\ncommands:\n  architecture <detect|analyze|enforce> [TARGET]\n  design <analyze|preserve|diff|prompt> [options]\n  agentic <audit|context|skills|models|compare|improve|migrate> [options] (experimental)\n  catalog-check\n  init TARGET [--boilerplate NAME] [--preset NAME] [--profile NAME] [--pack NAME] [--skill NAME] [--policy NAME]\n  upgrade TARGET [same options]\n  audit [TARGET]\n  design-system-components [TARGET] [--write]\n  compare BEFORE.json AFTER.json\n  gate AUDIT.json [--min-overall N] [--min-score dimension=N] [--max-architecture-errors N] [--fail-on-architecture-error]\n  validate [TARGET]\n  security-scan [TARGET]\n  harness-audit [TARGET]\n\ncompatibility:\n  --version prints build/catalog identity. Audit v2 uses null for unmeasured scores.\n  --template NAME is retained as an alias for --boilerplate NAME"
    );
}

fn required_value(args: &[String], index: usize, flag: &str) -> String {
    args.get(index)
        .cloned()
        .unwrap_or_else(|| die(format!("{flag} requires value")))
}

fn parse_compose(args: &[String]) -> (PathBuf, ComposeOpts, bool) {
    if args.is_empty() {
        die("missing target");
    }
    let target = PathBuf::from(&args[0]);
    let mut o = ComposeOpts {
        boilerplate: "base".into(),
        ..Default::default()
    };
    let mut allow = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--boilerplate" | "--template" => {
                let flag = args[i].clone();
                i += 1;
                o.boilerplate = required_value(args, i, &flag);
            }
            "--preset" => {
                i += 1;
                o.preset = Some(required_value(args, i, "--preset"));
            }
            "--profile" => {
                i += 1;
                o.profile = Some(required_value(args, i, "--profile"));
            }
            "--pack" => {
                i += 1;
                o.packs.push(required_value(args, i, "--pack"));
            }
            "--skill" => {
                i += 1;
                o.skills.push(required_value(args, i, "--skill"));
            }
            "--policy" => {
                i += 1;
                o.policies.push(required_value(args, i, "--policy"));
            }
            "--name" => {
                i += 1;
                o.name = Some(required_value(args, i, "--name"));
            }
            "--maturity" => {
                i += 1;
                let m = required_value(args, i, "--maturity");
                if !["prototype", "startup", "production", "critical", "beta"].contains(&m.as_str())
                {
                    die("--maturity must be prototype|startup|production|critical|beta");
                }
                o.maturity = Some(m);
            }
            "--allow-existing" => allow = true,
            x => die(format!("unknown option: {x}")),
        }
        i += 1;
    }
    (target, o, allow)
}

fn enrich_design_system(target: &Path, result: &mut Value) {
    if result["packs"]
        .as_array()
        .map(|a| a.iter().any(|x| x == "design-system"))
        .unwrap_or(false)
    {
        result["design_system_plan"] = design_system::component_plan(target);
    }
}

pub fn run(argv: Vec<String>) {
    let prog = Path::new(&argv[0])
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("ah");
    if argv.len() < 2 || ["-h", "--help"].contains(&argv[1].as_str()) {
        usage(prog);
        return;
    }

    let code = match argv[1].as_str() {
        "catalog-check" => {
            let r = catalog_check();
            let ok = r["valid"] == true;
            pretty(r);
            if ok { 0 } else { 1 }
        }
        "init" => {
            let (target, o, allow) = parse_compose(&argv[2..]);
            if !allow
                && fs::read_dir(&target)
                    .map(|mut x| x.next().is_some())
                    .unwrap_or(false)
            {
                die("target is not empty; use --allow-existing or upgrade");
            }
            let mut r = compose(&target, o, false).unwrap_or_else(|e| die(e.to_string()));
            r["mode"] = json!("INIT");
            r["target"] = json!(target);
            enrich_design_system(&target, &mut r);
            pretty(r);
            0
        }
        "upgrade" => {
            let (target, o, _) = parse_compose(&argv[2..]);
            crate::scan::require_directory(&target).unwrap_or_else(|e| die(e));
            let mut r = compose(&target, o, true).unwrap_or_else(|e| die(e.to_string()));
            r["mode"] = json!("UPGRADE");
            r["target"] = json!(target);
            r["preserved_existing"] = json!(true);
            enrich_design_system(&target, &mut r);
            pretty(r);
            0
        }
        "audit" => {
            let path = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            crate::scan::require_directory(&path).unwrap_or_else(|e| die(e));
            let r = codebase_audit(&path);
            let fail = r["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| matches!(f["severity"].as_str(), Some("high" | "critical")));
            pretty(r);
            if fail { 1 } else { 0 }
        }
        "design-system-components" => {
            let mut path = PathBuf::from(".");
            let mut write = false;
            for arg in &argv[2..] {
                if arg == "--write" {
                    write = true;
                } else {
                    path = PathBuf::from(arg);
                }
            }
            crate::scan::require_directory(&path).unwrap_or_else(|e| die(e));
            let mut r = design_system::component_plan(&path);
            if write {
                let out = design_system::write_plan(&path).unwrap_or_else(|e| die(e.to_string()));
                r["written"] = json!(out);
            }
            pretty(r);
            0
        }
        "security-scan" => {
            let path = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            crate::scan::require_directory(&path).unwrap_or_else(|e| die(e));
            let r = secret_scan(&path);
            let pass = r["passed"].as_bool().unwrap_or(false);
            pretty(r);
            if pass { 0 } else { 1 }
        }
        "validate" => {
            let path = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            crate::scan::require_directory(&path).unwrap_or_else(|e| die(e));
            let r = validate_repo(&path);
            let pass = r["valid"].as_bool().unwrap_or(false);
            pretty(r);
            if pass { 0 } else { 1 }
        }
        "harness-audit" => {
            let path = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            crate::scan::require_directory(&path).unwrap_or_else(|e| die(e));
            let r = harness_audit(&path);
            let fail = !r["missing"].as_array().unwrap().is_empty();
            pretty(r);
            if fail { 1 } else { 0 }
        }
        "compare" => {
            if argv.len() < 4 {
                die("compare requires before and after JSON");
            }
            let before: Value = serde_json::from_str(
                &fs::read_to_string(&argv[2]).unwrap_or_else(|e| die(e.to_string())),
            )
            .unwrap_or_else(|e| die(e.to_string()));
            let after: Value = serde_json::from_str(
                &fs::read_to_string(&argv[3]).unwrap_or_else(|e| die(e.to_string())),
            )
            .unwrap_or_else(|e| die(e.to_string()));
            crate::artifact::validate(&before).unwrap_or_else(|e| die(e));
            crate::artifact::validate(&after).unwrap_or_else(|e| die(e));
            if before.get("format_version") != after.get("format_version") {
                die("cannot compare incompatible audit versions");
            }
            let mut scores = serde_json::Map::new();
            let keys: BTreeSet<_> = before["scores"]
                .as_object()
                .into_iter()
                .flat_map(|m| m.keys().cloned())
                .chain(
                    after["scores"]
                        .as_object()
                        .into_iter()
                        .flat_map(|m| m.keys().cloned()),
                )
                .collect();
            for key in keys {
                let x = before["scores"][&key].as_f64();
                let y = after["scores"][&key].as_f64();
                scores.insert(key, json!({"before":x,"after":y,"delta":match(x,y){(Some(x),Some(y))=>Some(y-x),_=>None}}));
            }
            pretty(
                json!({"overall":{"before":before["overall"],"after":after["overall"],"delta":after["overall"].as_f64().zip(before["overall"].as_f64()).map(|(a,b)|a-b)},"scores":scores}),
            );
            0
        }
        "gate" => {
            if argv.len() < 3 {
                die("gate requires audit JSON");
            }
            let data: Value = serde_json::from_str(
                &fs::read_to_string(&argv[2]).unwrap_or_else(|e| die(e.to_string())),
            )
            .unwrap_or_else(|e| die(e.to_string()));
            let mut min: Option<f64> = None;
            let mut req = Vec::new();
            let mut max_architecture_errors = None;
            let mut i = 3;
            while i < argv.len() {
                match argv[i].as_str() {
                    "--min-overall" => {
                        i += 1;
                        min = Some(
                            crate::artifact::threshold(&required_value(&argv, i, "--min-overall"))
                                .unwrap_or_else(|e| die(e)),
                        );
                    }
                    "--min-score" => {
                        i += 1;
                        req.push(required_value(&argv, i, "--min-score"));
                    }
                    "--max-architecture-errors" => {
                        i += 1;
                        max_architecture_errors = Some(
                            required_value(&argv, i, "--max-architecture-errors")
                                .parse::<u64>()
                                .unwrap_or_else(|_| die("invalid --max-architecture-errors")),
                        );
                    }
                    "--fail-on-architecture-error" => {
                        max_architecture_errors = Some(0);
                    }
                    x => die(format!("unknown option: {x}")),
                }
                i += 1;
            }
            crate::artifact::validate(&data).unwrap_or_else(|e| die(e));
            let mut failures = Vec::new();
            if data["scan"]["complete"] == false
                || data["architecture"]["compliance"]["complete"] == false
            {
                failures.push("audit evidence is incomplete".into());
            }
            if let Some(min) = min
                && data["overall"].as_f64().is_none_or(|n| n < min)
            {
                failures.push(format!("overall is unmeasured or below {min}"));
            }
            for item in req {
                let Some((name, value)) = item.split_once('=') else {
                    die("--min-score must be dimension=N");
                };
                let value = crate::artifact::threshold(value).unwrap_or_else(|e| die(e));
                if data["scores"].get(name).is_none() {
                    die(format!("unknown score dimension: {name}"));
                }
                let actual = data["scores"][name].as_f64();
                if actual.map(|a| a < value).unwrap_or(true) {
                    failures.push(format!("{name} {:?} < {value}", actual));
                }
            }
            if let Some(max_errors) = max_architecture_errors
                && let Some(failure) = architecture_score::gate_failure(&data, max_errors)
            {
                failures.push(failure);
            }
            let ok = failures.is_empty();
            pretty(json!({"passed":ok,"failures":failures}));
            if ok { 0 } else { 1 }
        }
        _ => {
            usage(prog);
            2
        }
    };
    std::process::exit(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_metadata_upgrade_restores_original_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        compose(
            &root,
            ComposeOpts {
                boilerplate: "base".into(),
                ..Default::default()
            },
            false,
        )
        .unwrap();
        let manifest = fs::read(root.join(".agentic/manifest.yaml")).unwrap();
        let lock = fs::read(root.join(".agentic/lock.json")).unwrap();
        let result = compose(
            &root,
            ComposeOpts {
                boilerplate: "base".into(),
                name: Some("Changed".into()),
                fail_after_writes: Some(1),
                ..Default::default()
            },
            true,
        );
        assert!(result.unwrap_err().to_string().contains("injected"));
        assert_eq!(
            fs::read(root.join(".agentic/manifest.yaml")).unwrap(),
            manifest
        );
        assert_eq!(fs::read(root.join(".agentic/lock.json")).unwrap(), lock);
    }

    #[test]
    fn all_catalog_selections_generate_valid_projects() {
        let temp = tempfile::tempdir().unwrap();
        let mut options = Vec::new();
        for dir in PACKS.dirs() {
            options.push(ComposeOpts {
                packs: vec![dir.path().file_name().unwrap().to_string_lossy().into()],
                ..Default::default()
            });
        }
        for dir in PROFILES.dirs() {
            options.push(ComposeOpts {
                profile: Some(dir.path().file_name().unwrap().to_string_lossy().into()),
                ..Default::default()
            });
        }
        for file in PRESETS
            .files()
            .filter(|f| f.path().extension().is_some_and(|e| e == "json"))
        {
            options.push(ComposeOpts {
                preset: Some(file.path().file_stem().unwrap().to_string_lossy().into()),
                ..Default::default()
            });
        }
        for dir in SKILLS.dirs().filter(|d| dir_has_file(d, "SKILL.md")) {
            options.push(ComposeOpts {
                skills: vec![dir.path().file_name().unwrap().to_string_lossy().into()],
                ..Default::default()
            });
        }
        for (i, mut option) in options.into_iter().enumerate() {
            option.boilerplate = "base".into();
            let root = temp.path().join(i.to_string());
            compose(&root, option, false).unwrap();
            let result = crate::project::validate(&root);
            assert_eq!(result["valid"], true, "{result}");
        }
    }

    #[test]
    fn embedded_boilerplates_are_materialized() {
        for name in [
            "base",
            "web-app",
            "backend-api",
            "saas",
            "monorepo",
            "library-sdk",
        ] {
            let dir = boilerplate_dir(name);
            assert!(boilerplate_meta(name).is_object());
            assert!(dir.get_file("AGENTS.md").is_some());
            assert!(dir.get_file(".agentic/manifest.yaml").is_some());
        }
    }

    #[test]
    fn profile_exists() {
        assert!(PROFILES.get_file("startup/profile.json").is_some());
    }

    #[test]
    fn agent_skill_exists() {
        assert!(SKILLS.get_file("agentic-app/SKILL.md").is_some());
    }

    #[test]
    fn boilerplate_flag_and_template_alias_match() {
        let (_, preferred, _) =
            parse_compose(&["x".into(), "--boilerplate".into(), "web-app".into()]);
        let (_, legacy, _) = parse_compose(&["x".into(), "--template".into(), "web-app".into()]);
        assert_eq!(preferred.boilerplate, legacy.boilerplate);
    }

    #[test]
    fn preset_uses_boilerplate_field() {
        let o = ComposeOpts {
            boilerplate: "base".into(),
            preset: Some("vue-saas".into()),
            ..Default::default()
        };
        assert_eq!(resolve(o).boilerplate, "saas");
    }

    #[test]
    fn generated_project_does_not_leak_boilerplate_metadata() {
        let path = env::temp_dir().join(format!("ah-compose-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        let opts = ComposeOpts {
            boilerplate: "web-app".into(),
            ..Default::default()
        };
        compose(&path, opts, false).unwrap();
        assert!(!path.join("boilerplate.json").exists());
        assert!(path.join("AGENTS.md").exists());
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn secret_fixture() {
        let path = env::temp_dir().join(format!("ah-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("x"), ["-----BEGIN ", "PRIVATE KEY-----"].concat()).unwrap();
        assert!(!secret_scan(&path)["passed"].as_bool().unwrap());
        let _ = fs::remove_dir_all(path);
    }
}
