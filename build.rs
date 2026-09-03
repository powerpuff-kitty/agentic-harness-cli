use std::fs;
use std::io;
use std::path::Path;

fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    if dst.exists() { fs::remove_dir_all(dst)?; }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() { copy_dir(&path, &target)?; } else { fs::copy(&path, &target)?; }
    }
    Ok(())
}

fn main() {
    let boilerplates = Path::new("upstream/agentic-harness/boilerplates");
    let compatibility_templates = Path::new("upstream/agentic-harness/templates");
    if boilerplates.exists() {
        copy_dir(boilerplates, compatibility_templates)
            .expect("failed to prepare internal boilerplate compatibility tree");
    }

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
    println!("cargo:rerun-if-changed=upstream/agentic-harness/boilerplates");
    println!("cargo:rerun-if-changed=upstream/agentic-harness-agents");
}
