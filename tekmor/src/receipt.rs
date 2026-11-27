use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use regex::Regex;
use serde::Serialize;

use tekmor_core::evidence::{flaky_commands, EvidenceRecord};

use crate::db::store::BrainStore;
use crate::evidence_reader;
use crate::project_key;
use crate::spec::{self, Requirement};
use crate::verify;

const RECEIPT_SCAN_LIMIT: usize = 200;
const SHORT_SHA_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Passed,
    Failed,
    NotVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub id: String,
    pub cmd: String,
    pub exit_code: i32,
    pub recorded_at: i64,
    pub source: String,
    pub artifact: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementState {
    Proven,
    Failed,
    NotRun,
    NoCheck,
    NotAllowlisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequirementStatus {
    pub id: String,
    pub text: String,
    pub state: RequirementState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Receipt {
    pub verdict: Verdict,
    pub head_sha: Option<String>,
    pub code_hash: Option<String>,
    pub evidence: Option<Evidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<RequirementStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_changes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flaky: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_gap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_gap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_gap: Option<String>,
}

fn suite_gap(rows: &[EvidenceRecord], head: &str, hash: &str, default_cmd: Option<&str>) -> Option<(Verdict, String)> {
    let flaky = flaky_commands(rows, head, hash);
    let mut failed: Vec<&str> = rows
        .iter()
        .filter(|row| row.is_run_for(head, hash) && row.exit_code != 0 && !flaky.contains(&row.cmd))
        .map(|row| row.cmd.as_str())
        .collect();
    failed.sort_unstable();
    failed.dedup();
    if !failed.is_empty() {
        return Some((Verdict::Failed, format!("failed on this tree: {}", failed.join(", "))));
    }
    let cmd = default_cmd?;
    let ran = rows.iter().any(|row| row.is_run_for(head, hash) && row.cmd == cmd && !flaky.contains(&row.cmd));
    (!ran).then(|| {
        (
            Verdict::NotVerified,
            format!("the default suite `{cmd}` has not run on this tree; run `tekmor verify -- {cmd}`"),
        )
    })
}

pub fn build_receipt(
    rows: &[EvidenceRecord],
    head_sha: Option<&str>,
    code_hash: Option<&str>,
) -> Receipt {
    let flaky = head_sha.zip(code_hash).map(|(head, hash)| flaky_commands(rows, head, hash)).unwrap_or_default();
    let evidence = head_sha.zip(code_hash).and_then(|(head, hash)| {
        rows.iter()
            .find(|row| row.is_run_for(head, hash) && !flaky.contains(&row.cmd))
            .map(|row| Evidence {
                id: row.id.clone(),
                cmd: row.cmd.clone(),
                exit_code: row.exit_code,
                recorded_at: row.recorded_at,
                source: row.source.clone().unwrap_or_else(|| "cli".to_string()),
                artifact: None,
            })
    });
    let verdict = match &evidence {
        None => Verdict::NotVerified,
        Some(found) if found.exit_code == 0 => Verdict::Passed,
        Some(_) => Verdict::Failed,
    };
    Receipt {
        verdict,
        head_sha: head_sha.map(str::to_string),
        code_hash: code_hash.map(str::to_string),
        evidence,
        requirements: Vec::new(),
        protected_changes: Vec::new(),
        flaky,
        review_gap: None,
        test_gap: None,
        suite_gap: None,
    }
}

fn latest_run<'a>(
    rows: &'a [EvidenceRecord],
    check: &str,
    head_sha: Option<&str>,
    code_hash: Option<&str>,
) -> Option<&'a EvidenceRecord> {
    let (head, hash) = head_sha.zip(code_hash)?;
    rows.iter()
        .find(|row| row.cmd == verify::AllowEntry::parse(check).base() && row.is_run_for(head, hash) && !flaky_commands(rows, head, hash).contains(&row.cmd))
}

pub fn requirement_states(
    specs: &[(String, Vec<Requirement>)],
    rows: &[EvidenceRecord],
    head_sha: Option<&str>,
    code_hash: Option<&str>,
    allowlisted: impl Fn(&str) -> bool,
) -> Vec<RequirementStatus> {
    let allowlisted = &allowlisted;
    specs
        .iter()
        .flat_map(|(slug, requirements)| {
            requirements.iter().map(move |requirement| {
                let state = match &requirement.check {
                    None => RequirementState::NoCheck,
                    Some(check) if !allowlisted(check) => RequirementState::NotAllowlisted,
                    Some(check) => match latest_run(rows, check, head_sha, code_hash) {
                        None => RequirementState::NotRun,
                        Some(run) if run.exit_code == 0 => RequirementState::Proven,
                        Some(_) => RequirementState::Failed,
                    },
                };
                RequirementStatus {
                    id: format!("{slug}/{}", requirement.id),
                    text: requirement.text.clone(),
                    state,
                }
            })
        })
        .collect()
}

