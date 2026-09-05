use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SKIP: &[&str] = &[".git", "node_modules", "vendor", "dist", "build", ".next", ".nuxt", "target", ".venv", "venv", "coverage", "upstream"];

#[derive(Clone)]
struct Finding {
    rule: &'static str,
    severity: &'static str,
    dimension: &'static str,
    message: String,
    evidence: Vec<String>,
    remediation: &'static str,
    confidence: &'static str,
}

impl Finding {
    fn value(&self) -> Value {
        json!({
            "rule": self.rule,
            "severity": self.severity,
            "dimension": self.dimension,
            "message": self.message,
            "evidence": self.evidence,
            "remediation": self.remediation,
            "confidence": self.confidence
        })
    }
}

fn all_files(root: &Path) -> Vec<PathBuf> {
    fn walk(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(&path);
            if rel.components().any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().as_ref())) { continue; }
            if path.is_dir() { walk(&path, root, out); } else { out.push(path); }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

fn text(path: &Path) -> String { fs::read_to_string(path).unwrap_or_default() }
fn rel(root: &Path, path: &Path) -> String { path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/") }
fn words(s: &str) -> BTreeSet<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '-')
        .map(str::to_ascii_lowercase)
        .filter(|w| w.len() >= 4)
        .collect()
}
fn overlap(a: &str, b: &str) -> f64 {
    let a = words(a); let b = words(b);
    if a.is_empty() || b.is_empty() { return 0.0; }
    let common = a.intersection(&b).count() as f64;
    common / (a.len().min(b.len()) as f64)
}
fn severity_penalty(s: &str) -> i64 { match s { "critical" => 25, "high" => 15, "medium" => 7, _ => 3 } }
fn clamp(v: i64) -> i64 { v.clamp(0, 100) }

