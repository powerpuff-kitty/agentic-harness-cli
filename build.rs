use std::path::Path;

fn main() {
    let required = [
        "upstream/agentic-harness/base/boilerplate.json",
        "upstream/agentic-harness/web-app/boilerplate.json",
        "upstream/agentic-harness/backend-api/boilerplate.json",
        "upstream/agentic-harness/saas/boilerplate.json",
        "upstream/agentic-harness/monorepo/boilerplate.json",
        "upstream/agentic-harness/library-sdk/boilerplate.json",
        "upstream/agentic-harness/modules/packs",
        "upstream/agentic-harness/modules/policies",
        "upstream/agentic-harness/modules/profiles",
        "upstream/agentic-harness/presets",
        "upstream/agentic-harness-agents/skills/agentic-app/SKILL.md",
    ];
    let missing: Vec<_> = required.iter().filter(|p| !Path::new(p).exists()).collect();
    if !missing.is_empty() {
        panic!(
            "Agentic Harness upstream sources are missing. Run ./scripts/sync-upstream.sh before building. Missing: {:?}",
            missing
        );
    }

    println!("cargo:rerun-if-changed=upstream.lock.json");
    println!("cargo:rerun-if-changed=upstream/agentic-harness");
    println!("cargo:rerun-if-changed=upstream/agentic-harness-agents");
}
