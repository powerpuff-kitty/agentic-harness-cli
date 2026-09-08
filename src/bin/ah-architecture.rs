#[path = "../architecture.rs"]
mod architecture;
#[path = "../architecture_analysis.rs"]
mod architecture_analysis;

use std::env;
use std::path::{Path, PathBuf};

fn usage(prog: &str) {
    println!(
        "Agentic Harness Architecture\n\nusage:\n  {prog} detect [TARGET]\n  {prog} analyze [TARGET] [--profile PROFILE]\n\ncommands:\n  detect [TARGET]   Detect framework, ecosystem tooling, language and current architecture shape\n  analyze [TARGET]  Build the local import graph and report deterministic architecture violations"
    );
}

fn analyze_args(args: &[String]) -> Result<(PathBuf, Vec<String>), String> {
    let mut root = PathBuf::from(".");
    let mut profiles = Vec::new();
    let mut root_set = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--profile" => {
                index += 1;
                let Some(profile) = args.get(index) else {
                    return Err("--profile requires a profile ID".to_string());
                };
                if !architecture_analysis::known_profile(profile) {
                    return Err(format!("unsupported architecture profile: {profile}"));
                }
                profiles.push(profile.clone());
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option: {value}"));
            }
            value if !root_set => {
                root = PathBuf::from(value);
                root_set = true;
            }
            value => return Err(format!("unexpected argument: {value}")),
        }
        index += 1;
    }
    Ok((root, profiles))
}

fn main() {
    let argv: Vec<String> = env::args().collect();
    let prog = Path::new(&argv[0])
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("ah-architecture");

    if argv.len() < 2 || ["-h", "--help"].contains(&argv[1].as_str()) {
        usage(prog);
        return;
    }

    let code = match argv[1].as_str() {
        "detect" => {
            let root = PathBuf::from(argv.get(2).map(String::as_str).unwrap_or("."));
            if !root.exists() {
                eprintln!("target does not exist: {}", root.display());
                2
            } else {
                let result = architecture::detect(&root);
                println!("{}", serde_json::to_string_pretty(&result).unwrap());
                0
            }
        }
        "analyze" => match analyze_args(&argv[2..]) {
            Err(error) => {
                eprintln!("{error}");
                2
            }
            Ok((root, _)) if !root.exists() => {
                eprintln!("target does not exist: {}", root.display());
                2
            }
            Ok((root, profiles)) => {
                let result = architecture_analysis::analyze(&root, &profiles);
                println!("{}", serde_json::to_string_pretty(&result).unwrap());
                0
            }
        },
        _ => {
            usage(prog);
            2
        }
    };

    std::process::exit(code);
}
