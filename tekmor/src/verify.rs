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
            "orchestrator" => Ok(Self::Orchestrator),
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
    commands: Vec<AllowEntryFile>,
    #[serde(default)]
    default: Option<DefaultStepFile>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum AllowEntryFile {
    Text(String),
    Argv(Vec<String>),
}

const OPEN_MARKER: &str = "...";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowEntry {
    pub argv: Vec<String>,
    pub open: bool,
}

impl AllowEntry {
    fn from_argv(mut argv: Vec<String>) -> Self {
        let open = argv.last().is_some_and(|last| last == OPEN_MARKER);
        if open {
            argv.pop();
        }
        Self { argv, open }
    }

    pub fn parse(text: &str) -> Self {
        let trimmed = text.trim();
        let mut body = trimmed;
        let mut open = false;
        if trimmed.split_whitespace().last() == Some(OPEN_MARKER) {
            body = trimmed[..trimmed.len() - OPEN_MARKER.len()].trim_end();
            open = true;
        }
        if is_shell_command(body) {
            return Self {
                argv: vec!["sh".to_string(), "-c".to_string(), body.to_string()],
                open: false,
            };
        }
        let mut entry = Self::from_argv(body.split_whitespace().map(str::to_string).collect());
        entry.open = open;
        entry
    }

    pub fn matches<S: AsRef<str>>(&self, argv: &[S]) -> bool {
        let length_ok = if self.open {
            argv.len() >= self.argv.len()
        } else {
            argv.len() == self.argv.len()
        };
        !self.argv.is_empty()
            && length_ok
            && self.argv.iter().zip(argv).all(|(token, arg)| token == arg.as_ref())
    }

    pub fn base(&self) -> String {
        render_argv(&self.argv)
    }

    pub fn display(&self) -> String {
        if self.open {
            format!("{} {OPEN_MARKER}", self.base())
        } else {
            self.base()
        }
    }
}