fn protected_changes(root: &Path) -> Vec<String> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "status",
            "--porcelain",
            "--",
            ".verify/verify.toml",
            ".verify/run.sh",
            ".verify/checks",
        ])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| !line.starts_with("??"))
                .filter_map(|line| line.get(3..))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn load_specs(root: &Path) -> Vec<(String, Vec<Requirement>)> {
    let Ok(entries) = std::fs::read_dir(root.join(".verify/specs")) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let slug = path.file_stem()?.to_string_lossy().to_string();
            let text = std::fs::read_to_string(&path).ok()?;
            Some((slug, spec::parse(&text)))
        })
        .collect()
}

fn test_definition() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"#\[(tokio::)?test\]|^\s*(async\s+)?def\s+test_\w*|^\s*func\s+Test\w*\s*\(|\b(it|test)(\.skip)?\s*\(\s*['"`]|\bxit\s*\(\s*['"`]|@Test\b"#)
            .expect("static regex")
    })
}

fn test_skip() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"#\[ignore\b|@pytest\.mark\.(skip|xfail)\b|@unittest\.skip|\b(it|test|describe)\.skip\s*\(|\bx(it|describe)\s*\(|\bt\.Skip(Now|f)?\s*\(|@Disabled\b"#)
            .expect("static regex")
    })
}

struct ChangedLine {
    path: String,
    added: bool,
    text: String,
}

fn git_text(root: &Path, args: &[&str]) -> String {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

fn changed_lines(root: &Path) -> Vec<ChangedLine> {
    let mut lines = Vec::new();
    let mut file: Option<String> = None;
    for line in git_text(root, &["diff", "HEAD", "--unified=0", "--no-color", "--no-ext-diff"]).lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            file = header.rsplit_once(" b/").map(|(_, path)| path.to_string());
            continue;
        }
        if line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        let Some(path) = file.as_ref().filter(|path| verify::is_code_path(path)) else {
            continue;
        };
        let added = line.starts_with('+');
        if added || line.starts_with('-') {
            lines.push(ChangedLine { path: path.clone(), added, text: line[1..].to_string() });
        }
    }
    for path in git_text(root, &["ls-files", "--others", "--exclude-standard"])
        .lines()
        .filter(|path| verify::is_code_path(path))
    {
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        lines.extend(text.lines().map(|line| ChangedLine { path: path.to_string(), added: true, text: line.to_string() }));
    }
    lines
}

fn tests_text(count: usize) -> String {
    if count == 1 {
        "1 test".to_string()
    } else {
        format!("{count} tests")
    }
}

fn test_gap(root: &Path) -> Option<String> {
    let lines = changed_lines(root);
    let count = |added: bool, re: &Regex| lines.iter().filter(|line| line.added == added && re.is_match(&line.text)).count();
    let removed = count(false, test_definition()).saturating_sub(count(true, test_definition()));
    let disabled = count(true, test_skip()).saturating_sub(count(false, test_skip()));
    if removed == 0 && disabled == 0 {
        return None;
    }
    let mut files: Vec<&str> = lines
        .iter()
        .filter(|line| if line.added { test_skip().is_match(&line.text) } else { test_definition().is_match(&line.text) })
        .map(|line| line.path.as_str())
        .collect();
    files.sort();
    files.dedup();
    let mut parts = Vec::new();
    if removed > 0 {
        parts.push(format!("removes {}", tests_text(removed)));
    }
    if disabled > 0 {
        parts.push(format!("disables {}", tests_text(disabled)));
    }
    Some(format!(
        "the change {} compared to HEAD ({}); record an independent pass review to accept it",
        parts.join(" and "),
        files.join(", ")
    ))
}

pub fn receipt_for_project(store: &BrainStore, root: &Path) -> Result<Receipt> {
    let keys = crate::review::default_store(&crate::config::Config::load()?.home);
    receipt_with_keys(store, root, &keys)
}

pub fn receipt_with_keys(store: &BrainStore, root: &Path, keys: &crate::review::KeyStore) -> Result<Receipt> {
    let head = verify::git_head_sha(root);
    let code = verify::code_tree_hash(root);
    let rows = match project_key::project_key_if_known(root) {
        Some(key) => evidence_reader::project_runs(store, &key, RECEIPT_SCAN_LIMIT)?,
        None => Vec::new(),
    };
    let mut receipt = build_receipt(&rows, head.as_deref(), code.as_deref());
    let default_cmd = verify::load_default_step(root)?.map(|step| verify::render_argv(&step.command));
    if let (Some(head), Some(hash)) = (head.as_deref(), code.as_deref()) {
        if let Some((verdict, gap)) = suite_gap(&rows, head, hash, default_cmd.as_deref()) {
            receipt.verdict = verdict;
            receipt.suite_gap = Some(gap);
        }
    }
    if let Some(gap) = crate::review::gap(root, keys, code.as_deref()) {
        receipt.verdict = Verdict::NotVerified;
        receipt.review_gap = Some(gap);
    }
    let unreviewed = receipt.verdict == Verdict::Passed && !crate::review::passed(root, keys, code.as_deref());
    if let Some(gap) = unreviewed.then(|| test_gap(root)).flatten() {
        receipt.verdict = Verdict::NotVerified;
        receipt.test_gap = Some(gap);
    }
    receipt.requirements = requirement_states(
        &load_specs(root),
        &rows,
        head.as_deref(),
        code.as_deref(),
        |cmd| verify::command_text_is_allowlisted(cmd, Some(root)),
    );
    receipt.protected_changes = protected_changes(root);
    if receipt.verdict == Verdict::Passed && !receipt.protected_changes.is_empty() && !crate::review::passed(root, keys, code.as_deref()) {
        receipt.verdict = Verdict::NotVerified;
        receipt.suite_gap = Some(format!(
            "checks changed since last commit ({}); a pass review for this tree is needed",
            receipt.protected_changes.join(", ")
        ));
    }
    if let Some(evidence) = receipt.evidence.as_mut() {
        let artifact = verify::repo_artifact_path(root, &evidence.id);
        if artifact.is_file() {
            evidence.artifact = Some(
                artifact
                    .strip_prefix(root)
                    .unwrap_or(&artifact)
                    .display()
                    .to_string(),
            );
        }
    }
    Ok(receipt)
}

