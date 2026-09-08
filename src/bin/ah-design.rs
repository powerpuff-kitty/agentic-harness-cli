#[path = "../design_analysis.rs"]
mod design_analysis;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn die(message: impl AsRef<str>) -> ! {
    eprintln!("{}", message.as_ref());
    std::process::exit(2)
}

fn usage(program: &str) {
    println!(
        "Agentic Harness Design (experimental)\n\nusage: {program} analyze [TARGET] [--level static] [--output FILE]\n\ncommands:\n  analyze    deterministically inspect static design values and emit design-analysis format v1"
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = Path::new(&args[0])
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ah-design");

    if args.len() < 2 || matches!(args[1].as_str(), "-h" | "--help") {
        usage(program);
        return;
    }

    match args[1].as_str() {
        "analyze" => {
            let mut target = PathBuf::from(".");
            let mut output: Option<PathBuf> = None;
            let mut level = "static".to_string();
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--level" => {
                        i += 1;
                        level = args.get(i).cloned().unwrap_or_else(|| die("--level requires a value"));
                    }
                    "--output" => {
                        i += 1;
                        output = Some(PathBuf::from(args.get(i).cloned().unwrap_or_else(|| die("--output requires a value"))));
                    }
                    option if option.starts_with('-') => die(format!("unknown option: {option}")),
                    path => target = PathBuf::from(path),
                }
                i += 1;
            }

            if level != "static" {
                die("only --level static is implemented in this first slice");
            }
            if !target.exists() {
                die("target does not exist");
            }

            let report = design_analysis::analyze_static(&target);
            let serialized = serde_json::to_string_pretty(&report).expect("design report serialization must succeed") + "\n";
            if let Some(path) = output {
                if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                    fs::create_dir_all(parent).unwrap_or_else(|error| die(error.to_string()));
                }
                fs::write(&path, serialized.as_bytes()).unwrap_or_else(|error| die(error.to_string()));
            }
            print!("{serialized}");
        }
        _ => {
            usage(program);
            std::process::exit(2);
        }
    }
}
