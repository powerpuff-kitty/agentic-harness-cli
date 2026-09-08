#[path = "../design_analysis.rs"]
mod design_analysis;
#[path = "../design_diff.rs"]
mod design_diff;
#[path = "../design_genome.rs"]
mod design_genome;
#[path = "../design_prompt.rs"]
mod design_prompt;

use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn die(message: impl AsRef<str>) -> ! {
    eprintln!("{}", message.as_ref());
    std::process::exit(2)
}

fn usage(program: &str) {
    println!(
        "Agentic Harness Design (experimental)\n\nusage:\n  {program} analyze [TARGET] [--level static] [--output FILE]\n  {program} preserve --analysis DESIGN-ANALYSIS.json [--output FILE]\n  {program} diff BEFORE.json AFTER.json [--output FILE]\n  {program} prompt --genome DESIGN-GENOME.json --task DESIGN-TASK.json [--output FILE]\n\ncommands:\n  analyze     deterministically inspect static design values and emit Design Analysis format v1\n  preserve    derive a review-required candidate Design Genome from measured analysis evidence\n  diff        compare two Design Analysis v1 artifacts and report measurable drift\n  prompt      deterministically compile an approved Design Genome + structured task into a model-neutral implementation brief"
    );
}

fn write_json_output(value: &Value, output: Option<PathBuf>) {
    let serialized = serde_json::to_string_pretty(value).expect("design output serialization must succeed") + "\n";
    write_text_output(&serialized, output);
}

fn write_text_output(text: &str, output: Option<PathBuf>) {
    if let Some(path) = output {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).unwrap_or_else(|error| die(error.to_string()));
        }
        fs::write(&path, text.as_bytes()).unwrap_or_else(|error| die(error.to_string()));
    }
    print!("{text}");
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|error| die(format!("could not read {}: {error}", path.display())));
    serde_json::from_str(&text).unwrap_or_else(|error| die(format!("{} is not valid JSON: {error}", path.display())))
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

            write_json_output(&design_analysis::analyze_static(&target), output);
        }
        "preserve" => {
            let mut analysis: Option<PathBuf> = None;
            let mut output: Option<PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--analysis" => {
                        i += 1;
                        analysis = Some(PathBuf::from(args.get(i).cloned().unwrap_or_else(|| die("--analysis requires a value"))));
                    }
                    "--output" => {
                        i += 1;
                        output = Some(PathBuf::from(args.get(i).cloned().unwrap_or_else(|| die("--output requires a value"))));
                    }
                    option => die(format!("unknown preserve option: {option}")),
                }
                i += 1;
            }

            let analysis_path = analysis.unwrap_or_else(|| die("preserve requires --analysis DESIGN-ANALYSIS.json"));
            let analysis_value = read_json(&analysis_path);
            let candidate = match design_genome::candidate_from_analysis(
                &analysis_value,
                &analysis_path.to_string_lossy(),
            ) {
                Ok(candidate) => candidate,
                Err(error) => die(error),
            };
            write_json_output(&candidate, output);
        }
        "diff" => {
            let mut inputs = Vec::new();
            let mut output: Option<PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--output" => {
                        i += 1;
                        output = Some(PathBuf::from(args.get(i).cloned().unwrap_or_else(|| die("--output requires a value"))));
                    }
                    option if option.starts_with('-') => die(format!("unknown option: {option}")),
                    path => inputs.push(PathBuf::from(path)),
                }
                i += 1;
            }
            if inputs.len() != 2 {
                die("diff requires BEFORE.json and AFTER.json");
            }
            let before = read_json(&inputs[0]);
            let after = read_json(&inputs[1]);
            let report = match design_diff::diff_analysis(&before, &after) {
                Ok(report) => report,
                Err(error) => die(error),
            };
            write_json_output(&report, output);
        }
        "prompt" => {
            let mut genome: Option<PathBuf> = None;
            let mut task: Option<PathBuf> = None;
            let mut output: Option<PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--genome" => {
                        i += 1;
                        genome = Some(PathBuf::from(args.get(i).cloned().unwrap_or_else(|| die("--genome requires a value"))));
                    }
                    "--task" => {
                        i += 1;
                        task = Some(PathBuf::from(args.get(i).cloned().unwrap_or_else(|| die("--task requires a value"))));
                    }
                    "--output" => {
                        i += 1;
                        output = Some(PathBuf::from(args.get(i).cloned().unwrap_or_else(|| die("--output requires a value"))));
                    }
                    option => die(format!("unknown prompt option: {option}")),
                }
                i += 1;
            }

            let genome_path = genome.unwrap_or_else(|| die("prompt requires --genome DESIGN-GENOME.json"));
            let task_path = task.unwrap_or_else(|| die("prompt requires --task DESIGN-TASK.json"));
            let genome = read_json(&genome_path);
            let task = read_json(&task_path);
            let prompt = match design_prompt::compile_prompt(&genome, &task) {
                Ok(prompt) => prompt,
                Err(error) => die(error),
            };
            write_text_output(&prompt, output);
        }
        _ => {
            usage(program);
            std::process::exit(2);
        }
    }
}
