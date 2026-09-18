use crate::quality;
use std::path::{Path, PathBuf};

fn usage(program: &str) {
    println!(
        "Agentic Harness Quality (experimental)\n\nusage:\n  {program} detect [TARGET]\n  {program} analyze [TARGET]\n\ncommands:\n  detect [TARGET]   Detect languages, quality tooling, configuration and quality-related project scripts without executing them\n  analyze [TARGET]  Perform deterministic read-only quality/configuration analysis and report explicit coverage gaps\n\nExecution, baselines, diffs and autofix are tracked separately and are not performed by this first slice."
    );
}

fn target(args: &[String]) -> PathBuf {
    PathBuf::from(args.get(2).map(String::as_str).unwrap_or("."))
}

fn print_report(value: serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).expect("quality output serialization must succeed")
    );
}

pub fn run(args: Vec<String>) {
    let program = Path::new(&args[0])
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ah quality");

    if args.len() < 2 || matches!(args[1].as_str(), "-h" | "--help") {
        usage(program);
        return;
    }

    let root = target(&args);
    if !root.is_dir() {
        crate::fail(format!("target is not a directory: {}", root.display()));
    }

    match args[1].as_str() {
        "detect" => print_report(quality::detect(&root)),
        "analyze" => print_report(quality::analyze(&root)),
        _ => {
            usage(program);
            crate::finish(2);
        }
    }
}