fn audit(root: &Path) -> Value {
    let files = all_files(root);
    let mut findings = Vec::<Finding>::new();
    let agents = root.join("AGENTS.md");
    let agents_text = text(&agents);
    let agent_lines = agents_text.lines().count();
    let agent_words = agents_text.split_whitespace().count();

    if !agents.exists() {
        findings.push(Finding { rule:"AH-AGENTS-002", severity:"high", dimension:"agents_md_quality", message:"No root AGENTS.md found.".into(), evidence:vec![], remediation:"Add a compact root AGENTS.md that routes to canonical project context.", confidence:"deterministic" });
    } else {
        let lower = agents_text.to_ascii_lowercase();
        let mandatory = agents_text.lines().filter(|l| {
            let x = l.to_ascii_lowercase();
            (x.contains("read ") || x.contains("load ")) && !x.contains("relevant") && !x.contains("when ") && !x.contains("if ")
        }).count();
        if mandatory >= 4 {
            findings.push(Finding { rule:"AH-CONTEXT-001", severity:"high", dimension:"context_architecture", message:format!("AGENTS.md contains {mandatory} apparently unconditional read/load directives."), evidence:vec!["AGENTS.md".into()], remediation:"Replace broad mandatory reads with task-scoped context routes.", confidence:"heuristic" });
        }
        if agent_words > 1200 || agent_lines > 180 {
            findings.push(Finding { rule:"AH-CONTEXT-002", severity:"medium", dimension:"context_architecture", message:format!("AGENTS.md is large ({agent_words} words, {agent_lines} lines)."), evidence:vec!["AGENTS.md".into()], remediation:"Keep AGENTS.md as a compact entrypoint and move detail behind progressive disclosure.", confidence:"deterministic" });
        }
        if !(lower.contains("complete") || lower.contains("completion") || lower.contains("before declaring") || lower.contains("done")) {
            findings.push(Finding { rule:"AH-DONE-001", severity:"medium", dimension:"completion_semantics", message:"No clear completion/verification semantics detected in AGENTS.md.".into(), evidence:vec!["AGENTS.md".into()], remediation:"Define what completion means, including affected verification and unresolved gaps.", confidence:"heuristic" });
        }
        if !(lower.contains("destructive") || lower.contains("release") || lower.contains("production") || lower.contains("secret")) {
            findings.push(Finding { rule:"AH-AUTONOMY-001", severity:"high", dimension:"decision_boundaries", message:"No obvious approval boundary for destructive/release/secret operations.".into(), evidence:vec!["AGENTS.md".into()], remediation:"Define safe autonomy and explicit approval-required actions.", confidence:"heuristic" });
        }
    }

    let has_agentic = root.join(".agentic").exists();
    let has_router = root.join(".agentic/README.md").exists() || root.join(".agentic/INDEX.md").exists();
    if has_agentic && !has_router {
        findings.push(Finding { rule:"AH-CONTEXT-003", severity:"medium", dimension:"documentation_routing", message:".agentic/ exists without README.md or INDEX.md context router.".into(), evidence:vec![".agentic/".into()], remediation:"Add a concise task-scoped context map.", confidence:"deterministic" });
    }

    let mut skills = Vec::<(String,String)>::new();
    for path in &files {
        let r = rel(root, path);
        if r.ends_with("/SKILL.md") || r == "SKILL.md" {
            let t = text(path);
            let intro = t.lines().take(12).collect::<Vec<_>>().join(" ");
            skills.push((r, intro));
        }
    }
    let mut overlap_pairs = Vec::new();
    for i in 0..skills.len() {
        for j in (i+1)..skills.len() {
            let score = overlap(&skills[i].1, &skills[j].1);
            if score >= 0.65 { overlap_pairs.push((skills[i].0.clone(), skills[j].0.clone(), score)); }
        }
    }
    if !overlap_pairs.is_empty() {
        findings.push(Finding { rule:"AH-SKILL-001", severity:"high", dimension:"skill_architecture", message:format!("Detected {} highly overlapping skill-description pair(s).", overlap_pairs.len()), evidence:overlap_pairs.iter().take(8).map(|(a,b,s)| format!("{a} <-> {b} ({s:.2})")).collect(), remediation:"Narrow skill triggers, merge duplicates, or convert broad domain skills into routers.", confidence:"heuristic" });
    }
    if skills.len() > 30 {
        findings.push(Finding { rule:"AH-SKILL-002", severity:"medium", dimension:"skill_architecture", message:format!("{} skills are discoverable in the repository.", skills.len()), evidence:vec!["skills/".into(), ".agents/skills/".into()], remediation:"Reduce globally discoverable skills or improve narrow trigger-oriented discovery.", confidence:"heuristic" });
    }

    let vendor_files = ["CLAUDE.md", "GEMINI.md", ".github/copilot-instructions.md"];
    for vf in vendor_files {
        let p = root.join(vf);
        if p.exists() {
            let t = text(&p);
            if t.split_whitespace().count() > 700 {
                findings.push(Finding { rule:"AH-PORT-001", severity:"medium", dimension:"model_portability", message:format!("Vendor adapter {vf} is large enough to risk duplicating canonical project truth."), evidence:vec![vf.into()], remediation:"Keep vendor adapters thin and route them back to AGENTS.md/.agentic/.", confidence:"heuristic" });
            }
        }
    }

    let dimensions = [
        ("context_architecture",14),("agents_md_quality",10),("skill_architecture",10),("documentation_routing",9),
        ("completion_semantics",9),("decision_boundaries",8),("verification_tooling",10),("architecture_discoverability",7),
        ("decision_history",5),("model_portability",6),("instruction_health",7),("agent_security",5)
    ];
    let mut scores = BTreeMap::<&str,i64>::new();
    for (d,_) in dimensions { scores.insert(d,100); }
    for f in &findings {
        if let Some(s) = scores.get_mut(f.dimension) { *s = clamp(*s - severity_penalty(f.severity)); }
    }
    if !root.join("ARCHITECTURE.md").exists() && !root.join(".agentic/ARCHITECTURE.md").exists() {
        *scores.get_mut("architecture_discoverability").unwrap() -= 15;
    }
    if !root.join(".agentic/decisions").exists() && !root.join("docs/decisions").exists() { *scores.get_mut("decision_history").unwrap() -= 7; }
    if !files.iter().any(|p| rel(root,p).contains("test")) { *scores.get_mut("verification_tooling").unwrap() -= 7; }
    for s in scores.values_mut() { *s = clamp(*s); }
    let total_weight:i64 = dimensions.iter().map(|(_,w)| *w).sum();
    let overall:i64 = dimensions.iter().map(|(d,w)| scores[d] * w).sum::<i64>() / total_weight;
    let critical = findings.iter().any(|f| f.severity == "critical");
    let overall = if critical { overall.min(59) } else { overall };

    json!({
        "schema":"agentic-readiness/v1",
        "target":root,
        "universal_score":overall,
        "scores":scores,
        "metrics":{"agents_words":agent_words,"agents_lines":agent_lines,"skills":skills.len(),"skill_overlap_pairs":overlap_pairs.len(),"has_context_router":has_router},
        "findings":findings.iter().map(Finding::value).collect::<Vec<_>>(),
        "checks":{"performed":["AGENTS.md size/routing heuristics","context router presence","skill discovery/overlap","thin vendor adapter heuristic","completion/autonomy hints","architecture/decision/test presence"],"not_checked":["semantic contradiction by LLM","runtime tool behavior","model-specific compatibility without registry profile","build/test execution"]}
    })
}

