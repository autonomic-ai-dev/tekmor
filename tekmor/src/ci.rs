use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::db::store::BrainStore;
use crate::receipt::{self, Verdict};
use crate::review::KeyStore;
use crate::verify::{self, VerifySource};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CiVerdict {
    Passed,
    Failed,
    Held,
    NotVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CiReport {
    pub verdict: CiVerdict,
    pub head: String,
    pub merge_base: String,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub changed_checks: Vec<String>,
    pub gaps: Vec<String>,
    pub approved_by: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub failure_output: String,
}

fn git(root: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git").arg("-C").arg(root).args(args).status().context("run git")?;
    if !status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    verify::git_output(root, args).map(|out| String::from_utf8_lossy(&out).trim().to_string())
}

fn proof_files(root: &Path, rev: &str) -> Result<BTreeSet<String>> {
    let out = verify::git_output(root, &["ls-tree", "-r", "-z", "--name-only", rev, "--", ".verify"])
        .with_context(|| format!("list check files at {rev}"))?;
    Ok(verify::split_nul(&out).into_iter().filter(|path| verify::is_proof_input(path)).collect())
}

fn checkout(root: &Path, rev: &str, paths: &BTreeSet<String>) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["checkout", rev, "--"];
    args.extend(paths.iter().map(String::as_str));
    git(root, &args)
}

fn remove_all<'a>(root: &Path, paths: impl Iterator<Item = &'a String>) -> Result<()> {
    for path in paths {
        let full = root.join(path);
        if full.exists() {
            fs::remove_file(&full).with_context(|| format!("remove {}", full.display()))?;
        }
    }
    Ok(())
}

struct BaseChecks<'a> {
    root: &'a Path,
    head_files: BTreeSet<String>,
    base_files: BTreeSet<String>,
    previous_base: Option<String>,
    restored: bool,
}

impl<'a> BaseChecks<'a> {
    fn apply(root: &'a Path, merge_base: &str) -> Result<Self> {
        let head_files = proof_files(root, "HEAD")?;
        let base_files = proof_files(root, merge_base)?;
        let checks = Self {
            root,
            previous_base: std::env::var("TEKMOR_BASE").ok(),
            head_files,
            base_files,
            restored: false,
        };
        remove_all(root, checks.head_files.difference(&checks.base_files))?;
        checkout(root, merge_base, &checks.base_files)?;
        if !checks.base_files.is_empty() {
            let mut args = vec!["reset", "-q", "--"];
            args.extend(checks.base_files.iter().map(String::as_str));
            git(root, &args)?;
        }
        Ok(checks)
    }

    fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        match &self.previous_base {
            Some(value) => std::env::set_var("TEKMOR_BASE", value),
            None => std::env::remove_var("TEKMOR_BASE"),
        }
        remove_all(self.root, self.base_files.difference(&self.head_files))?;
        checkout(self.root, "HEAD", &self.head_files)
    }
}

impl Drop for BaseChecks<'_> {
    fn drop(&mut self) {
        if let Err(err) = self.restore() {
            eprintln!("tekmor ci could not restore the check files: {err:#}");
        }
    }
}

fn uncommitted(root: &Path) -> Result<Vec<String>> {
    let changes = verify::git_changes(root).context("tekmor ci must run inside a git repository")?;
    Ok(changes
        .into_iter()
        .filter(|path| verify::counts_toward_tree(path) || verify::is_proof_input(path))
        .collect())
}

