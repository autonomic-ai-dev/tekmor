use anyhow::Result;
use tekmor::{config::Config, host_hooks};
use tracing_subscriber::EnvFilter;

mod cli_help;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("tekmor=info".parse()?))
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if let Some(target) = cli_help::help_target(&args) {
        match target {
            None => cli_help::print_root(),
            Some(cmd) => {
                if !cli_help::print_command(cmd) {
                    eprintln!("Unknown command: {cmd}");
                    cli_help::print_root();
                    std::process::exit(1);
                }
            }
        }
        return Ok(());
    }

    let cmd = args.get(1).map(String::as_str).unwrap_or("help");

    match cmd {
        "install" => {
            if let Some(flag) = args[2..]
                .iter()
                .find(|a| !matches!(a.as_str(), "--global" | "-g" | "--quiet"))
            {
                anyhow::bail!(
                    "unknown install option `{flag}`; supported: --global (-g), --quiet"
                );
            }
            let user = args.iter().any(|a| a == "--global" || a == "-g");
            let quiet = args.iter().any(|a| a == "--quiet");
            host_hooks::install_claude_code_hooks(user, quiet)?;
        }
        "doctor" => {
            let cwd = std::env::current_dir()?;
            let root = tekmor::config::find_repo_root(&cwd).unwrap_or(cwd);
            if !tekmor::doctor::run(&root)? {
                std::process::exit(1);
            }
        }
        "version" => {
            println!("tekmor {}", env!("CARGO_PKG_VERSION"));
        }
        "stop-gate" => {
            tekmor::stop_gate::run_stop_gate()?;
        }
        "commit-gate" => {
            tekmor::commit_gate::run_commit_gate()?;
        }
        "bash-gate" => {
            tekmor::command_policy::run_bash_gate()?;
        }
        "rules" => {
            tekmor::command_rules::run_cli(&args[2..])?;
        }
        "verify" => {
            let sub = args.get(2).map(String::as_str).unwrap_or("");
            if sub == "init" {
                let force = args.iter().any(|a| a == "--force" || a == "-f");
                let target_dir = flag_value(&args, "--dir")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                    });
                let path = tekmor::verify::init_verify(&target_dir, force)?;
                println!("Initialized verification suite in {}", path.display());
                if tekmor::verify::load_command_allowlist(&target_dir).is_empty() {
                    println!("no test command detected; add yours to .verify/verify.toml");
                }
                return Ok(());
            }
            let config = Config::load()?;
            let store = tekmor::db::store::BrainStore::open(&config.db_path)?;
            if sub == "run" {
                let source_raw = flag_value(&args, "--source").unwrap_or_else(|| "cli".to_string());
                let source = tekmor::verify::VerifySource::parse(&source_raw)?;
                let root = match flag_value(&args, "--project") {
                    Some(path) => std::path::PathBuf::from(path),
                    None => {
                        let cwd = std::env::current_dir()?;
                        tekmor::config::find_repo_root(&cwd).unwrap_or_else(|| cwd.clone())
                    }
                };
                match tekmor::verify::run_default_step(&store, &root, source)? {
                    None => eprintln!("no [verify.default] step declared in {}", root.display()),
                    Some(run) => {
                        let report = &run.report;
                        if args.iter().any(|a| a == "--json") {
                            println!("{}", serde_json::to_string_pretty(report)?);
                        } else {
                            let verdict = if report.success { "passed" } else { "failed" };
                            println!(
                                "verify default {verdict} (exit {}): {}",
                                report.exit_code, report.cmd
                            );
                        }
                        if !report.success {
                            eprintln!("{}", run.output.trim_end());
                            std::process::exit(report.exit_code);
                        }
                    }
                }
                return Ok(());
            }
            if sub == "check-checks" {
                let cwd = std::env::current_dir()?;
                let root = tekmor::config::find_repo_root(&cwd).unwrap_or(cwd);
                let verdicts = tekmor::check_checks::run(&root)?;
                let mut accepted = true;
                for v in &verdicts {
                    let mark = if v.caught { "caught" } else { "NOT CAUGHT" };
                    println!("{mark:<10} {}", v.cmd);
                    accepted &= v.caught;
                }
                if !accepted {
                    std::process::exit(1);
                }
                return Ok(());
            }
            if sub == "drift" {
                let cwd = std::env::current_dir()?;
                let root = tekmor::config::find_repo_root(&cwd).unwrap_or(cwd);
                let drift = tekmor::drift::check(&root);
                for d in &drift {
                    println!("{}: {}", d.feature, d.problem);
                }
                if drift.is_empty() {
                    println!("no drift: feature maps match the code and the allowlist");
                } else {
                    std::process::exit(1);
                }
                return Ok(());
            }
            if sub == "review" {
                let cwd = std::env::current_dir()?;
                let root = tekmor::config::find_repo_root(&cwd).unwrap_or(cwd);
                let verdict = flag_value(&args, "--verdict").unwrap_or_default();
                let reviewer = flag_value(&args, "--reviewer").unwrap_or_default();
                let notes = flag_value(&args, "--notes").unwrap_or_default();
                if !matches!(verdict.as_str(), "pass" | "fail") || reviewer.is_empty() {
                    eprintln!("usage: tekmor verify review --verdict pass|fail --reviewer NAME [--notes TEXT]");
                    std::process::exit(2);
                }
                let Some(hash) = tekmor::verify::code_tree_hash(&root) else {
                    eprintln!("not a git repository: cannot bind a review to the tree");
                    std::process::exit(2);
                };
                let config = tekmor::config::Config::load()?;
                let store = tekmor::review::default_store(&config.home);
                let path = tekmor::review::record(&root, &store, &hash, &verdict, &reviewer, &notes)?;
                println!("recorded {verdict} review at {}", path.display());
                return Ok(());
            }
            if sub == "ratchet" && args.get(3).map(String::as_str) == Some("add") {
                let cwd = std::env::current_dir()?;
                let root = tekmor::config::find_repo_root(&cwd).unwrap_or(cwd);
                let option = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
                let paths: Vec<String> = args
                    .windows(2)
                    .filter(|pair| pair[0] == "--path")
                    .map(|pair| pair[1].clone())
                    .collect();
                let (Some(name), Some(pattern)) = (option("--name"), option("--pattern")) else {
                    anyhow::bail!("usage: tekmor verify ratchet add --name NAME --pattern REGEX [--path PREFIX]... [--baseline N]");
                };
                let baseline = option("--baseline").map(|v| v.parse::<usize>()).transpose()?;
                let rule = tekmor::ratchet::add(&root, &name, &pattern, paths, baseline)?;
                println!("added {} (baseline {}) to {}", rule.name, rule.baseline, tekmor::ratchet::RATCHET_FILE);
                return Ok(());
            }
            if sub == "ratchet" {
                let cwd = std::env::current_dir()?;
                let root = tekmor::config::find_repo_root(&cwd).unwrap_or(cwd);
                let ok = tekmor::ratchet::run(&root, args.iter().any(|a| a == "--update"))?;
                if !ok {
                    std::process::exit(1);
                }
                return Ok(());
            }
            if sub == "summary" {
                let summaries = tekmor_core::evidence::summarize(
                    &tekmor::evidence_reader::recent_runs(&store, 500)?,
                );
                if args.iter().any(|a| a == "--json") {
                    println!("{}", serde_json::to_string_pretty(&summaries)?);
                } else {
                    for s in &summaries {
                        println!(
                            "{}/{} failed  last exit {}  {}",
                            s.failures, s.runs, s.last_exit_code, s.cmd
                        );
                    }
                }
                return Ok(());
            }
            if sub == "receipt" {
                let root = match flag_value(&args, "--project") {
                    Some(path) => std::path::PathBuf::from(path),
                    None => {
                        let cwd = std::env::current_dir()?;
                        tekmor::config::find_repo_root(&cwd).unwrap_or(cwd)
                    }
                };
                let receipt = tekmor::receipt::receipt_for_project(&store, &root)?;
                if args.iter().any(|a| a == "--json") {
                    println!("{}", serde_json::to_string_pretty(&receipt)?);
                } else if args.iter().any(|a| a == "--markdown") {
                    print!("{}", tekmor::receipt::render_markdown(&receipt));
                } else {
                    print!("{}", tekmor::receipt::render_text(&receipt));
                }
                if receipt.verdict != tekmor::receipt::Verdict::Passed {
                    std::process::exit(1);
                }
                return Ok(());
            }
            if let Some(n) = flag_value(&args, "--repeat").and_then(|v| v.parse::<u32>().ok()) {
                let exe = std::env::current_exe()?;
                let mut rest = Vec::new();
                let mut i = 2;
                while i < args.len() {
                    if args[i] == "--repeat" {
                        i += 2;
                        continue;
                    }
                    rest.push(args[i].clone());
                    i += 1;
                }
                let mut all_ok = true;
                for _ in 0..n {
                    all_ok &= std::process::Command::new(&exe).arg("verify").args(&rest).status()?.success();
                }
                if !all_ok {
                    std::process::exit(1);
                }
                return Ok(());
            }
            let route_log_id = flag_value(&args, "--route-log-id");
            let cause = flag_value(&args, "--cause");
            let config_path = flag_value(&args, "--config").map(std::path::PathBuf::from);
            let scenarios = flag_value(&args, "--scenarios").map(std::path::PathBuf::from);
            let source_raw = flag_value(&args, "--source").unwrap_or_else(|| "cli".to_string());
            let source = tekmor::verify::VerifySource::parse(&source_raw)?;
            let save_artifact = args.iter().any(|a| a == "--save-artifact");
            let cmd: Vec<String> = if let Some(idx) = args.iter().position(|a| a == "--") {
                args[idx + 1..].to_vec()
            } else {
                eprintln!(
                    "Usage: tekmor verify [--source cli|orchestrator] [--config PATH] [--scenarios DIR] [--route-log-id ID] [--cause TAG] [--save-artifact] -- <cmd> [args...]\n       tekmor verify init [--force] [--dir DIR]"
                );
                std::process::exit(1);
            };
            let report = tekmor::verify::run_verify(
                &store,
                &cmd,
                tekmor::verify::VerifyRun {
                    route_log_id: route_log_id.as_deref(),
                    cause_tag: cause.as_deref(),
                    config_path: config_path.as_deref(),
                    scenarios_dir: scenarios.as_deref(),
                    source,
                    save_artifact,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.success {
                std::process::exit(report.exit_code);
            }
        }
        "help" | "--help" | "-h" => cli_help::print_root(),
        _ => {
            eprintln!("Unknown command: {cmd}");
            cli_help::print_root();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|idx| args.get(idx + 1))
        .cloned()
}

