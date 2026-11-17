//!
//! Project playbooks live at `<project>/.verify/`. Hashes of verify config /

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::db::store::BrainStore;

pub const VERIFY_DIR_NAME: &str = ".verify";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifySource {
    Cli,
    Orchestrator,
}

impl VerifySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Orchestrator => "orchestrator",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cli" => Ok(Self::Cli),
            "orchestrator" | "spine" => Ok(Self::Orchestrator),
            other => bail!(
                "unknown verify source `{other}` (expected cli|orchestrator)"
            ),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct VerifyTomlFile {
    #[serde(default)]
    verify: VerifyTomlSection,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct VerifyTomlSection {
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    default: Option<DefaultStepFile>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct DefaultStepFile {
    command: Vec<String>,
    #[serde(default = "default_step_timeout_secs")]
    timeout_secs: u64,
    #[serde(default = "default_step_hook")]
    hook: bool,
}

fn default_step_timeout_secs() -> u64 {
    300
}

fn default_step_hook() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultStep {
    pub command: Vec<String>,
    pub timeout: Duration,
    pub hook: bool,
}

pub const TIMEOUT_EXIT_CODE: i32 = 124;
pub const MAX_STEP_TIMEOUT_SECS: u64 = 600;
const VERIFY_RUN_TIMEOUT: Duration = Duration::from_secs(900);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub fn load_default_step(project_root: &Path) -> Result<Option<DefaultStep>> {
    for rel in [".verify/verify.toml", "verify.toml"] {
        let Ok(raw) = fs::read_to_string(project_root.join(rel)) else {
            continue;
        };
        let parsed: VerifyTomlFile =
            toml::from_str(&raw).with_context(|| format!("invalid verify config {rel}"))?;
        let Some(step) = parsed.verify.default else {
            continue;
        };
        if step.command.is_empty() {
            bail!("[verify.default] command is empty in {rel}");
        }
        if !(1..=MAX_STEP_TIMEOUT_SECS).contains(&step.timeout_secs) {
            bail!("[verify.default] timeout_secs must be between 1 and {MAX_STEP_TIMEOUT_SECS} in {rel}");
        }
        return Ok(Some(DefaultStep {
            command: step.command,
            timeout: Duration::from_secs(step.timeout_secs),
            hook: step.hook,
        }));
    }
    Ok(None)
}

const OUTPUT_TAIL_BYTES: u64 = 4000;

fn run_process(
    cmd: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    capture: bool,
) -> Result<(i32, String)> {
    if cmd.is_empty() {
        bail!("cannot run an empty command");
    }
    let mut command = Command::new(&cmd[0]);
    command.args(&cmd[1..]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = if capture {
        let file = tempfile::tempfile()?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(file.try_clone()?))
            .stderr(Stdio::from(file.try_clone()?));
        Some(file)
    } else {
        None
    };
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn {}", cmd[0]))?;
    let started = Instant::now();
    let exit_code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code().unwrap_or(1);
        }
        if started.elapsed() >= timeout {
            child.kill().ok();
            child.wait()?;
            break TIMEOUT_EXIT_CODE;
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    Ok((exit_code, output.map(read_output).unwrap_or_default()))
}

fn read_output(mut file: File) -> String {
    let mut buf = Vec::new();
    if file.seek(SeekFrom::Start(0)).is_ok() {
        file.read_to_end(&mut buf).ok();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn output_tail(output: &str) -> String {
    let bytes = output.as_bytes();
    let start = bytes.len().saturating_sub(OUTPUT_TAIL_BYTES as usize);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[derive(Debug)]
pub struct StepRun {
    pub report: VerifyReport,
    pub output: String,
}

pub fn run_default_step(
    store: &BrainStore,
    project_root: &Path,
    source: VerifySource,
) -> Result<Option<StepRun>> {
    let Some(step) = load_default_step(project_root)? else {
        return Ok(None);
    };
    let joined = step.command.join(" ");
    if !command_is_allowlisted(&joined, Some(project_root)) {
        bail!(
            "default verify step `{joined}` is not in the verify allowlist; add it to commands in .verify/verify.toml"
        );
    }
    let (exit_code, output) = run_process(&step.command, Some(project_root), step.timeout, true)?;
    let report = record_from_result(
        store,
        exit_code,
        &joined,
        VerifyRecord {
            route_log_id: None,
            cause_tag: None,
            config_path: None,
            scenarios_dir: None,
            cwd: Some(project_root),
            source,
        },
    )?;
    Ok(Some(StepRun {
        report,
        output: output_tail(&output),
    }))
}

/// Load project command allowlist from `<root>/.verify/verify.toml` or `<root>/verify.toml`.
pub fn load_command_allowlist(project_root: &Path) -> Vec<String> {
    for rel in [".verify/verify.toml", "verify.toml"] {
        let path = project_root.join(rel);
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(parsed) = toml::from_str::<VerifyTomlFile>(&raw) {
            if !parsed.verify.commands.is_empty() {
                return parsed.verify.commands;
            }
        }
    }
    Vec::new()
}

/// True if `cmd` matches an allowlist entry (substring or as prefix of the joined command).
/// Empty allowlist → fallback to common runners (cargo test, pytest, .verify/, mvnw verify).
pub fn command_is_allowlisted(cmd: &str, project_root: Option<&Path>) -> bool {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return false;
    }
    let list = project_root.map(load_command_allowlist).unwrap_or_default();
    if list.is_empty() {
        return default_allowlist_match(cmd);
    }
    list.iter().any(|entry| {
        let e = entry.trim();
        !e.is_empty() && (cmd == e || cmd.starts_with(e) || cmd.contains(e))
    })
}

fn default_allowlist_match(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    lower.contains("cargo test")
        || lower.contains("cargo clippy")
        || lower.contains("cargo check")
        || lower.contains("pytest")
        || lower.contains("npm test")
        || lower.contains("bun test")
        || lower.contains("go test")
        || lower.contains("make test")
        || lower.contains("./mvnw")
        || lower.contains("mvnw ")
        || lower.contains(".verify/")
        || lower.contains("tekmor proofs")
        || lower.contains("tekmor doctor")
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyReport {
    pub id: String,
    pub exit_code: i32,
    pub success: bool,
    pub cause_tag: Option<String>,
    pub verify_config_hash: Option<String>,
    pub scenario_dir_hash: Option<String>,
    pub cmd: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_root: Option<String>,
}

/// Walk ancestors of `start` for a directory containing `.verify/`.
pub fn find_verify_root(start: &Path) -> Option<PathBuf> {
    let mut cur = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        let candidate = cur.join(VERIFY_DIR_NAME);
        if candidate.is_dir() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn resolve_scenarios_dir(
    explicit: Option<&Path>,
    cwd: Option<&Path>,
) -> Result<(Option<PathBuf>, Option<PathBuf>)> {
    if let Some(dir) = explicit {
        return Ok((Some(dir.to_path_buf()), None));
    }
    let start = cwd
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if let Some(root) = find_verify_root(&start) {
        let verify_dir = root.join(VERIFY_DIR_NAME);
        return Ok((Some(verify_dir), Some(root)));
    }
    Ok((None, None))
}

pub struct VerifyRecord<'a> {
    pub route_log_id: Option<&'a str>,
    pub cause_tag: Option<&'a str>,
    pub config_path: Option<&'a Path>,
    pub scenarios_dir: Option<&'a Path>,
    pub cwd: Option<&'a Path>,
    pub source: VerifySource,
}

pub struct VerifyRun<'a> {
    pub route_log_id: Option<&'a str>,
    pub cause_tag: Option<&'a str>,
    pub config_path: Option<&'a Path>,
    pub scenarios_dir: Option<&'a Path>,
    pub source: VerifySource,
    pub save_artifact: bool,
}

/// Record an already-known exit code (no command spawn).
pub fn record_from_result(
    store: &BrainStore,
    exit_code: i32,
    cmd: &str,
    record: VerifyRecord<'_>,
) -> Result<VerifyReport> {
    let VerifyRecord {
        route_log_id,
        cause_tag,
        config_path,
        scenarios_dir,
        cwd,
        source,
    } = record;
    if cmd.trim().is_empty() {
        bail!("verify record requires a non-empty cmd summary after --");
    }
    let success = exit_code == 0;
    let config_hash = config_path.map(hash_path).transpose()?;
    let (scenarios, verify_root) = resolve_scenarios_dir(scenarios_dir, cwd)?;
    let scenarios_hash = scenarios.as_ref().map(|p| hash_tree(p)).transpose()?;
    let cause = cause_tag.map(|s| s.to_string()).or_else(|| {
        if success {
            None
        } else {
            Some(cmd.to_string())
        }
    });
    let resolved_log_id: Option<String> = route_log_id.map(String::from);
    let device_id = store.ensure_device_id()?;
    let project_key = crate::project_key::project_key_if_known(cwd.unwrap_or(Path::new(".")));
    let id = store.insert_verify_outcome(
        resolved_log_id.as_deref(),
        exit_code,
        cause.as_deref(),
        config_hash.as_deref(),
        scenarios_hash.as_deref(),
        cmd,
        &device_id,
        source.as_str(),
        project_key.as_deref(),
    )?;
    if let Some(root) = cwd.and_then(crate::config::find_repo_root) {
        if let Some(hash) = code_tree_hash(&root) {
            store.set_verify_outcome_code_hash(&id, &hash)?;
        }
        if let Some(sha) = git_head_sha(&root) {
            store.set_verify_outcome_head_sha(&id, &sha)?;
        }
    }

    Ok(VerifyReport {
        id,
        exit_code,
        success,
        cause_tag: cause,
        verify_config_hash: config_hash,
        scenario_dir_hash: scenarios_hash,
        cmd: cmd.to_string(),
        source: source.as_str().to_string(),
        verify_root: verify_root.map(|p| p.display().to_string()),
    })
}

/// Initialize `.verify/` in `project_root` with `verify.toml`, `run.sh`, and `README.md`.
fn default_step_for(project_root: &Path) -> Option<&'static [&'static str]> {
    if project_root.join("Cargo.toml").exists() {
        return Some(&["cargo", "test"]);
    }
    if project_root.join("package.json").exists() {
        return Some(&["npm", "test"]);
    }
    if project_root.join("pyproject.toml").exists()
        || project_root.join("requirements.txt").exists()
        || project_root.join("pytest.ini").exists()
    {
        return Some(&["pytest"]);
    }
    if project_root.join("go.mod").exists() {
        return Some(&["go", "test", "./..."]);
    }
    if project_root.join("Makefile").exists() {
        return Some(&["make", "test"]);
    }
    None
}

pub fn init_verify(project_root: &Path, force: bool) -> Result<PathBuf> {
    let verify_dir = project_root.join(VERIFY_DIR_NAME);
    fs::create_dir_all(&verify_dir)
        .with_context(|| format!("create verify directory at {}", verify_dir.display()))?;

    let toml_path = verify_dir.join("verify.toml");
    if force || !toml_path.exists() {
        let mut commands = Vec::new();
        if project_root.join("Cargo.toml").exists() {
            commands.push("cargo test".to_string());
            commands.push("cargo clippy".to_string());
            commands.push("cargo check".to_string());
        }
        let node_runner = if project_root.join("pnpm-lock.yaml").exists() {
            Some("pnpm test")
        } else if project_root.join("yarn.lock").exists() {
            Some("yarn test")
        } else if project_root.join("bun.lockb").exists() {
            Some("bun test")
        } else if project_root.join("package.json").exists() {
            Some("npm test")
        } else {
            None
        };
        commands.extend(node_runner.map(str::to_string));
        if fs::read_to_string(project_root.join("Gemfile")).is_ok_and(|text| text.contains("rspec"))
        {
            commands.push("bundle exec rspec".to_string());
        }
        for (marker, command) in [
            ("deno.json", "deno test"),
            ("pom.xml", "mvn test"),
            ("build.gradle", "gradle test"),
            ("mix.exs", "mix test"),
        ] {
            if project_root.join(marker).exists() {
                commands.push(command.to_string());
            }
        }
        if project_root.join("pyproject.toml").exists()
            || project_root.join("requirements.txt").exists()
            || project_root.join("pytest.ini").exists()
        {
            commands.push("pytest".to_string());
        }
        if project_root.join("go.mod").exists() {
            commands.push("go test ./...".to_string());
        }
        if project_root.join("Makefile").exists() {
            commands.push("make test".to_string());
        }
        if !commands.is_empty() {
            commands.push(".verify/run.sh".to_string());
        }

        let mut toml_content = String::from("[verify]\ncommands = [\n");
        for cmd in &commands {
            toml_content.push_str(&format!("  \"{cmd}\",\n"));
        }
        toml_content.push_str("]\n");
        if let Some(argv) = default_step_for(project_root) {
            let quoted = argv
                .iter()
                .map(|arg| format!("\"{arg}\""))
                .collect::<Vec<_>>()
                .join(", ");
            toml_content.push_str(&format!(
                "\n[verify.default]\ncommand = [{quoted}]\ntimeout_secs = 300\n"
            ));
        }

        fs::write(&toml_path, toml_content)
            .with_context(|| format!("write {}", toml_path.display()))?;
    }

    let run_sh_path = verify_dir.join("run.sh");
    if force || !run_sh_path.exists() {
        let script = r##"#!/usr/bin/env bash
set -uo pipefail

FEATURE="${1:-all}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
OUT="$ROOT/.verify/artifacts/$RUN_ID"
mkdir -p "$OUT"
FAILED=0

record() {
  echo "$1" | tee -a "$OUT/summary.txt"
}

run_phase() {
  local phase="$1"
  shift
  local log="$OUT/$phase.log"
  if "$@" >"$log" 2>&1; then
    record "[pass] $phase"
  else
    local code=$?
    record "[fail] $phase (exit $code, log: $log)"
    FAILED=1
  fi
}

cd "$ROOT"
record "run $RUN_ID feature=$FEATURE"

if [ -f .verify/doctor.sh ]; then
  run_phase doctor bash .verify/doctor.sh
  if [ "$FAILED" -ne 0 ]; then
    record "doctor failed; no tests were run"
    exit 1
  fi
else
  record "[skip] doctor: no .verify/doctor.sh"
fi

if [ -f Cargo.toml ]; then
  if [ "$FEATURE" = "all" ]; then
    run_phase unit cargo test
  else
    run_phase unit cargo test "$FEATURE"
  fi
elif [ -f package.json ]; then
  run_phase unit npm test
elif [ -f pyproject.toml ] || [ -f requirements.txt ]; then
  if [ "$FEATURE" = "all" ]; then
    run_phase unit pytest
  else
    run_phase unit pytest -k "$FEATURE"
  fi
elif [ -f Makefile ]; then
  run_phase unit make test
else
  record "[skip] unit: no test runner detected; edit .verify/run.sh"
fi

if [ -f .verify/static.sh ]; then
  run_phase static bash .verify/static.sh
else
  record "[skip] static: no .verify/static.sh"
fi

if [ -f .verify/functional/run.sh ]; then
  run_phase functional bash .verify/functional/run.sh
else
  record "[skip] functional: no .verify/functional/run.sh"
fi

record "artifacts: $OUT"
exit "$FAILED"
"##;
        fs::write(&run_sh_path, script)
            .with_context(|| format!("write {}", run_sh_path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&run_sh_path, fs::Permissions::from_mode(0o755));
        }
    }

    for dir in ["artifacts", "functional"] {
        let keep = verify_dir.join(dir).join(".gitkeep");
        if !keep.exists() {
            fs::create_dir_all(verify_dir.join(dir))
                .with_context(|| format!("create {dir} directory"))?;
            fs::write(&keep, "").with_context(|| format!("write {}", keep.display()))?;
        }
    }

    let readme_path = verify_dir.join("README.md");
    if force || !readme_path.exists() {
        let readme = r#"# Project Verification (`.verify/`)

Layout follows the agstack verification skill.

```text
.verify/
├── verify.toml        allowlist of trusted commands
├── run.sh             runner: doctor, unit, static, functional
├── doctor.sh          optional read-only environment checks (create it to enable)
├── static.sh          optional static checks (create it to enable)
├── functional/        optional functional suites; functional/run.sh runs when present
└── artifacts/         one folder per run, kept after the run
```

## Usage
- Run the whole suite: `tekmor verify -- .verify/run.sh`
- Run one feature: `tekmor verify -- .verify/run.sh <feature>`
- Each run writes `summary.txt` and one log per phase under `.verify/artifacts/<RUN_ID>/`.
- Allowlisted runs are recorded into the local brain database, and `tekmor verify receipt` shows the result for the current commit.
- Edit `verify.toml` to change the allowlist. Commands outside it are not evidence.
"#;
        fs::write(&readme_path, readme)
            .with_context(|| format!("write {}", readme_path.display()))?;
    }

    Ok(verify_dir)
}

pub fn run_verify(store: &BrainStore, cmd: &[String], run: VerifyRun<'_>) -> Result<VerifyReport> {
    let VerifyRun {
        route_log_id,
        cause_tag,
        config_path,
        scenarios_dir,
        source,
        save_artifact,
    } = run;
    if cmd.is_empty() {
        bail!("usage: tekmor verify [--source cli|orchestrator] [--config PATH] [--scenarios DIR] [--route-log-id ID] [--cause TAG] -- <cmd> [args...]");
    }
    let joined = cmd.join(" ");
    let project_root = std::env::current_dir()
        .ok()
        .map(|cwd| crate::config::find_repo_root(&cwd).unwrap_or(cwd));
    if !command_is_allowlisted(&joined, project_root.as_deref()) {
        bail!("`{joined}` is not in the verify allowlist; add it to .verify/verify.toml");
    }
    let (exit_code, output) = run_process(cmd, None, VERIFY_RUN_TIMEOUT, true)?;
    print!("{output}");
    let cwd = std::env::current_dir().ok();
    let report = record_from_result(
        store,
        exit_code,
        &cmd.join(" "),
        VerifyRecord {
            route_log_id,
            cause_tag,
            config_path,
            scenarios_dir,
            cwd: cwd.as_deref(),
            source,
        },
    )?;
    if save_artifact {
        let path = save_artifact_log(
            &artifacts_dir(),
            project_root.as_deref(),
            &report.id,
            &output,
        )?;
        eprintln!("artifact saved: {}", path.display());
    }
    if let Some(root) = project_root
        .as_deref()
        .filter(|root| root.join(".verify").is_dir())
    {
        save_repo_artifact(root, &report.id, &output)?;
    }
    Ok(report)
}

pub fn repo_artifact_path(root: &Path, report_id: &str) -> PathBuf {
    root.join(".verify")
        .join("artifacts")
        .join(format!("{report_id}.log"))
}

fn save_repo_artifact(root: &Path, report_id: &str, output: &str) -> Result<PathBuf> {
    let path = repo_artifact_path(root, report_id);
    fs::create_dir_all(path.parent().context("artifact has a parent")?)?;
    fs::write(&path, output).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn artifacts_dir() -> PathBuf {
    crate::global_workspace::autonomic_root().join("artifacts")
}

fn save_artifact_log(
    root: &Path,
    project_root: Option<&Path>,
    report_id: &str,
    output: &str,
) -> Result<PathBuf> {
    let project = project_root
        .and_then(Path::file_name)
        .unwrap_or_else(|| OsStr::new("unknown"));
    let dir = root.join(project);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join(format!("{report_id}.log"));
    fs::write(&path, output).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn hash_path(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

const GENERATED_SEGMENTS: [&str; 5] = ["__pycache__", ".pytest_cache", ".mypy_cache", "node_modules", "target"];

pub fn is_code_path(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.iter().any(|segment| matches!(*segment, ".verify" | ".claude" | ".git")) {
        return false;
    }
    if segments.iter().any(|segment| GENERATED_SEGMENTS.contains(segment)) || path.ends_with(".pyc") {
        return false;
    }
    !(path.ends_with(".md") || path.ends_with(".rst"))
}

fn counts_toward_tree(path: &str) -> bool {
    let doc = [".md", ".rst"].iter().any(|ext| path.ends_with(ext));
    let ignored = path.starts_with(".verify/artifacts/")
        || path.starts_with(".verify/features/")
        || path.starts_with(".verify/knowledge-base/")
        || path.starts_with(".claude/");
    !doc && !ignored
}

pub fn is_proof_input(path: &str) -> bool {
    path == ".verify/verify.toml" || path == ".verify/run.sh" || path.starts_with(".verify/checks/")
}

pub fn code_tree_hash(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut paths: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.get(3..))
        .filter_map(|path| path.rsplit(" -> ").next())
        .filter(|path| counts_toward_tree(path))
        .map(str::to_string)
        .collect();
    paths.sort();
    paths.dedup();
    let mut hasher = Sha256::new();
    for path in paths {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        match fs::read(root.join(&path)) {
            Ok(bytes) => hasher.update(&bytes),
            Err(_) => hasher.update(b"<deleted>"),
        }
        hasher.update([0]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

pub fn git_head_sha(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

fn hash_tree(dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut paths: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    paths.sort();
    for path in paths {
        let rel = path.strip_prefix(dir).unwrap_or(&path);
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(fs::read(&path)?);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::store::BrainStore;
    use tempfile::tempdir;

    #[test]
    fn find_verify_root_walks_ancestors() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        assert!(find_verify_root(&nested).is_none());
        fs::create_dir_all(dir.path().join(".verify")).unwrap();
        assert_eq!(find_verify_root(&nested).as_deref(), Some(dir.path()));
    }

    #[test]
    fn record_from_result_stores_source_and_verify_hash() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("t.db");
        let store = BrainStore::open(&db).unwrap();
        fs::create_dir_all(dir.path().join(".verify")).unwrap();
        fs::write(dir.path().join(".verify/README.md"), b"playbook").unwrap();

        let report = record_from_result(
            &store,
            0,
            "cargo test",
            VerifyRecord {
                route_log_id: None,
                cause_tag: None,
                config_path: None,
                scenarios_dir: None,
                cwd: Some(dir.path()),
                source: VerifySource::Cli,
            },
        )
        .unwrap();
        assert_eq!(report.source, "cli");
        assert!(report.scenario_dir_hash.is_some());
        assert_eq!(
            report.verify_root.as_deref(),
            Some(dir.path().to_str().unwrap())
        );

        let rows = store.list_verify_outcomes(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source.as_deref(), Some("cli"));
        assert!(rows[0].scenario_dir_hash.is_some());
    }

    #[test]
    fn orchestrator_source_parses_and_is_trusted() {
        assert_eq!(
            VerifySource::parse("orchestrator").unwrap(),
            VerifySource::Orchestrator
        );
        assert_eq!(
            VerifySource::parse("spine").unwrap(),
            VerifySource::Orchestrator
        );
        assert!(VerifySource::parse("mcp").is_err());
    }

    #[test]
    fn allowlist_from_verify_toml_and_rejects_true() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".verify")).unwrap();
        fs::write(
            dir.path().join(".verify/verify.toml"),
            r#"
[verify]
commands = ["./mvnw -q verify", ".verify/run.sh", "cargo test"]
"#,
        )
        .unwrap();
        assert!(command_is_allowlisted(
            "./mvnw -q verify -DskipITs",
            Some(dir.path())
        ));
        assert!(command_is_allowlisted(
            "cargo test -p tekmor",
            Some(dir.path())
        ));
        assert!(!command_is_allowlisted("true", Some(dir.path())));
        assert!(!command_is_allowlisted("echo hi", Some(dir.path())));
    }

    #[test]
    fn init_verify_runs_rspec_only_when_the_gemfile_names_it() {
        let with_rspec = tempdir().unwrap();
        fs::write(with_rspec.path().join("Gemfile"), b"gem \"rspec\"\n").unwrap();
        let verify_path = init_verify(with_rspec.path(), false).unwrap();
        let toml_str = fs::read_to_string(verify_path.join("verify.toml")).unwrap();
        assert!(toml_str.contains("\"bundle exec rspec\""));

        let without = tempdir().unwrap();
        fs::write(without.path().join("Gemfile"), b"gem \"rails\"\n").unwrap();
        let verify_path = init_verify(without.path(), false).unwrap();
        let toml_str = fs::read_to_string(verify_path.join("verify.toml")).unwrap();
        assert!(!toml_str.contains("rspec"));
    }

    #[test]
    fn init_verify_uses_the_node_runner_its_lockfile_names() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("package.json"), b"{}").unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), b"").unwrap();
        fs::write(dir.path().join("deno.json"), b"{}").unwrap();
        let verify_path = init_verify(dir.path(), false).unwrap();
        let toml_str = fs::read_to_string(verify_path.join("verify.toml")).unwrap();
        assert!(toml_str.contains("\"pnpm test\""));
        assert!(!toml_str.contains("\"npm test\""));
        assert!(toml_str.contains("\"deno test\""));
    }

    #[test]
    fn init_verify_in_an_unrecognised_project_allowlists_nothing() {
        let dir = tempdir().unwrap();
        let verify_path = init_verify(dir.path(), false).unwrap();
        let toml_str = fs::read_to_string(verify_path.join("verify.toml")).unwrap();
        assert!(!toml_str.contains("run.sh"));
        assert!(load_command_allowlist(dir.path()).is_empty());
    }

    #[test]
    fn init_verify_detects_a_makefile_as_a_test_runner() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Makefile"), b"test:\n\ttrue\n").unwrap();
        let verify_path = init_verify(dir.path(), false).unwrap();
        let toml_str = fs::read_to_string(verify_path.join("verify.toml")).unwrap();
        assert!(toml_str.contains("\"make test\""));
        assert!(toml_str.contains("\".verify/run.sh\""));
    }

    #[test]
    fn save_artifact_log_files_output_under_project_name() {
        let root = tempdir().unwrap();
        let project = root.path().join("my-project");
        let path = save_artifact_log(root.path(), Some(&project), "run-1", "ok\n").unwrap();
        assert_eq!(path, root.path().join("my-project").join("run-1.log"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "ok\n");
    }

    #[test]
    fn repo_artifact_is_written_under_the_harness_folder() {
        let root = tempfile::tempdir().unwrap();
        let path = save_repo_artifact(root.path(), "run-3", "passed\n").unwrap();
        assert_eq!(path, repo_artifact_path(root.path(), "run-3"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "passed\n");
        assert!(path.starts_with(root.path().join(".verify").join("artifacts")));
    }

    #[test]
    fn save_artifact_log_uses_unknown_folder_without_project() {
        let root = tempdir().unwrap();
        let path = save_artifact_log(root.path(), None, "run-2", "boom").unwrap();
        assert_eq!(path, root.path().join("unknown").join("run-2.log"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "boom");
    }

    fn write_verify_toml(root: &Path, body: &str) {
        fs::create_dir_all(root.join(".verify")).unwrap();
        fs::write(root.join(".verify/verify.toml"), body).unwrap();
    }

    #[test]
    fn default_step_is_parsed_with_timeout_and_hook_flag() {
        let dir = tempdir().unwrap();
        write_verify_toml(
            dir.path(),
            "[verify]\ncommands = [\"sh -c\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\ntimeout_secs = 120\n",
        );
        let step = load_default_step(dir.path()).unwrap().unwrap();
        assert_eq!(step.command, vec!["sh", "-c", "true"]);
        assert_eq!(step.timeout, Duration::from_secs(120));
        assert!(step.hook);
    }

    #[test]
    fn missing_default_step_is_none() {
        let dir = tempdir().unwrap();
        write_verify_toml(dir.path(), "[verify]\ncommands = [\"cargo test\"]\n");
        assert!(load_default_step(dir.path()).unwrap().is_none());
    }

    #[test]
    fn default_step_rejects_out_of_range_timeout() {
        let dir = tempdir().unwrap();
        write_verify_toml(
            dir.path(),
            "[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\ntimeout_secs = 0\n",
        );
        assert!(load_default_step(dir.path()).is_err());
    }

    #[test]
    fn default_step_rejects_invalid_toml_instead_of_skipping_it() {
        let dir = tempdir().unwrap();
        write_verify_toml(dir.path(), "[verify.default\ncommand = ");
        assert!(load_default_step(dir.path()).is_err());
    }

    #[test]
    fn process_is_killed_when_it_exceeds_its_timeout() {
        let cmd = ["sh", "-c", "exec sleep 30"].map(String::from);
        let (code, _) = run_process(&cmd, None, Duration::from_millis(200), false).unwrap();
        assert_eq!(code, TIMEOUT_EXIT_CODE);
    }

    #[test]
    fn default_step_refuses_commands_outside_the_allowlist() {
        let dir = tempdir().unwrap();
        let store = BrainStore::open(&dir.path().join("t.db")).unwrap();
        write_verify_toml(
            dir.path(),
            "[verify.default]\ncommand = [\"echo\", \"hi\"]\n",
        );
        let err = run_default_step(&store, dir.path(), VerifySource::Orchestrator).unwrap_err();
        assert!(err.to_string().contains("not in the verify allowlist"));
    }

    #[test]
    fn default_step_runs_in_project_root_and_records_the_result() {
        let dir = tempdir().unwrap();
        let store = BrainStore::open(&dir.path().join("t.db")).unwrap();
        write_verify_toml(
            dir.path(),
            "[verify]\ncommands = [\"sh -c\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n",
        );
        let report = run_default_step(&store, dir.path(), VerifySource::Orchestrator)
            .unwrap()
            .unwrap()
            .report;
        assert!(report.success);
        assert_eq!(store.list_verify_outcomes(10).unwrap().len(), 1);
    }

    #[test]
    fn init_writes_a_default_step_for_a_cargo_project() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        init_verify(dir.path(), false).unwrap();
        let step = load_default_step(dir.path()).unwrap().unwrap();
        assert_eq!(step.command, vec!["cargo", "test"]);
        assert_eq!(step.timeout, Duration::from_secs(300));
    }

    #[test]
    fn init_writes_no_default_step_when_no_runner_is_detected() {
        let dir = tempdir().unwrap();
        init_verify(dir.path(), false).unwrap();
        assert!(load_default_step(dir.path()).unwrap().is_none());
    }

    #[test]
    fn default_step_timeout_may_not_exceed_the_ceiling() {
        let dir = tempdir().unwrap();
        write_verify_toml(
            dir.path(),
            &format!(
                "[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\ntimeout_secs = {}\n",
                MAX_STEP_TIMEOUT_SECS + 1
            ),
        );
        assert!(load_default_step(dir.path()).is_err());
    }

    #[test]
    fn init_verify_scaffolding() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            b"[package]\nname = \"test\"\n",
        )
        .unwrap();
        let verify_path = init_verify(dir.path(), false).unwrap();
        assert!(verify_path.exists());
        assert!(verify_path.join("verify.toml").exists());
        assert!(verify_path.join("run.sh").exists());
        assert!(verify_path.join("README.md").exists());

        let toml_str = fs::read_to_string(verify_path.join("verify.toml")).unwrap();
        assert!(toml_str.contains("cargo test"));
        assert!(toml_str.contains("\".verify/run.sh\""));
        assert!(!toml_str.contains("\".verify/\""));

        assert!(command_is_allowlisted("cargo test", Some(dir.path())));
        assert!(command_is_allowlisted(".verify/run.sh", Some(dir.path())));
    }
}
