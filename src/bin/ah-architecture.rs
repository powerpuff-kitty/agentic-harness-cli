#[path = "../architecture.rs"]
mod architecture;

use std::env;
use std::path::{Path, PathBuf};

fn usage(prog: &str) {
    println!("Agentic Harness Architecture\n\nusage: {prog} detect [TARGET]\n\ncommands:\n  detect [TARGET]   Detect framework, ecosystem tooling, language and current architecture shape");
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
        _ => {
            usage(prog);
            2
        }
    };

    std::process::exit(code);
}