pub fn run(store: &BrainStore, root: &Path, keys: &KeyStore, base_ref: &str, approved_by: Option<&str>) -> Result<CiReport> {
    let dirty = uncommitted(root)?;
    if !dirty.is_empty() {
        bail!("tekmor ci needs a clean checkout; uncommitted: {}", dirty.join(", "));
    }
    let head = verify::git_head_sha(root).context("tekmor ci needs a commit at HEAD")?;
    let merge_base = git_text(root, &["merge-base", base_ref, "HEAD"])
        .filter(|sha| !sha.is_empty())
        .with_context(|| format!("no merge-base between {base_ref} and HEAD; fetch the base branch with full history"))?;
    let changed_checks: Vec<String> = verify::git_output(root, &["diff", "--name-only", "-z", "--no-renames", &merge_base, "HEAD", "--", ".verify"])
        .map(|out| verify::split_nul(&out).into_iter().filter(|path| verify::is_proof_input(path)).collect())
        .unwrap_or_default();

    let mut base_checks = BaseChecks::apply(root, &merge_base)?;
    let run = verify::run_default_step(store, root, VerifySource::Ci)?;
    std::env::set_var("TEKMOR_BASE", &merge_base);
    let receipt = receipt::receipt_with_keys(store, root, keys)?;
    base_checks.restore()?;

    let mut gaps = Vec::new();
    if !changed_checks.is_empty() {
        gaps.push(format!(
            "this change edits check files ({}); the base branch's versions ran instead, and a maintainer must approve this head",
            changed_checks.join(", ")
        ));
    }
    let review_gaps: Vec<String> = [receipt.review_gap.clone(), receipt.test_gap.clone()].into_iter().flatten().collect();
    gaps.extend(review_gaps.iter().cloned());
    gaps.extend(receipt.suite_gap.clone());
    let needs_approval = !changed_checks.is_empty() || !review_gaps.is_empty();
    let suite_proven = receipt.suite_gap.is_none() && (receipt.verdict == Verdict::Passed || (receipt.verdict == Verdict::NotVerified && !review_gaps.is_empty()));
    let verdict = match &run {
        None => {
            gaps.push("the base branch declares no [verify.default] in .verify/verify.toml; tekmor ci runs only that suite".into());
            CiVerdict::NotVerified
        }
        Some(step) if !step.report.success => CiVerdict::Failed,
        Some(_) if receipt.verdict == Verdict::Failed => CiVerdict::Failed,
        Some(_) if !suite_proven => CiVerdict::NotVerified,
        Some(_) if needs_approval && approved_by.is_none() => CiVerdict::Held,
        Some(_) => CiVerdict::Passed,
    };
    Ok(CiReport {
        verdict,
        head,
        merge_base,
        command: run.as_ref().map(|step| step.report.cmd.clone()),
        exit_code: run.as_ref().map(|step| step.report.exit_code),
        changed_checks,
        gaps,
        approved_by: approved_by.map(str::to_string),
        failure_output: match (&run, verdict) {
            (Some(step), CiVerdict::Failed) => step.output.trim_end().to_string(),
            _ => String::new(),
        },
    })
}

fn headline(verdict: CiVerdict) -> &'static str {
    match verdict {
        CiVerdict::Passed => "PASSED",
        CiVerdict::Failed => "FAILED",
        CiVerdict::Held => "HELD",
        CiVerdict::NotVerified => "NOT VERIFIED",
    }
}

pub fn render_markdown(report: &CiReport) -> String {
    let short = |sha: &str| sha.get(..12).unwrap_or(sha).to_string();
    let mut out = format!("**tekmor ci: {}**\n\n| Field | Value |\n|---|---|\n", headline(report.verdict));
    out.push_str(&format!("| Head | `{}` |\n", short(&report.head)));
    out.push_str(&format!("| Checks from | `{}` (merge-base) |\n", short(&report.merge_base)));
    if let Some(cmd) = &report.command {
        out.push_str(&format!("| Command | `{cmd}` |\n"));
    }
    if let Some(code) = report.exit_code {
        out.push_str(&format!("| Exit code | {code} |\n"));
    }
    if let Some(login) = &report.approved_by {
        out.push_str(&format!("| Approved by | {login} |\n"));
    }
    for gap in &report.gaps {
        out.push_str(&format!("\n- {gap}"));
    }
    out.push('\n');
    if !report.failure_output.is_empty() {
        out.push_str(&format!("\n<details><summary>Suite output (tail)</summary>\n\n```\n{}\n```\n\n</details>\n", report.failure_output));
    }
    out
}

