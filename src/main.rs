mod design_system;

use include_dir::{include_dir, Dir, DirEntry};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

static TEMPLATES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/templates");
static PACKS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/packs");
static SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness-agents/skills");
static PRESETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/presets");
static POLICIES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/policies");
static PROFILES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/upstream/agentic-harness/profiles");

const SKIP: &[&str] = &[".git", "node_modules", "vendor", "dist", "build", ".next", ".nuxt", "target", ".venv", "venv", "coverage", "upstream"];
const CODE_EXT: &[&str] = &["py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "vue", "rs", "go", "java", "kt", "swift", "rb", "php", "cs", "c", "cc", "cpp", "h", "hpp"];
const MANIFESTS: &[&str] = &["package.json", "pyproject.toml", "requirements.txt", "Cargo.toml", "go.mod", "pom.xml", "build.gradle", "Gemfile", "composer.json"];
const LOCKFILES: &[&str] = &["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb", "uv.lock", "poetry.lock", "Cargo.lock", "go.sum", "Gemfile.lock", "composer.lock"];

fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("{}", msg.as_ref());
    std::process::exit(2)
}

fn pretty(value: Value) {
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}

fn embedded_text(dir: &Dir<'_>, path: &str) -> Option<String> {
    dir.get_file(path).and_then(|f| f.contents_utf8()).map(str::to_string)
}

fn embedded_dir<'a>(dir: &'a Dir<'a>, path: &str) -> Option<&'a Dir<'a>> {
    dir.get_dir(path)
}