fn is_shell_command(body: &str) -> bool {
    if body
        .chars()
        .any(|c| "\"'`$|&;<>(){}*?[]~\\".contains(c))
    {
        return true;
    }
    body.split_whitespace().next().is_some_and(|first| {
        first.split_once('=').is_some_and(|(name, _)| {
            name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    })
}

impl From<AllowEntryFile> for AllowEntry {
    fn from(entry: AllowEntryFile) -> Self {
        match entry {
            AllowEntryFile::Text(text) => Self::parse(&text),
            AllowEntryFile::Argv(argv) => Self::from_argv(argv),
        }
    }
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

pub fn render_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_arg(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

pub fn run_default_step(
    store: &BrainStore,
    project_root: &Path,
    source: VerifySource,
) -> Result<Option<StepRun>> {
    let Some(step) = load_default_step(project_root)? else {
        return Ok(None);
    };
    let joined = render_argv(&step.command);
    if !command_is_allowlisted(step.command.as_slice(), Some(project_root)) {
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
pub fn load_allowlist(project_root: &Path) -> Vec<AllowEntry> {
    for rel in [".verify/verify.toml", "verify.toml"] {
        let path = project_root.join(rel);
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(parsed) = toml::from_str::<VerifyTomlFile>(&raw) {
            let entries: Vec<AllowEntry> = parsed
                .verify
                .commands
                .into_iter()
                .map(AllowEntry::from)
                .filter(|entry| !entry.argv.is_empty())
                .collect();
            if !entries.is_empty() {
                return entries;
            }
        }
    }
    Vec::new()
}

pub fn load_command_allowlist(project_root: &Path) -> Vec<String> {
    load_allowlist(project_root).iter().map(AllowEntry::base).collect()
}

pub fn command_is_allowlisted<S: AsRef<str>>(argv: &[S], project_root: Option<&Path>) -> bool {
    if argv.is_empty() {
        return false;
    }
    let list = project_root.map(load_allowlist).unwrap_or_default();
    if list.is_empty() {
        return default_allowlist_match(argv, project_root);
    }
    list.iter().any(|entry| entry.matches(argv))
}

pub fn command_text_is_allowlisted(cmd: &str, project_root: Option<&Path>) -> bool {
    let argv = AllowEntry::parse(cmd).argv;
    command_is_allowlisted(&argv, project_root)
}

const DEFAULT_RUNNERS: &[&str] = &[
    "cargo test",
    "cargo clippy",
    "cargo check",
    "pytest",
    "npm test",
    "bun test",
    "go test ./...",
    "make test",
    "./mvnw test",
    "mvnw test",
];

fn default_allowlist_match<S: AsRef<str>>(argv: &[S], project_root: Option<&Path>) -> bool {
    match argv.first().map(AsRef::as_ref) {
        Some(program) if program.starts_with(".verify/") => {
            argv.len() == 1 && verify_script_contained(program, project_root)
        }
        _ => DEFAULT_RUNNERS
            .iter()
            .any(|entry| AllowEntry::parse(entry).matches(argv)),
    }
}

fn verify_script_contained(program: &str, project_root: Option<&Path>) -> bool {
    let Some(base) = project_root
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
    else {
        return false;
    };
    let Ok(verify_dir) = base.join(VERIFY_DIR_NAME).canonicalize() else {
        return false;
    };
    let Ok(resolved) = base.join(program).canonicalize() else {
        return false;
    };
    resolved.starts_with(verify_dir)
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

fn resolve_scenarios_dir(cwd: Option<&Path>) -> (Option<PathBuf>, Option<PathBuf>) {
    let start = cwd
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if let Some(root) = find_verify_root(&start) {
        let verify_dir = root.join(VERIFY_DIR_NAME);
        return (Some(verify_dir), Some(root));
    }
    (None, None)
}

pub struct VerifyRecord<'a> {
    pub cwd: Option<&'a Path>,
    pub source: VerifySource,
}

pub struct VerifyRun {
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
    let VerifyRecord { cwd, source } = record;
    if cmd.trim().is_empty() {
        bail!("cannot record a run with an empty command");
    }
    let success = exit_code == 0;
    let (scenarios, verify_root) = resolve_scenarios_dir(cwd);
    let scenarios_hash = scenarios.as_ref().map(|p| hash_tree(p)).transpose()?;
    let cause = (!success).then(|| cmd.to_string());
    let device_id = store.ensure_device_id()?;
    let project_key = crate::project_key::project_key_if_known(cwd.unwrap_or(Path::new(".")));
    let id = store.insert_verify_outcome(
        exit_code,
        cause.as_deref(),
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
        verify_config_hash: None,
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
            commands.push(".verify/run.sh ...".to_string());
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

pub fn run_verify(store: &BrainStore, cmd: &[String], run: VerifyRun) -> Result<VerifyReport> {
    let VerifyRun { source, save_artifact } = run;
    if cmd.is_empty() {
        bail!("usage: tekmor verify [--source cli|orchestrator] [--save-artifact] -- <cmd> [args...]");
    }
    let joined = render_argv(cmd);
    let project_root = std::env::current_dir()
        .ok()
        .map(|cwd| crate::config::find_repo_root(&cwd).unwrap_or(cwd));
    if !command_is_allowlisted(cmd, project_root.as_deref()) {
        bail!("`{joined}` is not in the verify allowlist; add it to .verify/verify.toml");
    }
    let (exit_code, output) = run_process(cmd, None, VERIFY_RUN_TIMEOUT, true)?;
    print!("{output}");
    let cwd = std::env::current_dir().ok();
    let report = record_from_result(
        store,
        exit_code,
        &joined,
        VerifyRecord {
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

const GENERATED_SEGMENTS: [&str; 5] = ["__pycache__", ".pytest_cache", ".mypy_cache", "node_modules", "target"];

fn is_generated(path: &str) -> bool {
    path.split('/').any(|segment| GENERATED_SEGMENTS.contains(&segment)) || path.ends_with(".pyc")
}

pub fn is_code_path(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.iter().any(|segment| matches!(*segment, ".verify" | ".claude" | ".git")) {
        return false;
    }
    if is_generated(path) {
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
    !doc && !ignored && !is_generated(path)
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
        assert!(VerifySource::parse("spine").is_err());
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
        let root = Some(dir.path());
        assert!(command_text_is_allowlisted("./mvnw -q verify", root));
        assert!(command_text_is_allowlisted("cargo test", root));
        assert!(command_text_is_allowlisted(".verify/run.sh", root));
        assert!(!command_text_is_allowlisted("./mvnw -q verify -DskipITs", root));
        assert!(!command_text_is_allowlisted("cargo test -p tekmor", root));
        assert!(!command_text_is_allowlisted("true", root));
        assert!(!command_text_is_allowlisted("echo hi", root));
    }

    #[test]
    fn exact_entry_refuses_extra_args() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify"))?;
        fs::write(dir.path().join(".verify/verify.toml"), "[verify]\ncommands = [\"cargo test\"]\n")?;
        let root = Some(dir.path());
        assert!(command_is_allowlisted(&["cargo", "test"], root));
        assert!(!command_is_allowlisted(&["cargo", "test", "--no-run"], root));
        assert!(!command_is_allowlisted(&["cargo", "test", "-p", "x"], root));
        Ok(())
    }

    #[test]
    fn open_entry_accepts_trailing_args() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify"))?;
        fs::write(dir.path().join(".verify/verify.toml"), "[verify]\ncommands = [\"sh check.sh ...\"]\n")?;
        let root = Some(dir.path());
        assert!(command_is_allowlisted(&["sh", "check.sh"], root));
        assert!(command_is_allowlisted(&["sh", "check.sh", "unit"], root));
        assert!(!command_is_allowlisted(&["sh", "other.sh"], root));
        assert!(!command_is_allowlisted(&["sh"], root));
        assert_eq!(load_command_allowlist(dir.path()), ["sh check.sh"]);
        Ok(())
    }

    #[test]
    fn array_entry_matches_exact_argv() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify"))?;
        fs::write(
            dir.path().join(".verify/verify.toml"),
            "[verify]\ncommands = [[\"python3\", \"-c\", \"print(1)\"]]\n",
        )?;
        let root = Some(dir.path());
        assert!(command_is_allowlisted(&["python3", "-c", "print(1)"], root));
        assert!(!command_is_allowlisted(&["python3", "-c", "print(2)"], root));
        assert!(!command_is_allowlisted(&["python3", "-c"], root));
        Ok(())
    }

    #[test]
    fn mixed_string_and_array_entries_parse() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify"))?;
        fs::write(
            dir.path().join(".verify/verify.toml"),
            "[verify]\ncommands = [\"cargo test\", [\"sh\", \"a b.sh\", \"...\"]]\n",
        )?;
        let entries = load_allowlist(dir.path());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].display(), "sh 'a b.sh' ...");
        assert!(command_is_allowlisted(&["sh", "a b.sh", "x"], Some(dir.path())));
        Ok(())
    }

    #[test]
    fn an_entry_with_shell_syntax_parses_to_sh_c() {
        let ytdlp = r#"python devscripts/run_tests.py --pytest-args="-rA -v""#;
        let entry = AllowEntry::parse(ytdlp);
        assert_eq!(entry.argv, vec!["sh", "-c", ytdlp]);
        assert!(!entry.open);

        let frozen = r#"SWE100_FREEZE_DATE=2024-12-30T17:40:33 PYTHONPATH="/tmp/swe100-clock${PYTHONPATH:+:$PYTHONPATH}" pytest -rA test"#;
        let entry = AllowEntry::parse(frozen);
        assert_eq!(entry.argv, vec!["sh", "-c", frozen]);
        assert!(!entry.open);
    }

    #[test]
    fn plain_entries_still_split_on_whitespace() {
        assert_eq!(
            AllowEntry::parse("cargo test"),
            AllowEntry { argv: vec!["cargo".into(), "test".into()], open: false }
        );
        assert_eq!(
            AllowEntry::parse("sh check.sh ..."),
            AllowEntry { argv: vec!["sh".into(), "check.sh".into()], open: true }
        );
        assert_eq!(
            AllowEntry::parse(".verify/run.sh"),
            AllowEntry { argv: vec![".verify/run.sh".into()], open: false }
        );
    }

    #[test]
    fn a_leading_assignment_is_shell_but_a_flagged_one_is_not() {
        assert_eq!(
            AllowEntry::parse("FOO=bar pytest").argv,
            vec!["sh", "-c", "FOO=bar pytest"]
        );
        let flagged = AllowEntry::parse("pytest -k foo=bar");
        assert_eq!(flagged.argv, vec!["pytest", "-k", "foo=bar"]);
        assert!(!flagged.open);
    }

    #[test]
    fn a_shell_entry_matches_only_the_sh_c_argv() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify"))?;
        let body = r#"python devscripts/run_tests.py --pytest-args="-rA -v""#;
        fs::write(
            dir.path().join(".verify/verify.toml"),
            format!("[verify]\ncommands = ['{body}']\n"),
        )?;
        let root = Some(dir.path());
        assert!(command_is_allowlisted(&["sh", "-c", body], root));
        assert!(!command_is_allowlisted(
            &["python", "devscripts/run_tests.py", "--pytest-args=\"-rA", "-v\""],
            root
        ));
        assert!(!command_is_allowlisted(
            &["python", "devscripts/run_tests.py", "--pytest-args=-rA -v"],
            root
        ));
        assert!(command_text_is_allowlisted(body, root));
        assert_eq!(
            load_command_allowlist(dir.path()),
            [format!("sh -c '{body}'")]
        );
        Ok(())
    }

    #[test]
    fn allowlist_matches_argv_not_a_string_that_mentions_an_entry() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify"))?;
        fs::write(
            dir.path().join(".verify/verify.toml"),
            "[verify]\ncommands = [\"cargo test\", \"sh check.sh\"]\n",
        )?;
        let root = Some(dir.path());
        assert!(!command_is_allowlisted(&["echo", "cargo", "test"], root));
        assert!(!command_is_allowlisted(&["echo", "sh", "check.sh"], root));
        assert!(!command_is_allowlisted(&["cargo", "testing"], root));
        assert!(!command_is_allowlisted(&["cargo"], root));
        assert!(!command_is_allowlisted(&["cargo", "test", "-p", "x"], root));
        assert!(!command_is_allowlisted(&["cargo", "test", "--no-run"], root));
        assert!(command_is_allowlisted(&["cargo", "test"], root));
        assert!(command_is_allowlisted(&["sh", "check.sh"], root));
        Ok(())
    }

    #[test]
    fn default_runners_must_be_the_program_not_an_argument() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify"))?;
        fs::write(dir.path().join(".verify/run.sh"), "true")?;
        let root = Some(dir.path());
        assert!(command_is_allowlisted(&["cargo", "test"], root));
        assert!(command_is_allowlisted(&["pytest"], root));
        assert!(command_is_allowlisted(&[".verify/run.sh"], root));
        assert!(!command_is_allowlisted(&["pytest", "-q"], root));
        assert!(!command_is_allowlisted(&["pytest", "--collect-only"], root));
        assert!(!command_is_allowlisted(&["cargo", "test", "--no-run"], root));
        assert!(!command_is_allowlisted(&[".verify/run.sh", "unit"], root));
        assert!(!command_is_allowlisted(&["echo", "pytest"], root));
        assert!(!command_is_allowlisted(&["echo", "cargo", "test"], root));
        assert!(!command_is_allowlisted(&["echo", ".verify/run.sh"], root));
        assert!(!command_is_allowlisted::<&str>(&[], root));
        Ok(())
    }

    #[test]
    fn default_verify_program_must_stay_inside_verify_dir() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify/checks"))?;
        fs::write(dir.path().join(".verify/run.sh"), "true")?;
        fs::write(dir.path().join("evil.sh"), "true")?;
        let root = Some(dir.path());
        assert!(command_is_allowlisted(&[".verify/run.sh"], root));
        assert!(command_is_allowlisted(&[".verify/checks/../run.sh"], root));
        assert!(!command_is_allowlisted(&[".verify/../evil.sh"], root));
        assert!(!command_is_allowlisted(&[".verify/../../evil.sh"], root));
        assert!(!command_is_allowlisted(&[".verify/missing.sh"], root));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn default_verify_program_must_not_escape_through_a_symlink() -> Result<()> {
        let dir = tempdir()?;
        fs::create_dir_all(dir.path().join(".verify"))?;
        fs::write(dir.path().join("evil.sh"), "true")?;
        std::os::unix::fs::symlink("../evil.sh", dir.path().join(".verify/evil-link.sh"))?;
        assert!(!command_is_allowlisted(
            &[".verify/evil-link.sh"],
            Some(dir.path())
        ));
        Ok(())
    }

    #[test]
    fn render_argv_keeps_plain_words_and_quotes_the_rest() {
        let argv = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert_eq!(
            render_argv(&argv(&["cargo", "test", "-p", "tekmor"])),
            "cargo test -p tekmor"
        );
        assert_eq!(
            render_argv(&argv(&["sh", "check.sh", ";", "touch", "/tmp/x"])),
            "sh check.sh ';' touch /tmp/x"
        );
        assert_eq!(
            render_argv(&argv(&["echo", "", "it's"])),
            "echo '' 'it'\\''s'"
        );
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
        assert!(toml_str.contains("\".verify/run.sh ...\""));
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
            "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\ntimeout_secs = 120\n",
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
            "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n",
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
        assert!(toml_str.contains("\".verify/run.sh ...\""));
        assert!(!toml_str.contains("\".verify/\""));

        assert!(command_text_is_allowlisted("cargo test", Some(dir.path())));
        assert!(command_text_is_allowlisted(
            ".verify/run.sh",
            Some(dir.path())
        ));
        assert!(command_text_is_allowlisted(
            ".verify/run.sh unit",
            Some(dir.path())
        ));
    }

    #[test]
    fn a_leading_assignment_is_shell_only_when_it_names_a_variable() {
        for body in ["_A=1 pytest", "A1=1 pytest"] {
            assert_eq!(
                AllowEntry::parse(body).argv,
                ["sh", "-c", body],
                "{body}"
            );
        }
        for body in ["1A=1 pytest", "A-B=1 pytest"] {
            assert_eq!(
                AllowEntry::parse(body).argv,
                body.split_whitespace().collect::<Vec<_>>(),
                "{body}"
            );
        }
    }

    #[test]
    fn a_default_step_without_a_timeout_gets_300_seconds() {
        let dir = tempdir().unwrap();
        write_verify_toml(
            dir.path(),
            "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n",
        );
        let step = load_default_step(dir.path()).unwrap().unwrap();
        assert_eq!(step.timeout, Duration::from_secs(300));
    }

    #[test]
    fn a_failing_default_step_keeps_the_tail_of_its_output() {
        let dir = tempdir().unwrap();
        let store = BrainStore::open(&dir.path().join("t.db")).unwrap();
        write_verify_toml(
            dir.path(),
            "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"printf 'first\\nKNOWN_TAIL\\n'; exit 1\"]\n",
        );
        let run = run_default_step(&store, dir.path(), VerifySource::Orchestrator)
            .unwrap()
            .unwrap();
        assert!(!run.report.success);
        assert!(run.output.contains("KNOWN_TAIL"), "{:?}", run.output);
        assert!(!run.output.trim().is_empty());
    }

    #[test]
    fn a_failed_record_stores_the_command_as_its_cause_and_a_pass_stores_none() {
        let dir = tempdir().unwrap();
        let store = BrainStore::open(&dir.path().join("t.db")).unwrap();
        let record = VerifyRecord {
            cwd: Some(dir.path()),
            source: VerifySource::Cli,
        };
        let failed = record_from_result(&store, 7, "cargo test", record).unwrap();
        assert_eq!(failed.cause_tag.as_deref(), Some("cargo test"));
        let passed = record_from_result(
            &store,
            0,
            "cargo check",
            VerifyRecord {
                cwd: Some(dir.path()),
                source: VerifySource::Cli,
            },
        )
        .unwrap();
        assert_eq!(passed.cause_tag, None);
        let rows = store.list_verify_outcomes(10).unwrap();
        let failed_row = rows.iter().find(|row| row.cmd == "cargo test").unwrap();
        assert_eq!(failed_row.cause_tag.as_deref(), Some("cargo test"));
        let passed_row = rows.iter().find(|row| row.cmd == "cargo check").unwrap();
        assert_eq!(passed_row.cause_tag, None);
    }

    #[test]
    fn requirements_txt_alone_selects_pytest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), b"pytest\n").unwrap();
        assert_eq!(default_step_for(dir.path()), Some(&["pytest"][..]));
        let verify_path = init_verify(dir.path(), false).unwrap();
        let toml_str = fs::read_to_string(verify_path.join("verify.toml")).unwrap();
        assert!(toml_str.contains("commands = [\n  \"pytest\",\n"), "{toml_str}");
        assert!(toml_str.contains("[verify.default]"), "{toml_str}");
    }

    #[test]
    fn pytest_ini_alone_selects_pytest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pytest.ini"), b"[pytest]\n").unwrap();
        assert_eq!(default_step_for(dir.path()), Some(&["pytest"][..]));
        let verify_path = init_verify(dir.path(), false).unwrap();
        let toml_str = fs::read_to_string(verify_path.join("verify.toml")).unwrap();
        assert!(toml_str.contains("commands = [\n  \"pytest\",\n"), "{toml_str}");
        assert!(toml_str.contains("[verify.default]"), "{toml_str}");
    }

    #[test]
    fn the_artifacts_root_is_an_absolute_path_named_artifacts() {
        let dir = artifacts_dir();
        assert!(dir.is_absolute(), "{dir:?}");
        assert_eq!(dir.file_name().and_then(|name| name.to_str()), Some("artifacts"));
    }

    #[test]
    fn ignored_dirs_do_not_move_the_code_tree_hash() {
        let dir = tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q"]);
        fs::write(dir.path().join("a.rs"), b"fn a() {}\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "base"]);
        let base = code_tree_hash(dir.path()).unwrap();

        fs::create_dir_all(dir.path().join(".verify/knowledge-base")).unwrap();
        fs::write(dir.path().join(".verify/knowledge-base/data.json"), b"{}").unwrap();
        assert_eq!(code_tree_hash(dir.path()).as_deref(), Some(base.as_str()));

        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::write(dir.path().join(".claude/settings.json"), b"{}").unwrap();
        assert_eq!(code_tree_hash(dir.path()).as_deref(), Some(base.as_str()));

        fs::write(dir.path().join("b.rs"), b"fn b() {}\n").unwrap();
        assert_ne!(code_tree_hash(dir.path()).as_deref(), Some(base.as_str()));
    }

    #[test]
    fn git_head_sha_matches_rev_parse_head() {
        let dir = tempdir().unwrap();
        assert_eq!(git_head_sha(dir.path()), None);
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "-q"]);
        fs::write(dir.path().join("a.rs"), b"fn a() {}\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "base"]);
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let expected = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert!(!expected.is_empty());
        assert_eq!(git_head_sha(dir.path()).as_deref(), Some(expected.as_str()));
    }
}
