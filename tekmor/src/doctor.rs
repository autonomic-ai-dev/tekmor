use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

const INTERPRETERS: &[&str] = &["sh", "bash", "zsh", "python", "python3", "node", "ruby", "perl"];
const INLINE_FLAGS: &[&str] = &["-c", "-m", "-e"];

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

fn hook_subcommands(settings: &Value) -> Vec<String> {
    let Some(events) = settings.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    events
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .flat_map(|group| group.get("hooks").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|hook| hook.get("command")?.as_str()?.split_whitespace().last().map(str::to_string))
        .collect()
}

fn missing_gate_hooks(settings: &Value) -> Vec<&'static str> {
    let installed = hook_subcommands(settings);
    if !installed.iter().any(|name| name == "stop-gate" || name == "bash-gate") {
        return Vec::new();
    }
    ["session-start", "edit-gate"]
        .into_iter()
        .filter(|name| !installed.iter().any(|installed| installed == name))
        .collect()
}

fn check_default_suite(root: &Path) -> Vec<Finding> {
    match crate::verify::load_default_step(root) {
        Ok(Some(_)) => vec![finding(true, "suite", "[verify.default] names the suite every receipt requires")],
        Ok(None) if !crate::verify::load_command_allowlist(root).is_empty() => vec![finding(
            false,
            "suite",
            "no [verify.default] in .verify/verify.toml, so any one passing allowlisted command reads PASSED; add a default that runs your full suite",
        )],
        _ => Vec::new(),
    }
}

fn is_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(key, _)| {
        !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn command_binary(command: &str) -> Option<String> {
    let mut tokens = command
        .split_whitespace()
        .map(|t| t.trim_matches('\'').trim_matches('"'))
        .peekable();
    if tokens.peek() == Some(&"env") {
        tokens.next();
    }
    let bin = tokens.find(|t| !is_assignment(t))?;
    (Path::new(bin).file_name().and_then(|n| n.to_str()) == Some("tekmor")).then(|| bin.to_string())
}

pub fn same_binary(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn find_on_path(program: &str, path_var: &str) -> Option<PathBuf> {
    path_var
        .split(':')
        .map(|dir| Path::new(dir).join(program))
        .find(|candidate| is_executable(candidate))
}

pub fn resolve_on_path(program: &str, root: &Path, path_var: &str) -> bool {
    if program.contains('/') {
        return is_executable(&root.join(program));
    }
    find_on_path(program, path_var).is_some()
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn interpreter_script<'a>(program: &str, args: &[&'a str]) -> Option<&'a str> {
    let name = Path::new(program).file_name()?.to_str()?;
    if !INTERPRETERS.contains(&name) {
        return None;
    }
    if args.first().is_some_and(|first| INLINE_FLAGS.contains(first)) {
        return None;
    }
    args.iter().copied().find(|a| !a.starts_with('-'))
}

fn check_command(cmd: &str, root: &Path, path_var: &str) -> Finding {
    let mut tokens: Vec<&str> = cmd.split_whitespace().skip_while(|t| t.contains('=')).collect();
    if tokens.last() == Some(&"...") {
        tokens.pop();
    }
    let Some((&program, args)) = tokens.split_first() else {
        return finding(false, "allowlist", format!("`{cmd}` has no program name"));
    };
    if !resolve_on_path(program, root, path_var) {
        let detail = if program.contains('/') {
            format!("`{cmd}`: `{program}` is not an executable file under {}", root.display())
        } else {
            format!("`{cmd}`: `{program}` is not on PATH")
        };
        return finding(false, "allowlist", detail);
    }
    if let Some(script) = interpreter_script(program, args) {
        if !root.join(script).is_file() {
            return finding(false, "allowlist", format!("`{cmd}`: script `{script}` does not exist"));
        }
    }
    finding(true, "allowlist", format!("`{cmd}` resolves to `{program}`"))
}

