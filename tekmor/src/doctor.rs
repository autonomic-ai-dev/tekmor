use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

pub struct Finding {
    pub ok: bool,
    pub label: String,
    pub detail: String,
}

fn finding(ok: bool, label: &str, detail: impl Into<String>) -> Finding {
    Finding {
        ok,
        label: label.to_string(),
        detail: detail.into(),
    }
}

pub fn hook_binaries(settings: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(events) = settings.get("hooks").and_then(Value::as_object) else {
        return out;
    };
    for groups in events.values().filter_map(Value::as_array) {
        for group in groups {
            for hook in group.get("hooks").and_then(Value::as_array).into_iter().flatten() {
                if let Some(command) = hook.get("command").and_then(Value::as_str) {
                    if let Some(bin) = command_binary(command) {
                        out.push(bin);
                    }
                }
            }
        }
    }
    out
}

fn command_binary(command: &str) -> Option<String> {
    let first = command.split_whitespace().next()?;
    let bin = first.trim_matches('\'').trim_matches('"');
    bin.ends_with("tekmor").then(|| bin.to_string())
}

pub fn resolve_on_path(program: &str, path_var: &str) -> bool {
    if program.contains('/') {
        return is_executable(Path::new(program));
    }
    path_var
        .split(':')
        .any(|dir| is_executable(&Path::new(dir).join(program)))
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub fn allowlist_program(command: &str) -> Option<&str> {
    command.split_whitespace().find(|token| !token.contains('='))
}

fn check_hooks(root: &Path, current: &Path) -> Vec<Finding> {
    let mut settings: Vec<PathBuf> = vec![root.join(".claude/settings.json")];
    if let Some(home) = dirs::home_dir() {
        settings.push(home.join(".claude/settings.json"));
    }
    let mut out = Vec::new();
    let mut found = 0;
    for path in settings.into_iter().filter(|p| p.exists()) {
        let Ok(text) = fs::read_to_string(&path) else {
            out.push(finding(false, "hooks", format!("cannot read {}", path.display())));
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            out.push(finding(false, "hooks", format!("{} is not valid JSON", path.display())));
            continue;
        };
        for bin in hook_binaries(&value) {
            found += 1;
            let bin_path = Path::new(&bin);
            if !is_executable(bin_path) {
                out.push(finding(false, "hooks", format!("{} points at {bin}, which is missing or not executable; run `tekmor install` again", path.display())));
            } else if bin_path != current {
                out.push(finding(false, "hooks", format!("{} runs {bin}, not this binary ({}); run `tekmor install` again", path.display(), current.display())));
            } else {
                out.push(finding(true, "hooks", format!("{} runs {bin}", path.display())));
            }
        }
    }
    if found == 0 {
        out.push(finding(true, "hooks", "no tekmor hooks installed (receipt-only mode)"));
    }
    out
}

fn check_git(root: &Path) -> Vec<Finding> {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let top = run(&["rev-parse", "--show-toplevel"]);
    let head = run(&["rev-parse", "--verify", "HEAD"]);
    vec![
        match top {
            Some(path) => finding(true, "git", format!("repository {path}")),
            None => finding(false, "git", "not inside a git repository"),
        },
        match head {
            Some(sha) => finding(true, "git", format!("HEAD {sha}")),
            None => finding(false, "git", "no commit yet; run `git commit` before verifying"),
        },
    ]
}

fn check_allowlist(root: &Path) -> Vec<Finding> {
    let commands = crate::verify::load_command_allowlist(root);
    if commands.is_empty() {
        return vec![finding(true, "allowlist", "no allowlist (run `tekmor verify init`)")];
    }
    let path_var = std::env::var("PATH").unwrap_or_default();
    commands
        .iter()
        .map(|cmd| match allowlist_program(cmd) {
            Some(program) if resolve_on_path(program, &path_var) => {
                finding(true, "allowlist", format!("`{cmd}` resolves to `{program}`"))
            }
            Some(program) => finding(false, "allowlist", format!("`{cmd}`: `{program}` is not on PATH")),
            None => finding(false, "allowlist", format!("`{cmd}` has no program name")),
        })
        .collect()
}

fn check_store() -> Vec<Finding> {
    let Ok(config) = crate::config::Config::load() else {
        return vec![finding(false, "store", "cannot resolve TEKMOR_HOME")];
    };
    let db = &config.db_path;
    match fs::metadata(db) {
        Ok(meta) if !meta.permissions().readonly() => {
            vec![finding(true, "store", format!("writable {}", db.display()))]
        }
        Ok(_) => vec![finding(false, "store", format!("{} is read-only", db.display()))],
        Err(_) => vec![finding(true, "store", format!("{} not created yet (created on first run)", db.display()))],
    }
}

pub fn run(root: &Path) -> Result<bool> {
    let current = std::env::current_exe()?;
    let mut findings = Vec::new();
    findings.extend(check_hooks(root, &current));
    findings.extend(check_git(root));
    findings.extend(check_allowlist(root));
    findings.extend(check_store());
    let mut all_ok = true;
    for f in &findings {
        let tag = if f.ok { "ok  " } else { "FAIL" };
        println!("{tag} {:<10} {}", f.label, f.detail);
        all_ok &= f.ok;
    }
    Ok(all_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tekmor_hook_binaries_in_settings() {
        let settings = serde_json::json!({
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "'/opt/homebrew/bin/tekmor' stop-gate"}]}],
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/usr/bin/other bash-gate"}]}]
            }
        });
        assert_eq!(hook_binaries(&settings), vec!["/opt/homebrew/bin/tekmor".to_string()]);
    }

    #[test]
    fn skips_environment_assignments_to_find_the_program() {
        assert_eq!(allowlist_program("SWE=1 PYTHONPATH=x pytest -rA"), Some("pytest"));
        assert_eq!(allowlist_program("sh check.sh"), Some("sh"));
        assert_eq!(allowlist_program("FOO=bar"), None);
    }

    #[test]
    fn resolves_programs_on_the_given_path_only() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let tool = dir.path().join("mytool");
        std::fs::write(&tool, "#!/bin/sh\n").expect("write tool");
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).expect("chmod tool");
        let path = dir.path().to_string_lossy().to_string();
        assert!(resolve_on_path("mytool", &path));
        assert!(!resolve_on_path("absent-tool", &path));
    }
}