fn registry_root() -> PathBuf {
    env::var_os("AH_REGISTRY").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("upstream/agentic-harness/registry/models"))
}
fn load_profiles() -> Vec<Value> {
    let root = registry_root();
    if !root.exists() { return Vec::new(); }
    all_files(&root).into_iter().filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|p| serde_json::from_str::<Value>(&text(&p)).ok()).collect()
}
fn model_rows(root: &Path, task: Option<&str>) -> Value {
    let base = audit(root);
    let profiles = load_profiles();
    let rows = profiles.into_iter().map(|p| json!({
        "id":p["id"], "vendor":p["vendor"], "model":p["model"],
        "project_task_suitability":Value::Null,
        "current_structure_compatibility":base["universal_score"],
        "improved_compatibility":Value::Null,
        "task":task,
        "note":"Raw model suitability requires benchmark/evidence fields; no unsupported score is invented."
    })).collect::<Vec<_>>();
    json!({"target":root,"task":task,"models":rows,"registry":registry_root(),"universal_score":base["universal_score"]})
}
fn improve(root: &Path) -> Value {
    let a = audit(root);
    let actions = a["findings"].as_array().unwrap_or(&Vec::new()).iter().map(|f| json!({
        "action":"change", "rule":f["rule"], "file":f["evidence"].get(0), "proposal":f["remediation"], "severity":f["severity"]
    })).collect::<Vec<_>>();
    json!({"target":root,"mode":"preview","current_score":a["universal_score"],"actions":actions,"note":"No files were modified. Apply only deterministic transformations after review."})
}

fn usage() {
    eprintln!("ah-agentic <audit|context|skills|models|compare|improve|migrate> [TARGET] [options]\n\n  audit [TARGET]\n  context [TARGET]\n  skills [TARGET]\n  models [TARGET] [--task NAME]\n  compare MODEL_A MODEL_B [TARGET]\n  improve [TARGET]\n  migrate --from MODEL_A --to MODEL_B [TARGET]\n\nSet AH_REGISTRY to override the model registry path.");
}
fn main() {
    let args:Vec<String> = env::args().skip(1).collect();
    if args.is_empty() { usage(); std::process::exit(2); }
    let cmd = args[0].as_str();
    let target = args.iter().skip(1).find(|x| !x.starts_with('-') && *x != "models" && *x != "audit" && *x != "context" && *x != "skills" && *x != "improve").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let out = match cmd {
        "audit" => audit(&target),
        "context" => { let a=audit(&target); json!({"target":target,"score":a["scores"]["context_architecture"],"metrics":a["metrics"],"findings":a["findings"].as_array().unwrap().iter().filter(|f| f["dimension"]=="context_architecture" || f["dimension"]=="documentation_routing").cloned().collect::<Vec<_>>()}) },
        "skills" => { let a=audit(&target); json!({"target":target,"score":a["scores"]["skill_architecture"],"metrics":a["metrics"],"findings":a["findings"].as_array().unwrap().iter().filter(|f| f["dimension"]=="skill_architecture").cloned().collect::<Vec<_>>()}) },
        "models" => { let task=args.iter().position(|x| x=="--task").and_then(|i| args.get(i+1)).map(String::as_str); model_rows(&target,task) },
        "compare" => { if args.len()<3 { usage(); std::process::exit(2); } let profiles=load_profiles(); let ids=[args[1].clone(),args[2].clone()]; let selected=profiles.into_iter().filter(|p| ids.iter().any(|id| p["id"].as_str()==Some(id))).collect::<Vec<_>>(); json!({"models":selected,"note":"Profile evidence is compared without inventing capability scores."}) },
        "improve" => improve(&target),
        "migrate" => { let from=args.iter().position(|x|x=="--from").and_then(|i|args.get(i+1)); let to=args.iter().position(|x|x=="--to").and_then(|i|args.get(i+1)); json!({"target":target,"from":from,"to":to,"plan":improve(&target),"note":"Model-specific migration actions require both evidence-backed profiles in the registry."}) },
        _ => { usage(); std::process::exit(2); }
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlap_detects_shared_trigger_language() { assert!(overlap("postgres migration schema change", "review postgres migration schema") > 0.6); }
    #[test]
    fn penalties_are_stable() { assert_eq!(severity_penalty("high"),15); assert_eq!(severity_penalty("medium"),7); }
}
