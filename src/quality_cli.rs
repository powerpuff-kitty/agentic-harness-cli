use crate::quality;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn usage(program: &str) {
    println!(
        "Agentic Harness Quality (experimental)\n\nusage:\n  {program} detect [TARGET]\n  {program} analyze [TARGET]\n  {program} baseline [TARGET] [--output PATH]\n  {program} diff BASELINE [TARGET] [--output PATH]\n\ncommands:\n  detect [TARGET]    Detect languages, quality tooling, configuration and quality-related project scripts without executing them\n  analyze [TARGET]   Perform deterministic read-only quality/configuration analysis and report explicit coverage gaps\n  baseline [TARGET]  Snapshot current static findings and quality contract/tool-config identity\n  diff BASELINE      Compare current static findings with a repository-local baseline and report stale identity\n\nNo project quality command, formatter, linter, typechecker or autofix is executed by these commands."
    );
}

fn split_args(args: &[String], start: usize) -> (Vec<String>, Option<String>) {
    let mut positional = Vec::new();
    let mut output = None;
    let mut index = start;
    while index < args.len() {
        if args[index] == "--output" {
            index += 1;
            output = args.get(index).cloned();
        } else {
            positional.push(args[index].clone());
        }
        index += 1;
    }
    (positional, output)
}

fn require_root(path: Option<&String>) -> PathBuf {
    let root = PathBuf::from(path.map(String::as_str).unwrap_or("."));
    if !root.is_dir() {
        crate::fail(format!("target is not a directory: {}", root.display()));
    }
    root
}

fn emit(root: &Path, output: Option<&str>, value: Value) {
    let text = serde_json::to_string_pretty(&value)
        .expect("quality output serialization must succeed")
        + "\n";
    if let Some(output) = output {
        crate::scan::write(root, output, text.as_bytes())
            .unwrap_or_else(|error| crate::fail(format!("could not write {output}: {error}")));
    }
    print!("{text}");
}

fn read_baseline(root: &Path, relative: &str) -> Value {
    let path = root.join(relative);
    let text = crate::scan::read(root, &path, 5_000_000).unwrap_or_else(|error| {
        crate::fail(format!("could not read baseline {relative}: {error}"))
    });
    serde_json::from_str(&text)
        .unwrap_or_else(|error| crate::fail(format!("invalid baseline JSON {relative}: {error}")))
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

    match args[1].as_str() {
        "detect" => {
            let root = require_root(args.get(2));
            emit(&root, None, quality::detect(&root));
        }
        "analyze" => {
            let root = require_root(args.get(2));
            emit(&root, None, quality::analyze(&root));
        }
        "baseline" => {
            let (positional, output) = split_args(&args, 2);
            let root = require_root(positional.first());
            let value = quality::baseline(&root).unwrap_or_else(|error| crate::fail(error));
            emit(&root, output.as_deref(), value);
        }
        "diff" => {
            let (positional, output) = split_args(&args, 2);
            let baseline_path = positional
                .first()
                .unwrap_or_else(|| crate::fail("quality diff requires BASELINE"));
            let root = require_root(positional.get(1));
            let previous = read_baseline(&root, baseline_path);
            let value = quality::diff(&root, baseline_path, &previous)
                .unwrap_or_else(|error| crate::fail(error));
            emit(&root, output.as_deref(), value);
        }
        _ => {
            usage(program);
            crate::finish(2);
        }
    }
}