fn check_hooks(root: &Path, current: &Path, path_var: &str) -> Vec<Finding> {
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
        let missing = missing_gate_hooks(&value);
        if !missing.is_empty() {
            out.push(finding(false, "hooks", format!("{} lacks the {} hook; run `tekmor install` again", path.display(), missing.join(" and "))));
        }
        for bin in hook_binaries(&value) {
            found += 1;
            let bin_path = if bin.contains('/') {
                PathBuf::from(&bin)
            } else {
                find_on_path(&bin, path_var).unwrap_or_else(|| PathBuf::from(&bin))
            };
            if !is_executable(&bin_path) {
                out.push(finding(false, "hooks", format!("{} points at {bin}, which is missing or not executable; run `tekmor install` again", path.display())));
            } else if !same_binary(&bin_path, current) {
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

fn allowlist_parse_failures(root: &Path) -> Vec<Finding> {
    [".verify/verify.toml", "verify.toml"]
        .iter()
        .map(|rel| root.join(rel))
        .filter_map(|path| {
            let raw = fs::read_to_string(&path).ok()?;
            let err = toml::from_str::<toml::Value>(&raw).err()?;
            Some(finding(false, "allowlist", format!("{} does not parse: {}", path.display(), err.message())))
        })
        .collect()
}

fn check_allowlist(root: &Path, path_var: &str) -> Vec<Finding> {
    let mut out = allowlist_parse_failures(root);
    let commands = crate::verify::load_command_allowlist(root);
    if commands.is_empty() {
        if out.is_empty() {
            out.push(finding(true, "allowlist", "no allowlist (run `tekmor verify init`)"));
        }
        return out;
    }
    out.extend(commands.iter().map(|cmd| check_command(cmd, root, path_var)));
    out
}

fn check_drift(root: &Path) -> Vec<Finding> {
    let drift = crate::drift::check(root);
    if drift.is_empty() {
        return vec![finding(true, "drift", "feature maps match the code and the allowlist")];
    }
    drift
        .into_iter()
        .map(|d| finding(false, "drift", format!("{}: {}", d.feature, d.problem)))
        .collect()
}

fn writable_dir(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_dir() && !m.permissions().readonly())
        .unwrap_or(false)
}

fn check_store(db: &Path) -> Vec<Finding> {
    let shown = db.display();
    let result = match fs::metadata(db) {
        Ok(meta) if meta.is_dir() => finding(false, "store", format!("{shown} is a directory, not a database file")),
        Ok(meta) if meta.permissions().readonly() => finding(false, "store", format!("{shown} is read-only")),
        Ok(_) => finding(true, "store", format!("writable {shown}")),
        Err(_) => match db.ancestors().skip(1).find(|a| a.exists()) {
            Some(dir) if writable_dir(dir) => finding(true, "store", format!("{shown} not created yet (created on first run)")),
            Some(dir) => finding(false, "store", format!("{shown} cannot be created: {} is not a writable directory", dir.display())),
            None => finding(false, "store", format!("{shown} cannot be created: no parent directory exists")),
        },
    };
    vec![result]
}

pub fn run(root: &Path) -> Result<bool> {
    let current = std::env::current_exe()?;
    let path_var = std::env::var("PATH").unwrap_or_default();
    let mut findings = Vec::new();
    findings.extend(check_hooks(root, &current, &path_var));
    findings.extend(check_git(root));
    findings.extend(check_allowlist(root, &path_var));
    findings.extend(check_default_suite(root));
    match crate::config::Config::load() {
        Ok(config) => findings.extend(check_store(&config.db_path)),
        Err(_) => findings.push(finding(false, "store", "cannot resolve TEKMOR_HOME")),
    }
    findings.extend(check_drift(root));
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

    fn details(findings: &[Finding]) -> Vec<String> {
        findings.iter().map(|f| format!("{} {}", f.ok, f.detail)).collect()
    }

    fn executable(path: &Path) {
        std::fs::write(path, "#!/bin/sh\n").expect("write executable");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod executable");
    }

    #[test]
    fn reports_a_feature_map_that_points_at_missing_code() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::create_dir_all(dir.path().join(".verify/features")).expect("features dir");
        std::fs::write(
            dir.path().join(".verify/features/app.md"),
            "---\nfeature: app\n---\n\n# app\n\n## Code\n\n- `src/missing.rs`\n",
        )
        .expect("feature map");
        let findings = check_drift(dir.path());
        assert!(findings.iter().any(|f| !f.ok && f.detail.contains("missing")), "{:?}", details(&findings));
    }

    #[test]
    fn an_install_from_before_the_session_and_edit_gates_is_reported() {
        let old = serde_json::json!({
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "'/x/tekmor' stop-gate"}]}],
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "'/x/tekmor' bash-gate"}]}]
            }
        });
        assert_eq!(missing_gate_hooks(&old), ["session-start", "edit-gate"]);
        let unrelated = serde_json::json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "other"}]}]}});
        assert!(missing_gate_hooks(&unrelated).is_empty());
    }

    #[test]
    fn a_missing_default_suite_is_a_failure_only_when_an_allowlist_exists() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        assert!(check_default_suite(dir.path()).is_empty());
        fs::create_dir_all(dir.path().join(".verify")).expect("verify dir");
        fs::write(dir.path().join(".verify/verify.toml"), "[verify]\ncommands = [\"make test\"]\n").expect("toml");
        let findings = check_default_suite(dir.path());
        assert!(findings.iter().any(|f| !f.ok && f.detail.contains("[verify.default]")), "{:?}", details(&findings));
        fs::write(
            dir.path().join(".verify/verify.toml"),
            "[verify]\ncommands = [\"make test\"]\n\n[verify.default]\ncommand = [\"make\", \"test\"]\n",
        )
        .expect("toml");
        assert!(check_default_suite(dir.path()).iter().all(|f| f.ok));
    }

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
    fn finds_tekmor_behind_env_wrappers_and_rejects_lookalikes() {
        let settings = serde_json::json!({
            "hooks": {
                "Stop": [{"hooks": [
                    {"type": "command", "command": "env TEKMOR=1 /x/tekmor stop-gate"},
                    {"type": "command", "command": "A=1 '/x/tekmor' stop-gate"},
                    {"type": "command", "command": "/usr/bin/nottekmor bash-gate"}
                ]}]
            }
        });
        assert_eq!(hook_binaries(&settings), vec!["/x/tekmor".to_string(), "/x/tekmor".to_string()]);
    }

    #[test]
    fn treats_a_symlink_as_the_binary_it_points_at() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let target = dir.path().join("tekmor-real");
        executable(&target);
        let link = dir.path().join("tekmor");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let other = dir.path().join("other");
        executable(&other);
        assert!(same_binary(&link, &target));
        assert!(!same_binary(&link, &other));
    }

    #[test]
    fn resolves_programs_on_the_given_path_only() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        executable(&dir.path().join("mytool"));
        let path = dir.path().to_string_lossy().to_string();
        assert!(resolve_on_path("mytool", dir.path(), &path));
        assert!(!resolve_on_path("absent-tool", dir.path(), &path));
    }

    #[test]
    fn resolves_a_relative_program_against_the_repo_root() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("bin")).expect("bin dir");
        executable(&dir.path().join("bin/tool"));
        let elsewhere = tempfile::TempDir::new().expect("other dir");
        assert!(resolve_on_path("./bin/tool", dir.path(), ""));
        assert!(check_command("./bin/tool --flag", dir.path(), "").ok);
        assert!(!resolve_on_path("./bin/tool", elsewhere.path(), ""));
    }

    #[test]
    fn checks_that_an_interpreter_script_exists() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin dir");
        executable(&bin.join("sh"));
        executable(&bin.join("python3"));
        std::fs::write(dir.path().join("present.sh"), "true\n").expect("present script");
        let path = bin.to_string_lossy().to_string();
        let missing = check_command("sh missing.sh", dir.path(), &path);
        assert!(!missing.ok && missing.detail.contains("does not exist"), "{}", missing.detail);
        assert!(check_command("sh present.sh", dir.path(), &path).ok);
        assert!(check_command("SWE=1 PYTHONPATH=x sh present.sh", dir.path(), &path).ok);
        assert!(check_command("python3 -c print", dir.path(), &path).ok);
        assert!(check_command("sh present.sh ...", dir.path(), &path).ok);
        assert!(check_command("sh -c 'FOO=bar sh missing.sh'", dir.path(), &path).ok);
    }

    #[test]
    fn reports_a_verify_toml_that_does_not_parse() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::create_dir_all(dir.path().join(".verify")).expect("verify dir");
        std::fs::write(dir.path().join(".verify/verify.toml"), "[verify\ncommands = [\n").expect("verify.toml");
        let findings = check_allowlist(dir.path(), "");
        assert!(findings.iter().any(|f| !f.ok && f.label == "allowlist" && f.detail.contains("does not parse")), "{:?}", details(&findings));
    }

    #[test]
    fn reports_a_database_path_that_is_a_directory() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let db = dir.path().join("brain.db");
        std::fs::create_dir_all(&db).expect("db dir");
        let findings = check_store(&db);
        assert!(findings.iter().any(|f| !f.ok && f.detail.contains("is a directory")), "{:?}", details(&findings));
        let fresh = dir.path().join("nested/brain.db");
        assert!(check_store(&fresh).iter().all(|f| f.ok), "{:?}", details(&check_store(&fresh)));
    }
}
