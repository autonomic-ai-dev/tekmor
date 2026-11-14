use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const REVIEW_CONFIG: &str = ".verify/review.toml";

#[derive(Debug, Default, Deserialize)]
struct ReviewConfig {
    #[serde(default)]
    risky: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRecord {
    pub code_hash: String,
    pub verdict: String,
    pub reviewer: String,
    pub notes: String,
}

fn record_path(root: &Path, code_hash: &str) -> PathBuf {
    root.join(".verify/artifacts/reviews").join(format!("{code_hash}.json"))
}

fn risky_prefixes(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join(REVIEW_CONFIG))
        .ok()
        .and_then(|raw| toml::from_str::<ReviewConfig>(&raw).ok())
        .map(|c| c.risky)
        .unwrap_or_default()
}

pub fn risky_changed(root: &Path) -> Vec<String> {
    let prefixes = risky_prefixes(root);
    crate::stop_gate::changed_paths(root)
        .into_iter()
        .filter(|path| prefixes.iter().any(|p| path.starts_with(p.as_str())))
        .collect()
}

pub fn record(root: &Path, code_hash: &str, verdict: &str, reviewer: &str, notes: &str) -> Result<PathBuf> {
    let path = record_path(root, code_hash);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let rec = ReviewRecord {
        code_hash: code_hash.into(),
        verdict: verdict.into(),
        reviewer: reviewer.into(),
        notes: notes.into(),
    };
    fs::write(&path, serde_json::to_string_pretty(&rec)?).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

pub fn gap(root: &Path, code_hash: Option<&str>) -> Option<String> {
    let risky = risky_changed(root);
    if risky.is_empty() {
        return None;
    }
    let passed = code_hash.is_some_and(|hash| {
        fs::read_to_string(record_path(root, hash))
            .ok()
            .and_then(|raw| serde_json::from_str::<ReviewRecord>(&raw).ok())
            .is_some_and(|rec| rec.verdict == "pass")
    });
    (!passed).then(|| format!("risky paths changed without an independent pass review: {}", risky.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::process::Command::new("git").arg("-C").arg(root).args(["init", "-q"]).status().unwrap();
        fs::create_dir_all(root.join(".verify")).unwrap();
        fs::write(root.join(REVIEW_CONFIG), "risky = [\"src/auth\"]\n").unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/auth.rs"), "fn login() {}").unwrap();
        dir
    }

    #[test]
    fn no_risky_change_needs_no_review() {
        let dir = TempDir::new().unwrap();
        assert!(gap(dir.path(), Some("h")).is_none());
    }

    #[test]
    fn a_risky_change_without_a_review_is_a_gap() {
        let dir = repo();
        assert!(gap(dir.path(), Some("h")).unwrap().contains("src/auth.rs"));
    }

    #[test]
    fn a_pass_review_for_this_tree_closes_the_gap() {
        let dir = repo();
        record(dir.path(), "h", "pass", "reviewer", "ok").unwrap();
        assert!(gap(dir.path(), Some("h")).is_none());
    }

    #[test]
    fn a_fail_review_or_a_review_for_another_tree_is_a_gap() {
        let dir = repo();
        record(dir.path(), "h", "fail", "reviewer", "bad").unwrap();
        assert!(gap(dir.path(), Some("h")).is_some());
        record(dir.path(), "other", "pass", "reviewer", "ok").unwrap();
        assert!(gap(dir.path(), Some("h")).is_some());
    }
}