fn short(value: &str) -> &str {
    value.get(..SHORT_SHA_LEN).unwrap_or(value)
}

fn recorded_at_text(millis: i64) -> String {
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|at| at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_else(|| millis.to_string())
}

fn headline(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Passed => "PASSED",
        Verdict::Failed => "FAILED",
        Verdict::NotVerified => "NOT VERIFIED",
    }
}

fn not_verified_hint(receipt: &Receipt) -> &'static str {
    if receipt.head_sha.is_none() {
        "no git commit found for this directory; run inside a git repository with at least one commit"
    } else {
        "no verify run recorded for this commit and code state; run `tekmor verify -- <cmd>`"
    }
}

pub fn render_text(receipt: &Receipt) -> String {
    let mut out = format!("Verify receipt: {}\n", headline(receipt.verdict));
    match &receipt.evidence {
        Some(evidence) => {
            out.push_str(&format!("  command:   {}\n", evidence.cmd));
            out.push_str(&format!("  exit code: {}\n", evidence.exit_code));
            out.push_str(&format!(
                "  recorded:  {}\n",
                recorded_at_text(evidence.recorded_at)
            ));
            out.push_str(&format!("  source:    {}\n", evidence.source));
            if let Some(artifact) = &evidence.artifact {
                out.push_str(&format!("  artifact:  {artifact}\n"));
            }
        }
        None => out.push_str(&format!("  {}\n", not_verified_hint(receipt))),
    }
    out.push_str(&requirements_text(&receipt.requirements));
    if let Some(gap) = &receipt.review_gap {
        out.push_str(&format!("  review:    {gap}\n"));
    }
    if let Some(gap) = &receipt.test_gap {
        out.push_str(&format!("  tests:     {gap}\n"));
    }
    if let Some(gap) = &receipt.suite_gap {
        out.push_str(&format!("  suite:     {gap}\n"));
    }
    if !receipt.protected_changes.is_empty() {
        out.push_str(&format!(
            "  checks changed since last commit: {}\n",
            receipt.protected_changes.join(", ")
        ));
    }
    if let Some(head) = &receipt.head_sha {
        out.push_str(&format!("  commit:    {}\n", short(head)));
    }
    if let Some(hash) = &receipt.code_hash {
        out.push_str(&format!("  code hash: {}\n", short(hash)));
    }
    out
}

fn requirement_label(state: RequirementState) -> Option<&'static str> {
    match state {
        RequirementState::Proven => None,
        RequirementState::Failed => Some("failed"),
        RequirementState::NotRun => Some("not run"),
        RequirementState::NoCheck => Some("no check yet"),
        RequirementState::NotAllowlisted => Some("check is not in the allowlist"),
    }
}

fn requirement_count(requirements: &[RequirementStatus]) -> (usize, usize) {
    let proven = requirements
        .iter()
        .filter(|item| item.state == RequirementState::Proven)
        .count();
    (proven, requirements.len())
}

fn requirements_text(requirements: &[RequirementStatus]) -> String {
    if requirements.is_empty() {
        return String::new();
    }
    let (proven, total) = requirement_count(requirements);
    let mut out = format!("  requirements: {proven} of {total} proven\n");
    for item in requirements {
        if let Some(label) = requirement_label(item.state) {
            out.push_str(&format!("    {}: {label} - {}\n", item.id, item.text));
        }
    }
    out
}

fn requirements_markdown(requirements: &[RequirementStatus]) -> String {
    if requirements.is_empty() {
        return String::new();
    }
    let (proven, total) = requirement_count(requirements);
    let mut out = format!("\n**Requirements: {proven} of {total} proven**\n\n");
    for item in requirements {
        if let Some(label) = requirement_label(item.state) {
            out.push_str(&format!(
                "- `{}`: {label} - {}\n",
                item.id,
                markdown_cell(&item.text)
            ));
        }
    }
    out
}

fn markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('`', "'")
}

