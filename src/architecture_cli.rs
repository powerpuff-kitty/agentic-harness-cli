use crate::{architecture, architecture_analysis, architecture_contract};
use std::path::{Path, PathBuf};

fn usage(prog: &str) {
    println!(
        "Agentic Harness Architecture\n\nusage:\n  {prog} detect [TARGET]\n  {prog} analyze [TARGET] [--profile PROFILE] [--as-of YYYY-MM-DD]\n  {prog} enforce [TARGET] [--profile PROFILE] [--as-of YYYY-MM-DD] [--write]\n\ncommands:\n  detect [TARGET]   Detect framework, ecosystem tooling, language and current architecture shape\n  analyze [TARGET]  Build the local import graph and report deterministic architecture violations\n  enforce [TARGET]  Preview or write the normalized project architecture contract"
    );
}

fn profile_args(
    args: &[String],
    allow_write: bool,
) -> Result<(PathBuf, Vec<String>, bool), String> {
    let mut root = PathBuf::from(".");
    let mut profiles = Vec::new();
    let mut root_set = false;
    let mut write = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--profile" => {
                index += 1;
                let Some(profile) = args.get(index) else {
                    return Err("--profile requires a profile ID".to_string());
                };
                if !architecture_contract::known_profile(profile) {
                    return Err(format!("unsupported architecture profile: {profile}"));
                }
                profiles.push(profile.clone());
            }
            "--write" if allow_write => write = true,
            "--write" => return Err("--write is only valid with architecture enforce".to_string()),
            value if value.starts_with('-') => return Err(format!("unknown option: {value}")),
            value if !root_set => {
                root = PathBuf::from(value);
                root_set = true;
            }
            value => return Err(format!("unexpected argument: {value}")),
        }
        index += 1;
    }
    Ok((root, profiles, write))
}

fn print_result(result: Result<serde_json::Value, String>) -> i32 {
    match result {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            0
        }
        Err(error) => crate::fail(error),
    }
}

pub fn run(argv: Vec<String>) {
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
        "analyze" => match profile_args(&argv[2..], false) {
            Err(error) => crate::fail(error),
            Ok((root, _, _)) if !root.exists() => {
                eprintln!("target does not exist: {}", root.display());
                2
            }
            Ok((root, explicit_profiles, _)) => {
                match architecture_contract::profiles_for_analysis(&root, &explicit_profiles) {
                    Err(error) => crate::fail(error),
                    Ok(profiles) => {
                        let mut result = architecture_analysis::analyze(&root, &profiles);
                        match architecture_contract::apply_exceptions(&root, &mut result) {
                            Ok(()) => {
                                println!("{}", serde_json::to_string_pretty(&result).unwrap());
                                0
                            }
                            Err(error) => crate::fail(error),
                        }
                    }
                }
            }
        },
        "enforce" => match profile_args(&argv[2..], true) {
            Err(error) => crate::fail(error),
            Ok((root, _, _)) if !root.exists() => {
                eprintln!("target does not exist: {}", root.display());
                2
            }
            Ok((root, profiles, write)) => {
                if write {
                    print_result(architecture_contract::write(&root, &profiles))
                } else {
                    print_result(architecture_contract::preview(&root, &profiles))
                }
            }
        },
        _ => {
            usage(prog);
            2
        }
    };

    std::process::exit(code);
}
