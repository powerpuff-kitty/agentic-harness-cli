use std::path::Path;

fn main() {
    let required = [
        "upstream/agentic-harness/templates/base/AGENTS.md",
        "upstream/agentic-harness/packs",
        "upstream/agentic-harness/policies",
        "upstream/agentic-harness/profiles",
        "upstream/agentic-harness/presets",
        "upstream/agentic-harness-agents/skills/agentic-app/SKILL.md",
    ];
    let missing: Vec<_> = required.iter().filter(|p| !Path::new(p).exists()).collect();
    if !missing.is_empty() {
        panic!("Agentic Harness upstream sources are missing. Run ./scripts/sync-upstream.sh before building. Missing: {:?}", missing);
    }
    println!("cargo:rerun-if-changed=upstream.lock.json");
    println!("cargo:rerun-if-changed=upstream/agentic-harness");
    println!("cargo:rerun-if-changed=upstream/agentic-harness-agents");
}
