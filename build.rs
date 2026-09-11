use std::path::Path;

fn main() {
    let required = [
        "upstream/agentic-harness-registry/registry/models",
        "upstream/agentic-harness/catalog/variants/base/variant.json",
        "upstream/agentic-harness/catalog/variants/web-app/variant.json",
        "upstream/agentic-harness/catalog/variants/backend-api/variant.json",
        "upstream/agentic-harness/catalog/variants/saas/variant.json",
        "upstream/agentic-harness/catalog/variants/monorepo/variant.json",
        "upstream/agentic-harness/catalog/variants/library-sdk/variant.json",
        "upstream/agentic-harness/catalog/packs",
        "upstream/agentic-harness/catalog/policies",
        "upstream/agentic-harness/catalog/profiles",
        "upstream/agentic-harness/catalog/presets",
        "upstream/agentic-harness-agents/skills/agentic-app/SKILL.md",
    ];
    let missing: Vec<_> = required.iter().filter(|p| !Path::new(p).exists()).collect();
    if !missing.is_empty() {
        panic!(
            "Agentic Harness upstream sources are missing. Run ./scripts/sync-upstream.sh before building. Missing: {:?}",
            missing
        );
    }

    // include_dir snapshots must be rebuilt when source inputs change.
    println!("cargo:rerun-if-changed=upstream.lock.json");
    println!("cargo:rerun-if-changed=upstream/agentic-harness-registry");
    println!("cargo:rerun-if-changed=upstream/agentic-harness");
    println!("cargo:rerun-if-changed=upstream/agentic-harness-agents");
}
