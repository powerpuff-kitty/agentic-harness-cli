use std::{env, fs, path::Path};

fn copy_skill(source: &Path, destination: &Path) {
    let metadata = fs::symlink_metadata(source).expect("read skill payload metadata");
    assert!(!metadata.file_type().is_symlink(), "linked skill payload");
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        assert_eq!(
            metadata.file_attributes() & 0x400,
            0,
            "reparse skill payload"
        );
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination).expect("create generated skill directory");
        let mut entries: Vec<_> = fs::read_dir(source)
            .expect("read skill directory")
            .map(|entry| entry.expect("read skill entry"))
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_skill(&entry.path(), &destination.join(entry.file_name()));
        }
    } else {
        assert!(metadata.is_file(), "non-regular skill payload");
        fs::copy(source, destination).expect("stage skill payload");
    }
}

fn stage_skills() {
    // Root skill folders share a checkout with Git metadata and install guidance.
    // Embed only self-contained skills, never the checkout's object store/config.
    let destination = Path::new(&env::var_os("OUT_DIR").expect("Cargo OUT_DIR")).join("skills");
    if destination.exists() {
        fs::remove_dir_all(&destination).expect("replace generated skill staging");
    }
    fs::create_dir_all(&destination).expect("create skill staging");
    let mut count = 0;
    for entry in fs::read_dir("upstream/skills").expect("read pinned skills") {
        let entry = entry.expect("read root skill entry");
        let name = entry.file_name();
        if name == ".git" || name == "INSTALL.md" || name == "README.md" {
            continue;
        }
        let source = entry.path();
        assert!(
            source.join("SKILL.md").is_file() && source.join("LICENSE").is_file(),
            "unexpected root entry in pinned skills"
        );
        copy_skill(&source, &destination.join(name));
        count += 1;
    }
    assert!(count > 0, "no pinned skills found");
}

fn main() {
    let required = [
        "upstream/agentic-harness/catalog/context/profiles.v1.json",
        "upstream/agentic-harness/catalog/context/minimal-map.md",
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
        "upstream/agentic-harness/catalog/schema/decision-spec.v1.schema.json",
        "upstream/agentic-harness/catalog/schema/decision-graph.v1.schema.json",
        "upstream/agentic-harness/catalog/schema/decision-request.v1.schema.json",
        "upstream/agentic-harness/catalog/schema/decision-provider-profile.v1.schema.json",
        "upstream/agentic-harness/catalog/schema/decision-policy.v1.schema.json",
        "upstream/agentic-harness/catalog/schema/decision-receipt.v1.schema.json",
        "upstream/agentic-harness/catalog/schema/decision-outcome.v1.schema.json",
        "upstream/agentic-harness/catalog/schema/decision-evaluation.v1.schema.json",
        "upstream/skills/agentic-app/SKILL.md",
    ];
    let missing: Vec<_> = required.iter().filter(|p| !Path::new(p).exists()).collect();
    if !missing.is_empty() {
        panic!(
            "Agentic Harness upstream sources are missing. Run ./scripts/sync-upstream.sh before building. Missing: {:?}",
            missing
        );
    }

    stage_skills();

    // include_dir snapshots must be rebuilt when source inputs change.
    println!("cargo:rerun-if-changed=upstream.lock.json");
    println!("cargo:rerun-if-changed=upstream/agentic-harness-registry");
    println!("cargo:rerun-if-changed=upstream/agentic-harness");
    println!("cargo:rerun-if-changed=upstream/skills");
}
