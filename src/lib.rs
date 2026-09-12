mod agentic;
mod architecture;
mod architecture_analysis;
mod architecture_cli;
mod architecture_contract;
mod architecture_score;
mod artifact;
mod check_execution;
mod check_inputs;
mod checks;
mod cli;
mod date;
mod design_analysis;
mod design_cli;
mod design_diff;
mod design_genome;
mod design_prompt;
mod design_system;
mod execution_review;
mod process_check;
mod project;
mod scan;
mod strict_json;
mod syntax;
mod syntax_worker;

pub fn fail(message: impl AsRef<str>) -> ! {
    eprintln!(
        "{}",
        serde_json::json!({"format_version":1,"kind":"diagnostic","code":"invalid-input","message":message.as_ref()})
    );
    finish(2)
}

pub(crate) fn finish(code: i32) -> ! {
    crate::syntax_worker::shutdown();
    std::process::exit(code)
}

pub fn version() -> serde_json::Value {
    serde_json::json!({"format_version":1,"kind":"version","version":env!("CARGO_PKG_VERSION"),"sources":serde_json::from_str::<serde_json::Value>(include_str!("../upstream.lock.json")).unwrap()})
}

pub fn entry(family: Option<&str>) {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && args[1] == "--internal-syntax-worker" {
        if crate::syntax_worker::run().is_err() {
            std::process::exit(2);
        }
        return;
    }
    if args.get(1).is_some_and(|x| x == "--version" || x == "-V") {
        if args.len() != 2 {
            fail("--version takes no other arguments");
        }
        println!("{}", version());
        return;
    }
    let family = family.map(str::to_owned).or_else(|| {
        if args
            .get(1)
            .is_some_and(|x| ["agentic", "architecture", "design", "checks"].contains(&x.as_str()))
        {
            Some(args.remove(1))
        } else {
            None
        }
    });
    validate_args(family.as_deref(), &args).unwrap_or_else(|e| fail(e));
    if let Some(i) = args.iter().position(|s| s == "--as-of") {
        crate::date::set(&args[i + 1]).unwrap_or_else(|e| fail(e));
        args.drain(i..=i + 1);
    }
    if family.is_none()
        && args
            .get(1)
            .is_none_or(|s| ["--help", "-h"].contains(&s.as_str()))
    {
        println!("Experimental family: checks <plan|prepare|run> [TARGET]; execution requires explicit review and unsandboxed acknowledgment.\n");
    }
    crate::scan::begin();
    match family.as_deref() {
        Some("agentic") => agentic::run(args.into_iter().skip(1).collect()),
        Some("architecture") => architecture_cli::run(args),
        Some("design") => design_cli::run(args),
        Some("checks") => checks::run(args),
        _ => cli::run(args),
    }
    crate::syntax_worker::shutdown();
}

fn validate_args(family: Option<&str>, args: &[String]) -> Result<(), String> {
    let Some(command) = args.get(1) else {
        return Ok(());
    };
    if ["--help", "-h"].contains(&command.as_str()) {
        return if args.len() == 2 {
            Ok(())
        } else {
            Err("help takes no arguments".into())
        };
    }
    let (flags, switches, min, max, directory): (&[&str], &[&str], usize, usize, bool) =
        match (family, command.as_str()) {
            (None, "init" | "upgrade") => (
                &[
                    "--boilerplate",
                    "--template",
                    "--preset",
                    "--profile",
                    "--pack",
                    "--skill",
                    "--policy",
                    "--name",
                    "--maturity",
                ],
                &["--allow-existing"],
                1,
                1,
                false,
            ),
            (None, "audit" | "validate" | "security-scan" | "harness-audit") => {
                (&[], &[], 0, 1, true)
            }
            (None, "catalog-check") => (&[], &[], 0, 0, false),
            (None, "design-system-components") => (&[], &["--write"], 0, 1, true),
            (None, "compare") => (&[], &[], 2, 2, false),
            (None, "gate") => (
                &["--min-overall", "--min-score", "--max-architecture-errors"],
                &["--fail-on-architecture-error"],
                1,
                1,
                false,
            ),
            (Some("checks"), "plan") => (&["--config"], &[], 0, 1, true),
            (Some("checks"), "prepare") => (&["--config", "--settings"], &[], 0, 1, true),
            (Some("checks"), "run") => (
                &["--config", "--settings", "--approve-review"],
                &["--allow-unsandboxed"],
                0,
                1,
                true,
            ),
            (Some("architecture"), "detect") => (&[], &[], 0, 1, true),
            (Some("architecture"), "analyze") => (&["--profile", "--as-of"], &[], 0, 1, true),
            (Some("architecture"), "enforce") => {
                (&["--profile", "--as-of"], &["--write"], 0, 1, true)
            }
            (Some("design"), "analyze") => (&["--level", "--output"], &[], 0, 1, true),
            (Some("design"), "preserve") => (&["--analysis", "--output"], &[], 0, 0, false),
            (Some("design"), "prompt") => (&["--genome", "--task", "--output"], &[], 0, 0, false),
            (Some("design"), "diff") => (&["--output"], &[], 2, 2, false),
            (Some("agentic"), "audit" | "context" | "skills" | "improve") => (&[], &[], 0, 1, true),
            (Some("agentic"), "models") => (&["--task"], &[], 0, 1, true),
            (Some("agentic"), "compare") => (&[], &[], 2, 2, false),
            (Some("agentic"), "migrate") => (&["--from", "--to"], &[], 0, 1, true),
            _ => return Err(format!("unknown command: {command}")),
        };
    let mut positional = Vec::new();
    let mut i = 2;
    while i < args.len() {
        let arg = &args[i];
        if flags.contains(&arg.as_str()) {
            i += 1;
            if args
                .get(i)
                .is_none_or(|v| v.starts_with('-') || v.is_empty())
            {
                return Err(format!("{arg} requires a value"));
            }
        } else if switches.contains(&arg.as_str()) {
        } else if arg.starts_with('-') {
            return Err(format!("unknown or unsupported option: {arg}"));
        } else {
            positional.push(arg);
        }
        i += 1;
    }
    if positional.len() < min || positional.len() > max {
        return Err(format!(
            "{command} expects {min}..={max} positional arguments"
        ));
    }
    if directory {
        scan::require_directory(std::path::Path::new(
            positional.first().map(|s| s.as_str()).unwrap_or("."),
        ))?;
    }
    Ok(())
}