pub fn render_text(report: &CiReport) -> String {
    let mut out = format!("tekmor ci: {}\n  head:        {}\n  checks from: {}\n", headline(report.verdict), report.head, report.merge_base);
    if let (Some(cmd), Some(code)) = (&report.command, report.exit_code) {
        out.push_str(&format!("  command:     {cmd} (exit {code})\n"));
    }
    if let Some(login) = &report.approved_by {
        out.push_str(&format!("  approved by: {login}\n"));
    }
    for gap in &report.gaps {
        out.push_str(&format!("  - {gap}\n"));
    }
    if !report.failure_output.is_empty() {
        out.push_str(&format!("--- suite output (tail) ---\n{}\n", report.failure_output));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const PASSING_SUITE: &str = "#!/bin/sh\n[ -z \"$TEKMOR_BASE\" ] || { echo 'TEKMOR_BASE leaked into the suite'; exit 1; }\ngrep -q 'fn good' src/lib.rs || { echo 'fn good is missing'; exit 1; }\n";

    fn sh(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn put(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        if rel.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn commit(root: &Path, message: &str) {
        sh(root, &["add", "-A"]);
        sh(root, &["commit", "-qm", message]);
    }

    struct Fixture {
        repo: TempDir,
        aux: TempDir,
        base: String,
    }

    impl Fixture {
        fn new() -> Self {
            let repo = TempDir::new().unwrap();
            let root = repo.path();
            sh(root, &["init", "-q", "-b", "main"]);
            put(root, ".verify/verify.toml", "[verify]\ncommands = [\".verify/run.sh\"]\n\n[verify.default]\ncommand = [\".verify/run.sh\"]\n");
            put(root, ".verify/run.sh", PASSING_SUITE);
            put(root, "src/lib.rs", "fn good() {}\n#[test]\nfn keeps() {}\n");
            commit(root, "base");
            let base = git_text(root, &["rev-parse", "HEAD"]).unwrap();
            sh(root, &["checkout", "-q", "-b", "pr"]);
            Self { repo, aux: TempDir::new().unwrap(), base }
        }

        fn root(&self) -> &Path {
            self.repo.path()
        }

        fn ci(&self, approved_by: Option<&str>) -> CiReport {
            let store = BrainStore::open(&self.aux.path().join("t.db")).unwrap();
            let keys = KeyStore::File(self.aux.path().join("review.key"));
            run(&store, self.root(), &keys, &self.base, approved_by).unwrap()
        }
    }

    #[test]
    fn an_untouched_suite_on_good_code_passes() {
        let fx = Fixture::new();
        put(fx.root(), "src/lib.rs", "fn good() { let _ = 1; }\n#[test]\nfn keeps() {}\n");
        commit(fx.root(), "work");
        let report = fx.ci(None);
        assert_eq!(report.verdict, CiVerdict::Passed, "{report:?}");
        assert_eq!(report.command.as_deref(), Some(".verify/run.sh"));
    }

    #[test]
    fn a_pr_that_guts_its_suite_still_fails_on_the_base_suite() {
        let fx = Fixture::new();
        put(fx.root(), ".verify/run.sh", "#!/bin/sh\nexit 0\n");
        put(fx.root(), "src/lib.rs", "fn broken() {}\n#[test]\nfn keeps() {}\n");
        commit(fx.root(), "gut the suite and break the code");
        let report = fx.ci(None);
        assert_eq!(report.verdict, CiVerdict::Failed, "{report:?}");
        assert_eq!(report.changed_checks, vec![".verify/run.sh".to_string()]);
    }

    #[test]
    fn a_changed_check_on_good_code_is_held_until_approved() {
        let fx = Fixture::new();
        put(fx.root(), ".verify/run.sh", "#!/bin/sh\nexit 0\n");
        commit(fx.root(), "gut the suite");
        assert_eq!(fx.ci(None).verdict, CiVerdict::Held);
        let approved = fx.ci(Some("maintainer"));
        assert_eq!(approved.verdict, CiVerdict::Passed, "{approved:?}");
        assert_eq!(approved.approved_by.as_deref(), Some("maintainer"));
    }

    #[test]
    fn a_check_file_added_by_the_pr_is_absent_during_the_run_and_restored_after() {
        let fx = Fixture::new();
        put(fx.root(), ".verify/checks/extra.sh", "#!/bin/sh\nexit 0\n");
        put(fx.root(), ".verify/run.sh", "#!/bin/sh\ntest -f .verify/checks/extra.sh && exit 0\nexit 1\n");
        commit(fx.root(), "add a check");
        let report = fx.ci(None);
        assert_eq!(report.verdict, CiVerdict::Held, "{report:?}");
        assert!(fx.root().join(".verify/checks/extra.sh").is_file());
        assert_eq!(fs::read_to_string(fx.root().join(".verify/run.sh")).unwrap(), "#!/bin/sh\ntest -f .verify/checks/extra.sh && exit 0\nexit 1\n");
        assert!(uncommitted(fx.root()).unwrap().is_empty());
        assert!(std::env::var("TEKMOR_BASE").is_err());
    }

    #[test]
    fn a_committed_test_removal_is_held_against_the_merge_base() {
        let fx = Fixture::new();
        put(fx.root(), "src/lib.rs", "fn good() {}\n");
        commit(fx.root(), "drop a test");
        let report = fx.ci(None);
        assert_eq!(report.verdict, CiVerdict::Held, "{report:?}");
        assert!(report.gaps.iter().any(|gap| gap.contains("removes 1 test")), "{report:?}");
    }

    #[test]
    fn review_ratchet_and_broken_copies_are_check_files_but_records_are_not() {
        for path in [".verify/review.toml", ".verify/ratchet.toml", ".verify/broken/lib.rs", ".verify/checks/a.sh", ".verify/run.sh"] {
            assert!(verify::is_proof_input(path), "{path}");
        }
        for path in [".verify/artifacts/x.log", ".verify/features/cli.md", ".verify/knowledge-base/a.md", ".verify/README.md", "src/lib.rs"] {
            assert!(!verify::is_proof_input(path), "{path}");
        }
    }

    #[test]
    fn a_pr_that_drops_a_risky_path_from_review_toml_is_held() {
        let fx = Fixture::new();
        put(fx.root(), ".verify/review.toml", "risky = []\n");
        commit(fx.root(), "no more risky paths");
        let report = fx.ci(None);
        assert_eq!(report.verdict, CiVerdict::Held, "{report:?}");
        assert_eq!(report.changed_checks, vec![".verify/review.toml".to_string()]);
    }

    #[test]
    fn failing_code_fails() {
        let fx = Fixture::new();
        put(fx.root(), "src/lib.rs", "fn bad() {}\n#[test]\nfn keeps() {}\n");
        commit(fx.root(), "break it");
        let report = fx.ci(None);
        assert_eq!(report.verdict, CiVerdict::Failed);
        assert!(render_text(&report).contains("suite output"), "{report:?}");
    }

    #[test]
    fn an_uncommitted_change_is_refused() {
        let fx = Fixture::new();
        put(fx.root(), "src/lib.rs", "fn good() {}\nfn dirty() {}\n");
        let store = BrainStore::open(&fx.aux.path().join("t.db")).unwrap();
        let keys = KeyStore::File(fx.aux.path().join("review.key"));
        let err = run(&store, fx.root(), &keys, &fx.base, None).unwrap_err();
        assert!(err.to_string().contains("clean checkout"), "{err}");
    }

    #[test]
    fn a_base_without_a_default_suite_is_not_verified() {
        let fx = Fixture::new();
        sh(fx.root(), &["checkout", "-q", "main"]);
        put(fx.root(), ".verify/verify.toml", "[verify]\ncommands = [\".verify/run.sh\"]\n");
        commit(fx.root(), "no default");
        let fx = Fixture { base: git_text(fx.root(), &["rev-parse", "HEAD"]).unwrap(), ..fx };
        sh(fx.root(), &["checkout", "-q", "-b", "pr2"]);
        let report = fx.ci(None);
        assert_eq!(report.verdict, CiVerdict::NotVerified, "{report:?}");
    }
}