fn copy_embedded(dir: &Dir<'_>, dst: &Path, preserve: bool) -> io::Result<Vec<String>> {
    fn walk(dir: &Dir<'_>, root: &Path, dst: &Path, preserve: bool, out: &mut Vec<String>) -> io::Result<()> {
        for entry in dir.entries() {
            match entry {
                DirEntry::Dir(child) => walk(child, root, dst, preserve, out)?,
                DirEntry::File(file) => {
                    let rel = file.path().strip_prefix(root).unwrap_or(file.path());
                    if rel.file_name().and_then(|x| x.to_str()) == Some("template.json") {
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

#[derive(Default)]
struct ComposeOpts {
    template: String,
    preset: Option<String>,
    profile: Option<String>,
    packs: Vec<String>,
    skills: Vec<String>,
    policies: Vec<String>,
    name: Option<String>,
    maturity: Option<String>,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn template_meta(name: &str) -> Value {
    let path = format!("{name}/template.json");
    let text = embedded_text(&TEMPLATES, &path).unwrap_or_else(|| die(format!("unknown template: {name}")));
    serde_json::from_str(&text).unwrap_or_else(|e| die(format!("invalid {path}: {e}")))
}

fn template_chain(mut name: String) -> Vec<String> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(name.clone()) {
            die("template inheritance cycle");
        }
        let meta = template_meta(&name);
        chain.push(name.clone());
        match meta.get("extends").and_then(Value::as_str) {
            Some(parent) => name = parent.to_string(),
            None => break,
        }
    }
    chain.reverse();
    chain
}

fn resolve(mut o: ComposeOpts) -> (Vec<String>, ComposeOpts) {
    if let Some(profile) = &o.profile {
        let path = format!("{profile}/profile.json");
        let text = embedded_text(&PROFILES, &path).unwrap_or_else(|| die(format!("unknown profile: {profile}")));
        let data: Value = serde_json::from_str(&text).unwrap_or_else(|e| die(format!("invalid profile {profile}: {e}")));
        if o.maturity.is_none() {
            o.maturity = data.get("maturity").and_then(Value::as_str).map(str::to_string);
        }
        o.packs.extend(strings(data.get("packs")));
        o.skills.extend(strings(data.get("skills")));
        o.policies.extend(strings(data.get("policies")));
    }

    if let Some(preset) = &o.preset {
        let path = format!("{preset}.json");
        let text = embedded_text(&PRESETS, &path).unwrap_or_else(|| die(format!("unknown preset: {preset}")));
        let data: Value = serde_json::from_str(&text).unwrap_or_else(|e| die(format!("invalid preset {preset}: {e}")));
        if let Some(template) = data.get("template").and_then(Value::as_str) {
            o.template = template.to_string();
        }
        o.packs.extend(strings(data.get("packs")));
        o.skills.extend(strings(data.get("skills")));
    }

    let chain = template_chain(o.template.clone());
    let leaf = template_meta(chain.last().unwrap());
    if o.packs.is_empty() {
        o.packs = strings(leaf.get("default_packs"));
    }
    if o.skills.is_empty() {
        o.skills = strings(leaf.get("default_skills"));
    }
    dedupe(&mut o.packs);
    dedupe(&mut o.skills);
    dedupe(&mut o.policies);
    (chain, o)
}

fn patch_manifest(path: &Path, name: Option<&str>, maturity: Option<&str>, packs: &[String]) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut text = fs::read_to_string(path)?;
    if let Some(name) = name {
        let mut replaced = false;
        text = text
            .lines()
            .map(|line| {
                if !replaced && line.trim_start().starts_with("name:") {
                    replaced = true;
                    format!("{}name: {}", &line[..line.len() - line.trim_start().len()], name)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n") + "\n";
    }
    if let Some(maturity) = maturity {
        for old in ["prototype", "startup", "production", "critical"] {
            let needle = format!("maturity: {old}");
            if text.contains(&needle) {
                text = text.replacen(&needle, &format!("maturity: {maturity}"), 1);
                break;
            }
        }
    }
    if !packs.is_empty() {
        let mut out = Vec::new();
        let mut inside = false;
        for line in text.lines() {
            if line == "packs:" {
                out.push("packs:".to_string());
                out.extend(packs.iter().map(|p| format!("  - {p}")));
                inside = true;
                continue;
            }
            if inside && line.starts_with("  - ") {
                continue;
            }
            if inside && !line.starts_with(' ') {
                inside = false;
            }
            out.push(line.to_string());
        }
        text = out.join("\n") + "\n";
    }
    fs::write(path, text)
}

fn install_modules(target: &Path, packs: &[String], skills: &[String], policies: &[String]) -> io::Result<()> {
    for (kind, names, root, dstroot) in [
        ("pack", packs, &PACKS, target.join(".agentic/packs")),
        ("skill", skills, &SKILLS, target.join(".agents/skills")),
    ] {
        for name in names {
            let dir = embedded_dir(root, name).unwrap_or_else(|| die(format!("unknown {kind}: {name}")));
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
        let file = POLICIES.get_file(&path).unwrap_or_else(|| die(format!("unknown policy: {name}")));
        let dst = target.join(".agentic/policies").join(&path);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dst, file.contents())?;
    }
    Ok(())
}

fn compose(target: &Path, o: ComposeOpts, preserve: bool) -> io::Result<Value> {
    let (chain, o) = resolve(o);
    let mut created = Vec::new();
    let base = embedded_dir(&TEMPLATES, "base").unwrap();
    created.extend(copy_embedded(base, target, true)?);
    for name in chain.iter().filter(|n| n.as_str() != "base") {
        if let Some(overlay) = embedded_dir(&TEMPLATES, &format!("{name}/overlay")) {
            created.extend(copy_embedded(overlay, target, preserve)?);
        }
    }
    install_modules(target, &o.packs, &o.skills, &o.policies)?;
    patch_manifest(&target.join("agentic.yaml"), o.name.as_deref(), o.maturity.as_deref(), &o.packs)?;
    Ok(json!({
        "templates": chain,
        "preset": o.preset,
        "profile": o.profile,
        "created": created,
        "packs": o.packs,
        "skills": o.skills,
        "policies": o.policies,
        "maturity": o.maturity
    }))
}

fn all_files(root: &Path) -> Vec<PathBuf> {
    fn walk(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(&path);
            if rel.components().any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().as_ref())) {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

fn is_test_path(s: &str) -> bool {
    let x = s.to_ascii_lowercase();
    ["test", "tests", "spec", "specs"]
        .iter()
        .any(|n| x.split(|c: char| "/_.-".contains(c)).any(|p| p == *n))
}

fn clamp(v: i64) -> i64 {
    v.clamp(0, 100)
}

fn codebase_audit(root: &Path) -> Value {
    let files = all_files(root);
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
    let mut agent = false;
    let mut ops = false;

    for path in &files {
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let ext = path.extension().unwrap_or_default().to_string_lossy();
        if ["md", "mdx", "rst", "txt"].contains(&ext.as_ref()) { docs += 1; }
        if MANIFESTS.contains(&name.as_ref()) { manifests.push(rel.clone()); }
        if LOCKFILES.contains(&name.as_ref()) { locks.push(rel.clone()); }
        if is_test_path(&rel) { tests += 1; }
        if rel.starts_with(".github/workflows/") { workflows += 1; }
        if rel.to_ascii_lowercase().contains("security") { security = true; }
        if ["AGENTS.md", "CLAUDE.md", "GEMINI.md", "agentic.yaml"].contains(&name.as_ref()) || rel.contains("/skills/") { agent = true; }
        if ["runbook", "deploy", "rollback", "observability", "monitor", "incident", "backup"].iter().any(|x| rel.to_ascii_lowercase().contains(x)) { ops = true; }
        if CODE_EXT.contains(&ext.as_ref()) {
            code += 1;
            if let Ok(text) = fs::read_to_string(path) {
                let n = text.lines().count() as i64;
                loc += n;
                if n > 800 { large.push(rel.clone()); }
                let upper = text.to_ascii_uppercase();
                for marker in ["TODO", "FIXME", "HACK", "XXX"] {
                    todos += upper.matches(marker).count() as i64;
                }
            }
        }
    }

    let ci = workflows > 0;
    let has_tests = tests > 0;
    let has_docs = docs > 0 || root.join("README.md").exists() || root.join("docs").exists();
    let has_lock = manifests.is_empty() || !locks.is_empty();
    let mut scores = BTreeMap::new();
    scores.insert("code_quality", clamp(75 - 5 * large.len() as i64 - todos.min(15)));
    scores.insert("maintainability", clamp(72 - 4 * large.len() as i64 + if has_docs { 5 } else { -8 }));
    scores.insert("architecture", if root.join("ARCHITECTURE.md").exists() || root.join("docs/architecture").exists() { 78 } else { 58 });
    scores.insert("testing", if has_tests && ci { 78 } else if has_tests { 62 } else { 38 });
    scores.insert("security", clamp((if security { 72 } else { 48 }) + if ci { 6 } else { -5 }));
    scores.insert("performance", if root.join("docs/performance.md").exists() || root.join("benchmarks").exists() { 70 } else { 55 });
    scores.insert("dependency_health", clamp((if has_lock { 78 } else { 58 }) + if ci { 4 } else { 0 }));
    scores.insert("documentation", if has_docs { 82 } else { 42 });
    scores.insert("agent_docs", if agent { 86 } else { 50 });
    scores.insert("operations", if ops && ci { 76 } else if ci { 60 } else { 40 });

    let weights = [("code_quality",12),("maintainability",12),("architecture",12),("testing",12),("security",16),("performance",6),("dependency_health",8),("documentation",8),("agent_docs",7),("operations",7)];
    let total_weight: i64 = weights.iter().map(|(_, w)| *w).sum();
    let mut overall = weights.iter().map(|(k, w)| scores[*k] * w).sum::<i64>() / total_weight;
    let mut findings = Vec::new();
    if !has_tests { findings.push(json!({"severity":"high","dimension":"testing","message":"No tests/spec files detected."})); }
    if !ci { findings.push(json!({"severity":"high","dimension":"operations","message":"No GitHub Actions workflow detected."})); }
    if !security { findings.push(json!({"severity":"high","dimension":"security","message":"No security guidance/configuration detected."})); }
    if !ops { findings.push(json!({"severity":"medium","dimension":"operations","message":"No deployment/rollback/runbook/observability material detected."})); }

    let ds = design_system::audit(root);
    if ds["active"].as_bool().unwrap_or(false) {
        let ds_score = ds["score"].as_i64().unwrap_or(0);
        scores.insert("design_system", ds_score);
        overall = ((overall as f64 * 0.90) + (ds_score as f64 * 0.10)).round() as i64;
        if let Some(violations) = ds["violations"].as_array() {
            for violation in violations {
                findings.push(json!({"severity":violation["severity"],"dimension":"design_system","message":violation["message"],"evidence":violation["evidence"]}));
            }
        }
    }

    json!({
        "overall": overall,
        "target_maturity": "unknown",
        "scores": scores,
        "readiness": {
            "prototype": clamp(overall + 15),
            "startup": clamp(overall + if has_tests && ci { 3 } else { -8 }),
            "production": clamp(overall - if has_tests && ci && security && ops { 8 } else { 20 }),
            "critical": clamp(overall - if has_tests && ci && security && ops { 22 } else { 35 })
        },
        "profile": {"root":root,"files":files.len(),"code_files":code,"code_loc":loc,"doc_files":docs,"tests_detected":tests,"workflows":workflows,"manifests":manifests,"lockfiles":locks,"large_code_files":large,"todo_markers":todos},
        "design_system": ds,
        "findings": findings,
        "checks": {"performed":["repository structure","file/LOC scan","test/CI presence","docs/security/agent/operations presence","manifest/lockfile presence","design-system compliance when active"],"not_checked":["build execution","test execution","coverage","dependency vulnerabilities","runtime performance","branch protection","deployment environment","visual regression"]}
    })
}

fn secret_scan(root: &Path) -> Value {
    let marker = ["-----BEGIN ", "PRIVATE KEY-----"].concat();
    let rsa = ["-----BEGIN RSA ", "PRIVATE KEY-----"].concat();
    let openssh = ["-----BEGIN OPENSSH ", "PRIVATE KEY-----"].concat();
    let mut findings = Vec::new();
    for path in all_files(root) {
        if fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(true) { continue; }
        if let Ok(text) = fs::read_to_string(&path) {
            for (typ, needle) in [("private_key", marker.as_str()), ("private_key", rsa.as_str()), ("private_key", openssh.as_str()), ("aws_access_key", "AKIA")] {
                for (i, line) in text.lines().enumerate() {
                    if line.contains(needle) {
                        findings.push(json!({"severity":"high","type":typ,"path":path.strip_prefix(root).unwrap_or(&path),"line":i+1}));
                    }
                }
            }
        }
    }
    json!({"passed":findings.is_empty(),"findings":findings,"note":"high-signal baseline only; use platform secret scanning/gitleaks for production"})
}

fn dir_has_file(dir: &Dir<'_>, name: &str) -> bool {
    dir.files().any(|file| file.path().file_name().and_then(|x| x.to_str()) == Some(name))
}

fn validate_repo(root: &Path) -> Value {
    let mut errors = Vec::new();
    for file in ["AGENTS.md", "agentic.yaml", "PRODUCT.md", "ARCHITECTURE.md", "DESIGN.md", "REFERENCE.md", "SECURITY.md"] {
        if !root.join(file).exists() && TEMPLATES.get_file(format!("base/{file}")).is_none() {
            errors.push(format!("base template missing {file}"));
        }
    }
    for child in PACKS.dirs() {
        if !dir_has_file(child, "PACK.md") { errors.push(format!("{} missing PACK.md", child.path().display())); }
    }
    for child in SKILLS.dirs() {
        if !dir_has_file(child, "SKILL.md") { errors.push(format!("{} missing SKILL.md", child.path().display())); }
    }
    for child in PROFILES.dirs() {
        if !dir_has_file(child, "profile.json") { errors.push(format!("{} missing profile.json", child.path().display())); }
    }
    let manifest = if root.join("agentic.yaml").exists() {
        fs::read_to_string(root.join("agentic.yaml")).unwrap_or_default()
    } else {
        embedded_text(&TEMPLATES, "base/agentic.yaml").unwrap_or_default()
    };
    for token in ["version:", "project:", "maturity:", "packs:", "agent:", "forbidden:"] {
        if !manifest.contains(token) { errors.push(format!("agentic manifest missing {token}")); }
    }
    json!({"valid":errors.is_empty(),"errors":errors,"target":root})
}

fn harness_audit(root: &Path) -> Value {
    let core = ["AGENTS.md", "agentic.yaml", "PRODUCT.md", "ARCHITECTURE.md", "DESIGN.md", "REFERENCE.md", "SECURITY.md", "docs/decisions", "docs/plans", "evals", "examples"];
    let recommended = ["docs/testing", "docs/operations", "docs/research", "docs/tasks"];
    let present: Vec<_> = core.iter().filter(|p| root.join(p).exists()).cloned().collect();
    let missing: Vec<_> = core.iter().filter(|p| !root.join(p).exists()).cloned().collect();
    let weak: Vec<_> = recommended.iter().filter(|p| !root.join(p).exists()).cloned().collect();
    let score = clamp((100 * present.len() as i64 / core.len() as i64) - 2 * weak.len() as i64);
    json!({"target":root,"score":score,"present":present,"weak":weak,"missing":missing,"conflicting":[],"recommendations":missing.iter().chain(weak.iter()).map(|p|format!("Add or resolve {p}")).collect::<Vec<_>>()})
}

fn usage(prog: &str) {
    println!("Agentic Harness\n\nusage: {prog} <command> [options]\n\ncommands:\n  init TARGET [--template NAME] [--preset NAME] [--profile NAME] [--pack NAME] [--skill NAME] [--policy NAME]\n  upgrade TARGET [same options]\n  audit [TARGET]\n  design-system-components [TARGET] [--write]\n  compare BEFORE.json AFTER.json\n  gate AUDIT.json [--min-overall N] [--min-score dimension=N]\n  validate [TARGET]\n  security-scan [TARGET]\n  harness-audit [TARGET]");
}

fn required_value(args: &[String], index: usize, flag: &str) -> String {
    args.get(index).cloned().unwrap_or_else(|| die(format!("{flag} requires value")))
}

fn parse_compose(args: &[String]) -> (PathBuf, ComposeOpts, bool) {
    if args.is_empty() { die("missing target"); }
    let target = PathBuf::from(&args[0]);
    let mut o = ComposeOpts { template: "base".into(), ..Default::default() };
    let mut allow = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--template" => { i += 1; o.template = required_value(args, i, "--template"); }
            "--preset" => { i += 1; o.preset = Some(required_value(args, i, "--preset")); }
            "--profile" => { i += 1; o.profile = Some(required_value(args, i, "--profile")); }
            "--pack" => { i += 1; o.packs.push(required_value(args, i, "--pack")); }
            "--skill" => { i += 1; o.skills.push(required_value(args, i, "--skill")); }
            "--policy" => { i += 1; o.policies.push(required_value(args, i, "--policy")); }
            "--name" => { i += 1; o.name = Some(required_value(args, i, "--name")); }
            "--maturity" => {
                i += 1;
                let m = required_value(args, i, "--maturity");
                if !["prototype", "startup", "production", "critical"].contains(&m.as_str()) {
                    die("--maturity must be prototype|startup|production|critical");
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
    if result["packs"].as_array().map(|a| a.iter().any(|x| x == "design-system")).unwrap_or(false) {
        result["design_system_plan"] = design_system::component_plan(target);
    }
}

fn main() {
    let argv: Vec<String> = env::args().collect();
    let prog = Path::new(&argv[0]).file_name().and_then(|x| x.to_str()).unwrap_or("ah");
    if argv.len() < 2 || ["-h", "--help"].contains(&argv[1].as_str()) {
        usage(prog);
        return;
    }

    let code = match argv[1].as_str() {
        "init" => {
            let (target, o, allow) = parse_compose(&argv[2..]);
            fs::create_dir_all(&target).unwrap_or_else(|e| die(e.to_string()));
            if !allow && fs::read_dir(&target).map(|mut x| x.next().is_some()).unwrap_or(false) {
                die("target is not empty; use --allow-existing or upgrade");
            }
            let mut r = compose(&target, o, false).unwrap_or_else(|e| die(e.to_string()));
            r["mode"] = json!("INIT"); r["target"] = json!(target); enrich_design_system(&target, &mut r); pretty(r); 0
        }
        "upgrade" => {
            let (target, o, _) = parse_compose(&argv[2..]);
            if !target.exists() { die("target does not exist"); }
            let mut r = compose(&target, o, true).unwrap_or_else(|e| die(e.to_string()));
            r["mode"] = json!("UPGRADE"); r["target"] = json!(target); r["preserved_existing"] = json!(true); enrich_design_system(&target, &mut r); pretty(r); 0
        }
        "audit" => {
            let path = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            if !path.exists() { die("target does not exist"); }
            let r = codebase_audit(&path);
            let fail = r["findings"].as_array().unwrap().iter().any(|f| matches!(f["severity"].as_str(), Some("high" | "critical")));
            pretty(r); if fail { 1 } else { 0 }
        }
        "design-system-components" => {
            let mut path = PathBuf::from("."); let mut write = false;
            for arg in &argv[2..] { if arg == "--write" { write = true; } else { path = PathBuf::from(arg); } }
            if !path.exists() { die("target does not exist"); }
            let mut r = design_system::component_plan(&path);
            if write { let out = design_system::write_plan(&path).unwrap_or_else(|e| die(e.to_string())); r["written"] = json!(out); }
            pretty(r); 0
        }
        "security-scan" => {
            let path = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            if !path.exists() { die("target does not exist"); }
            let r = secret_scan(&path); let pass = r["passed"].as_bool().unwrap_or(false); pretty(r); if pass { 0 } else { 1 }
        }
        "validate" => {
            let path = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            if !path.exists() { die("target does not exist"); }
            let r = validate_repo(&path); let pass = r["valid"].as_bool().unwrap_or(false); pretty(r); if pass { 0 } else { 1 }
        }
        "harness-audit" => {
            let path = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            if !path.exists() { die("target does not exist"); }
            let r = harness_audit(&path); let fail = !r["missing"].as_array().unwrap().is_empty(); pretty(r); if fail { 1 } else { 0 }
        }
        "compare" => {
            if argv.len() < 4 { die("compare requires before and after JSON"); }
            let before: Value = serde_json::from_str(&fs::read_to_string(&argv[2]).unwrap_or_else(|e| die(e.to_string()))).unwrap_or_else(|e| die(e.to_string()));
            let after: Value = serde_json::from_str(&fs::read_to_string(&argv[3]).unwrap_or_else(|e| die(e.to_string()))).unwrap_or_else(|e| die(e.to_string()));
            let mut scores = serde_json::Map::new();
            let keys: BTreeSet<_> = before["scores"].as_object().into_iter().flat_map(|m| m.keys().cloned()).chain(after["scores"].as_object().into_iter().flat_map(|m| m.keys().cloned())).collect();
            for key in keys {
                let x = before["scores"][&key].as_i64(); let y = after["scores"][&key].as_i64();
                scores.insert(key, json!({"before":x,"after":y,"delta":match(x,y){(Some(x),Some(y))=>Some(y-x),_=>None}}));
            }
            pretty(json!({"overall":{"before":before["overall"],"after":after["overall"],"delta":after["overall"].as_i64().unwrap_or(0)-before["overall"].as_i64().unwrap_or(0)},"scores":scores})); 0
        }
        "gate" => {
            if argv.len() < 3 { die("gate requires audit JSON"); }
            let data: Value = serde_json::from_str(&fs::read_to_string(&argv[2]).unwrap_or_else(|e| die(e.to_string()))).unwrap_or_else(|e| die(e.to_string()));
            let mut min = 0f64; let mut req = Vec::new(); let mut i = 3;
            while i < argv.len() {
                match argv[i].as_str() {
                    "--min-overall" => { i += 1; min = required_value(&argv, i, "--min-overall").parse().unwrap_or_else(|_| die("invalid --min-overall")); }
                    "--min-score" => { i += 1; req.push(required_value(&argv, i, "--min-score")); }
                    x => die(format!("unknown option: {x}")),
                }
                i += 1;
            }
            let mut failures = Vec::new();
            if data["overall"].as_f64().unwrap_or(0.0) < min { failures.push(format!("overall {} < {min}", data["overall"])); }
            for item in req {
                let Some((name, value)) = item.split_once('=') else { die("--min-score must be dimension=N"); };
                let value: f64 = value.parse().unwrap_or_else(|_| die("invalid --min-score value"));
                let actual = data["scores"][name].as_f64();
                if actual.map(|a| a < value).unwrap_or(true) { failures.push(format!("{name} {:?} < {value}", actual)); }
            }
            let ok = failures.is_empty(); pretty(json!({"passed":ok,"failures":failures})); if ok { 0 } else { 1 }
        }
        _ => { usage(prog); 2 }
    };
    std::process::exit(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_base_exists() {
        assert!(TEMPLATES.get_file("base/AGENTS.md").is_some());
    }

    #[test]
    fn chain_works() {
        assert_eq!(template_chain("web-app".into()).first().unwrap(), "base");
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
    fn secret_fixture() {
        let path = env::temp_dir().join(format!("ah-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("x"), ["-----BEGIN ", "PRIVATE KEY-----"].concat()).unwrap();
        assert!(!secret_scan(&path)["passed"].as_bool().unwrap());
        let _ = fs::remove_dir_all(path);
    }
}