pub fn render_markdown(receipt: &Receipt) -> String {
    let mut out = format!("**Verify receipt: {}**\n\n", headline(receipt.verdict));
    match &receipt.evidence {
        Some(evidence) => {
            out.push_str("| Field | Value |\n|---|---|\n");
            out.push_str(&format!(
                "| Command | `{}` |\n",
                markdown_cell(&evidence.cmd)
            ));
            out.push_str(&format!("| Exit code | {} |\n", evidence.exit_code));
            out.push_str(&format!(
                "| Recorded | {} |\n",
                recorded_at_text(evidence.recorded_at)
            ));
            out.push_str(&format!("| Source | {} |\n", evidence.source));
            if let Some(artifact) = &evidence.artifact {
                out.push_str(&format!(
                    "| Proof artifact | `{}` |\n",
                    markdown_cell(artifact)
                ));
            }
            if let Some(head) = &receipt.head_sha {
                out.push_str(&format!("| Commit | `{}` |\n", short(head)));
            }
            if let Some(hash) = &receipt.code_hash {
                out.push_str(&format!("| Code hash | `{}` |\n", short(hash)));
            }
        }
        None => out.push_str(&format!("{}.\n", not_verified_hint(receipt))),
    }
    out.push_str(&requirements_markdown(&receipt.requirements));
    if let Some(gap) = &receipt.review_gap {
        out.push_str(&format!("\n**Review:** {gap}\n"));
    }
    if let Some(gap) = &receipt.test_gap {
        out.push_str(&format!("\n**Tests:** {gap}\n"));
    }
    if let Some(gap) = &receipt.suite_gap {
        out.push_str(&format!("\n**Suite:** {gap}\n"));
    }
    if !receipt.protected_changes.is_empty() {
        out.push_str(&format!(
            "\n**Checks changed since last commit:** {}\n",
            receipt.protected_changes.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b";
    const HASH: &str = "9f8e7d6c5b4a3921fedcba9876543210ffeeddcc";

    fn row(
        cmd: &str,
        exit: i32,
        head: Option<&str>,
        hash: Option<&str>,
        source: Option<&str>,
    ) -> EvidenceRecord {
        EvidenceRecord {
            id: format!("id-{cmd}-{exit}"),
            cmd: cmd.to_string(),
            exit_code: exit,
            recorded_at: 1_794_000_000_000,
            source: source.map(str::to_string),
            code_hash: hash.map(str::to_string),
            head_sha: head.map(str::to_string),
            verify_config_hash: None,
        }
    }

    #[test]
    fn passing_run_for_this_commit_and_code_state_is_passed() {
        let rows = [row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli"))];
        let receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        assert_eq!(receipt.verdict, Verdict::Passed);
        assert_eq!(
            receipt.evidence.as_ref().map(|e| e.cmd.as_str()),
            Some("cargo test")
        );
    }

    #[test]
    fn failing_run_for_this_commit_is_failed_with_its_exit_code() {
        let rows = [row("cargo test", 101, Some(HEAD), Some(HASH), Some("cli"))];
        let receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        assert_eq!(receipt.verdict, Verdict::Failed);
        assert_eq!(receipt.evidence.map(|e| e.exit_code), Some(101));
    }

    #[test]
    fn mixed_outcomes_on_one_tree_are_flaky_and_not_a_verdict() {
        let rows = [
            row("cargo test", 1, Some(HEAD), Some(HASH), Some("cli")),
            row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli")),
        ];
        assert_eq!(
            build_receipt(&rows, Some(HEAD), Some(HASH)).verdict,
            Verdict::NotVerified
        );
    }

    #[test]
    fn run_for_another_commit_does_not_count() {
        let other = "ffffffffffffffffffffffffffffffffffffffff";
        let rows = [row("cargo test", 0, Some(other), Some(HASH), Some("cli"))];
        let receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        assert_eq!(receipt.verdict, Verdict::NotVerified);
        assert!(receipt.evidence.is_none());
    }

    #[test]
    fn run_for_an_earlier_code_state_on_the_same_commit_does_not_count() {
        let rows = [row("cargo test", 0, Some(HEAD), Some("0000"), Some("cli"))];
        assert_eq!(
            build_receipt(&rows, Some(HEAD), Some(HASH)).verdict,
            Verdict::NotVerified
        );
    }

    #[test]
    fn rows_recorded_before_commit_tracking_never_count() {
        let rows = [row("cargo test", 0, None, Some(HASH), Some("cli"))];
        assert_eq!(
            build_receipt(&rows, Some(HEAD), Some(HASH)).verdict,
            Verdict::NotVerified
        );
    }

    #[test]
    fn mixed_outcomes_for_one_command_on_the_same_tree_are_not_proof() {
        let rows = [
            row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli")),
            row("cargo test", 1, Some(HEAD), Some(HASH), Some("cli")),
        ];
        let receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        assert_eq!(receipt.verdict, Verdict::NotVerified);
        assert_eq!(receipt.flaky, ["cargo test"]);
    }

    #[test]
    fn hook_records_do_not_count_as_a_verify_run() {
        let rows = [
            row("cargo test", 0, Some(HEAD), Some(HASH), Some("hook")),
            row("cargo test", 0, Some(HEAD), Some(HASH), Some("hook")),
        ];
        assert_eq!(
            build_receipt(&rows, Some(HEAD), Some(HASH)).verdict,
            Verdict::NotVerified
        );
    }

    #[test]
    fn missing_git_state_is_not_verified() {
        let rows = [row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli"))];
        assert_eq!(
            build_receipt(&rows, None, Some(HASH)).verdict,
            Verdict::NotVerified
        );
        assert_eq!(
            build_receipt(&rows, Some(HEAD), None).verdict,
            Verdict::NotVerified
        );
    }

    #[test]
    fn text_shows_verdict_command_exit_and_short_commit() {
        let rows = [row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli"))];
        let text = render_text(&build_receipt(&rows, Some(HEAD), Some(HASH)));
        assert!(text.starts_with("Verify receipt: PASSED"));
        assert!(text.contains("command:   cargo test"));
        assert!(text.contains("exit code: 0"));
        assert!(text.contains("commit:    1a2b3c4d5e6f"));
        assert!(!text.contains(HEAD));
    }

    #[test]
    fn not_verified_text_tells_the_user_what_to_run() {
        let text = render_text(&build_receipt(&[], Some(HEAD), Some(HASH)));
        assert!(text.starts_with("Verify receipt: NOT VERIFIED"));
        assert!(text.contains("tekmor verify -- <cmd>"));
    }

    #[test]
    fn not_a_git_repo_explains_why() {
        let text = render_text(&build_receipt(&[], None, None));
        assert!(text.contains("no git commit found"));
    }

    #[test]
    fn markdown_escapes_pipes_in_the_command() {
        let rows = [row(
            "cargo test -- a|b",
            0,
            Some(HEAD),
            Some(HASH),
            Some("cli"),
        )];
        let markdown = render_markdown(&build_receipt(&rows, Some(HEAD), Some(HASH)));
        assert!(markdown.contains("| Command | `cargo test -- a\\|b` |"));
        assert!(markdown.contains("| Commit | `1a2b3c4d5e6f` |"));
    }

    #[test]
    fn text_and_markdown_show_the_proof_artifact_when_present() {
        let rows = [row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli"))];
        let mut receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        receipt.evidence.as_mut().unwrap().artifact =
            Some(".verify/artifacts/run-1.log".to_string());
        assert!(render_text(&receipt).contains("artifact:  .verify/artifacts/run-1.log"));
        assert!(render_markdown(&receipt)
            .contains("| Proof artifact | `.verify/artifacts/run-1.log` |"));
    }

    #[test]
    fn no_artifact_line_when_none_was_saved() {
        let rows = [row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli"))];
        let receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        assert!(!render_text(&receipt).contains("artifact:"));
        assert!(!render_markdown(&receipt).contains("Proof artifact"));
    }

    #[test]
    fn json_uses_snake_case_verdict() {
        let receipt = build_receipt(&[], Some(HEAD), Some(HASH));
        let json = serde_json::to_value(&receipt).expect("serialize");
        assert_eq!(json["verdict"], "not_verified");
        assert!(json["evidence"].is_null());
    }

    fn spec(slug: &str, text: &str) -> (String, Vec<Requirement>) {
        (slug.to_string(), spec::parse(text))
    }

    fn states(
        specs: &[(String, Vec<Requirement>)],
        rows: &[EvidenceRecord],
        allowed: &[&str],
    ) -> Vec<RequirementState> {
        requirement_states(specs, rows, Some(HEAD), Some(HASH), |cmd| {
            allowed.contains(&cmd)
        })
        .into_iter()
        .map(|item| item.state)
        .collect()
    }

    #[test]
    fn a_requirement_is_proven_by_a_passing_run_for_this_tree() {
        let specs = [spec(
            "login",
            "R1. Users log in. check: `cargo test login`\n",
        )];
        let rows = [row(
            "cargo test login",
            0,
            Some(HEAD),
            Some(HASH),
            Some("cli"),
        )];
        assert_eq!(
            states(&specs, &rows, &["cargo test login"]),
            [RequirementState::Proven]
        );
    }

    #[test]
    fn a_shell_check_is_proven_by_its_sh_c_run() {
        let specs = [spec(
            "env",
            "R1. Needs env. check: `FOO=bar sh check.sh`\n",
        )];
        let rows = [row(
            "sh -c 'FOO=bar sh check.sh'",
            0,
            Some(HEAD),
            Some(HASH),
            Some("cli"),
        )];
        assert_eq!(
            states(&specs, &rows, &["FOO=bar sh check.sh"]),
            [RequirementState::Proven]
        );
    }

    #[test]
    fn mixed_outcomes_for_a_requirement_leave_it_not_run() {
        let specs = [spec(
            "login",
            "R1. Users log in. check: `cargo test login`\n",
        )];
        let rows = [
            row("cargo test login", 1, Some(HEAD), Some(HASH), Some("cli")),
            row("cargo test login", 0, Some(HEAD), Some(HASH), Some("cli")),
        ];
        assert_eq!(
            states(&specs, &rows, &["cargo test login"]),
            [RequirementState::NotRun]
        );
    }

    #[test]
    fn runs_for_another_tree_or_from_hooks_do_not_prove_a_requirement() {
        let specs = [spec(
            "login",
            "R1. Users log in. check: `cargo test login`\n",
        )];
        let rows = [
            row(
                "cargo test login",
                0,
                Some("other"),
                Some(HASH),
                Some("cli"),
            ),
            row("cargo test login", 0, Some(HEAD), Some(HASH), Some("hook")),
        ];
        assert_eq!(
            states(&specs, &rows, &["cargo test login"]),
            [RequirementState::NotRun]
        );
    }

    #[test]
    fn a_requirement_with_no_check_or_an_unlisted_check_says_so() {
        let specs = [spec(
            "login",
            "R1. Looks nice.\nR2. Works. check: `rm -rf build`\n",
        )];
        assert_eq!(
            states(&specs, &[], &[]),
            [RequirementState::NoCheck, RequirementState::NotAllowlisted]
        );
    }

    #[test]
    fn requirement_ids_are_namespaced_by_spec_file() {
        let specs = [spec("login", "R1. A.\n"), spec("cart", "R1. B.\n")];
        let ids: Vec<String> = requirement_states(&specs, &[], None, None, |_| true)
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(ids, ["login/R1", "cart/R1"]);
    }

    #[test]
    fn text_lists_the_count_and_each_unproven_requirement() {
        let mut receipt = build_receipt(&[], Some(HEAD), Some(HASH));
        receipt.requirements = requirement_states(
            &[spec("login", "R1. A.\nR2. B. check: `cargo test b`\n")],
            &[row("cargo test b", 0, Some(HEAD), Some(HASH), Some("cli"))],
            Some(HEAD),
            Some(HASH),
            |_| true,
        );
        let text = render_text(&receipt);
        assert!(text.contains("  requirements: 1 of 2 proven\n"));
        assert!(text.contains("    login/R1: no check yet - A.\n"));
        assert!(!text.contains("login/R2"));
        let markdown = render_markdown(&receipt);
        assert!(markdown.contains("**Requirements: 1 of 2 proven**"));
    }

    #[test]
    fn a_project_without_specs_renders_no_requirement_lines() {
        let receipt = build_receipt(&[], Some(HEAD), Some(HASH));
        assert!(!render_text(&receipt).contains("requirements"));
        assert!(!render_markdown(&receipt).contains("Requirements"));
        let json = serde_json::to_value(&receipt).expect("serialize");
        assert!(json.get("requirements").is_none());
    }

    #[test]
    fn a_check_file_changed_since_the_last_commit_is_listed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["-c", "user.email=t@t", "-c", "user.name=t"])
                .args(args)
                .status()
                .unwrap();
        };
        git(&["init", "-q"]);
        std::fs::create_dir_all(root.join(".verify")).unwrap();
        std::fs::write(root.join(".verify/verify.toml"), "[verify]\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        assert!(protected_changes(root).is_empty());

        std::fs::write(
            root.join(".verify/verify.toml"),
            "[verify]\ncommands = []\n",
        )
        .unwrap();
        assert_eq!(protected_changes(root), [".verify/verify.toml"]);
    }

    fn git(root: &Path, args: &[&str]) -> Result<()> {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .status()?;
        anyhow::ensure!(status.success(), "git {args:?} failed");
        Ok(())
    }

    fn put(root: &Path, path: &str, text: &str) -> Result<()> {
        let file = root.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, text)?;
        Ok(())
    }

    fn committed(files: &[(&str, &str)]) -> Result<tempfile::TempDir> {
        let dir = tempfile::TempDir::new()?;
        git(dir.path(), &["init", "-q"])?;
        for (path, text) in files {
            put(dir.path(), path, text)?;
        }
        git(dir.path(), &["add", "-A"])?;
        git(dir.path(), &["commit", "-qm", "base"])?;
        Ok(dir)
    }

    const RUST_TEST: &str = "fn a() {}\n\n#[test]\nfn checks_a() {}\n";

    #[test]
    fn deleting_a_rust_test_is_a_test_gap() -> Result<()> {
        let dir = committed(&[("src/lib.rs", RUST_TEST)])?;
        put(dir.path(), "src/lib.rs", "fn a() {}\n")?;
        let gap = test_gap(dir.path()).ok_or_else(|| anyhow::anyhow!("expected a gap"))?;
        assert!(gap.contains("removes 1 test") && gap.contains("src/lib.rs"), "{gap}");
        Ok(())
    }

    #[test]
    fn ignoring_a_rust_test_is_a_test_gap() -> Result<()> {
        let dir = committed(&[("src/lib.rs", RUST_TEST)])?;
        put(dir.path(), "src/lib.rs", "fn a() {}\n\n#[test]\n#[ignore]\nfn checks_a() {}\n")?;
        let gap = test_gap(dir.path()).ok_or_else(|| anyhow::anyhow!("expected a gap"))?;
        assert!(gap.contains("disables 1 test"), "{gap}");
        Ok(())
    }

    #[test]
    fn adding_a_test_is_not_a_test_gap() -> Result<()> {
        let dir = committed(&[("src/lib.rs", RUST_TEST)])?;
        put(dir.path(), "src/lib.rs", &format!("{RUST_TEST}\n#[test]\nfn checks_b() {{}}\n"))?;
        assert_eq!(test_gap(dir.path()), None);
        Ok(())
    }

    #[test]
    fn moving_a_test_into_a_new_file_is_not_a_test_gap() -> Result<()> {
        let dir = committed(&[("src/lib.rs", RUST_TEST)])?;
        put(dir.path(), "src/lib.rs", "fn a() {}\n")?;
        put(dir.path(), "src/moved.rs", "#[test]\nfn checks_a() {}\n")?;
        assert_eq!(test_gap(dir.path()), None);
        Ok(())
    }

    #[test]
    fn test_text_in_docs_is_not_a_test_gap() -> Result<()> {
        let dir = committed(&[("docs/guide.md", "#[test]\nfn example() {}\n")])?;
        put(dir.path(), "docs/guide.md", "nothing\n")?;
        assert_eq!(test_gap(dir.path()), None);
        Ok(())
    }

    #[test]
    fn python_and_js_tests_are_guarded() -> Result<()> {
        let dir = committed(&[
            ("tests/test_x.py", "def test_x():\n    assert True\n"),
            ("src/a.test.js", "it('works', () => {});\n"),
        ])?;
        put(dir.path(), "tests/test_x.py", "\n")?;
        put(dir.path(), "src/a.test.js", "it.skip('works', () => {});\n")?;
        let gap = test_gap(dir.path()).ok_or_else(|| anyhow::anyhow!("expected a gap"))?;
        assert!(gap.contains("removes 1 test") && gap.contains("disables 1 test"), "{gap}");
        Ok(())
    }

    #[test]
    fn a_passing_run_that_removes_a_test_is_not_verified_until_reviewed() -> Result<()> {
        let dir = committed(&[
            ("src/lib.rs", RUST_TEST),
            (".verify/verify.toml", "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n"),
        ])?;
        let aux = tempfile::TempDir::new()?;
        let store = BrainStore::open(&aux.path().join("t.db"))?;
        let keys = crate::review::KeyStore::File(aux.path().join("review.key"));
        put(dir.path(), "src/lib.rs", "fn a() {}\n")?;
        verify::run_default_step(&store, dir.path(), verify::VerifySource::Cli)?;
        let receipt = receipt_with_keys(&store, dir.path(), &keys)?;
        assert_eq!(receipt.verdict, Verdict::NotVerified);
        assert!(receipt.test_gap.is_some());
        assert!(render_text(&receipt).contains("  tests:     the change removes 1 test"));
        assert!(render_markdown(&receipt).contains("**Tests:**"));
        let hash = receipt.code_hash.clone().ok_or_else(|| anyhow::anyhow!("no code hash"))?;
        crate::review::record(dir.path(), &keys, &hash, "pass", "reviewer", "tests moved")?;
        let reviewed = receipt_with_keys(&store, dir.path(), &keys)?;
        assert_eq!(reviewed.verdict, Verdict::Passed);
        assert_eq!(reviewed.test_gap, None);
        Ok(())
    }

    #[test]
    fn a_passing_command_cannot_stand_in_for_a_configured_suite() {
        let rows = [row("true", 0, Some(HEAD), Some(HASH), Some("cli"))];
        let (verdict, gap) = suite_gap(&rows, HEAD, HASH, Some("sh .verify/run.sh")).expect("a gap");
        assert_eq!(verdict, Verdict::NotVerified);
        assert!(gap.contains("sh .verify/run.sh"), "{gap}");
    }

    #[test]
    fn a_later_pass_does_not_hide_a_failing_run_on_the_same_tree() {
        let rows = [
            row("true", 0, Some(HEAD), Some(HASH), Some("cli")),
            row("sh .verify/full.sh", 1, Some(HEAD), Some(HASH), Some("cli")),
        ];
        let (verdict, gap) = suite_gap(&rows, HEAD, HASH, None).expect("a gap");
        assert_eq!(verdict, Verdict::Failed);
        assert!(gap.contains("sh .verify/full.sh"), "{gap}");
    }

    #[test]
    fn a_failure_that_was_rerun_to_pass_is_flaky_not_failed() {
        let rows = [
            row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli")),
            row("cargo test", 1, Some(HEAD), Some(HASH), Some("cli")),
        ];
        assert_eq!(suite_gap(&rows, HEAD, HASH, None), None);
    }

    #[test]
    fn a_configured_suite_that_passed_on_this_tree_is_no_gap() {
        let rows = [row("sh .verify/run.sh", 0, Some(HEAD), Some(HASH), Some("cli"))];
        assert_eq!(suite_gap(&rows, HEAD, HASH, Some("sh .verify/run.sh")), None);
    }

    #[test]
    fn a_suite_run_on_another_tree_does_not_count() {
        let rows = [row("sh .verify/run.sh", 0, Some(HEAD), Some("other"), Some("cli"))];
        assert!(suite_gap(&rows, HEAD, HASH, Some("sh .verify/run.sh")).is_some());
    }

    #[test]
    fn a_failing_run_for_this_tree_marks_the_requirement_failed() {
        let specs = [spec(
            "login",
            "R1. Users log in. check: `cargo test login`\n",
        )];
        let rows = [row(
            "cargo test login",
            1,
            Some(HEAD),
            Some(HASH),
            Some("cli"),
        )];
        assert_eq!(
            states(&specs, &rows, &["cargo test login"]),
            [RequirementState::Failed]
        );
    }

    #[test]
    fn spec_files_on_disk_feed_the_receipt_and_only_markdown_counts() -> Result<()> {
        let dir = committed(&[
            ("src/lib.rs", "fn a() {}\n"),
            (".verify/verify.toml", "[verify]\ncommands = [\"sh -c ...\"]\n"),
            (
                ".verify/specs/login.md",
                "R1. Users log in. check: `sh -c true`\nR2. Passwords hash.\n",
            ),
            (".verify/specs/notes.txt", "R1. Not a spec. check: `sh -c true`\n"),
        ])?;
        let aux = tempfile::TempDir::new()?;
        let store = BrainStore::open(&aux.path().join("t.db"))?;
        let keys = crate::review::KeyStore::File(aux.path().join("review.key"));
        let receipt = receipt_with_keys(&store, dir.path(), &keys)?;
        let ids: Vec<&str> = receipt
            .requirements
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, ["login/R1", "login/R2"]);
        assert!(render_text(&receipt).contains("requirements: 0 of 2 proven"));
        Ok(())
    }

    #[test]
    fn removing_two_tests_says_two_tests() -> Result<()> {
        let dir = committed(&[(
            "src/lib.rs",
            "fn a() {}\n\n#[test]\nfn checks_a() {}\n\n#[test]\nfn checks_b() {}\n",
        )])?;
        put(dir.path(), "src/lib.rs", "fn a() {}\n")?;
        let gap = test_gap(dir.path()).ok_or_else(|| anyhow::anyhow!("expected a gap"))?;
        assert_eq!(
            gap,
            "the change removes 2 tests compared to HEAD (src/lib.rs); record an independent pass review to accept it"
        );
        Ok(())
    }

    #[test]
    fn the_gap_names_only_the_kind_of_test_change_that_happened() -> Result<()> {
        let dir = committed(&[("src/lib.rs", RUST_TEST)])?;
        put(dir.path(), "src/lib.rs", "fn a() {}\n")?;
        assert_eq!(
            test_gap(dir.path()),
            Some(
                "the change removes 1 test compared to HEAD (src/lib.rs); record an independent pass review to accept it"
                    .to_string()
            )
        );
        git(dir.path(), &["add", "-A"])?;
        git(dir.path(), &["commit", "-qm", "clean"])?;
        put(
            dir.path(),
            "src/lib.rs",
            "fn a() {}\n\n#[test]\n#[ignore]\nfn checks_a() {}\n",
        )?;
        let gap = test_gap(dir.path()).ok_or_else(|| anyhow::anyhow!("expected a gap"))?;
        assert_eq!(
            gap,
            "the change disables 1 test compared to HEAD (src/lib.rs); record an independent pass review to accept it"
        );
        Ok(())
    }

    #[test]
    fn the_recorded_time_is_rendered_as_rfc3339() {
        assert_eq!(recorded_at_text(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            recorded_at_text(1_794_000_000_000),
            "2026-11-06T21:20:00Z"
        );
        let rows = [row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli"))];
        let receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        assert!(render_text(&receipt).contains("recorded:  2026-11-06T21:20:00Z"));
        assert!(render_markdown(&receipt).contains("| Recorded | 2026-11-06T21:20:00Z |"));
    }

    #[test]
    fn protected_changes_are_listed_when_present_and_hidden_when_absent() {
        let rows = [row("cargo test", 0, Some(HEAD), Some(HASH), Some("cli"))];
        let mut receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        receipt.protected_changes = vec![".verify/verify.toml".to_string()];
        assert!(
            render_text(&receipt)
                .contains("  checks changed since last commit: .verify/verify.toml\n")
        );
        assert!(
            render_markdown(&receipt)
                .contains("**Checks changed since last commit:** .verify/verify.toml\n")
        );
        receipt.protected_changes.clear();
        assert!(!render_text(&receipt).contains("checks changed"));
        assert!(!render_markdown(&receipt).contains("Checks changed"));
    }

    #[test]
    fn the_proven_count_counts_only_proven_requirements() {
        let specs = [spec(
            "login",
            "R1. A. check: `cargo test a`\nR2. B. check: `cargo test b`\nR3. C.\n",
        )];
        let rows = [
            row("cargo test a", 0, Some(HEAD), Some(HASH), Some("cli")),
            row("cargo test b", 0, Some(HEAD), Some(HASH), Some("cli")),
        ];
        let mut receipt = build_receipt(&rows, Some(HEAD), Some(HASH));
        receipt.requirements =
            requirement_states(&specs, &rows, Some(HEAD), Some(HASH), |_| true);
        assert!(render_text(&receipt).contains("  requirements: 2 of 3 proven\n"));
        assert!(render_markdown(&receipt).contains("**Requirements: 2 of 3 proven**"));
    }
}
