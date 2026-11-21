use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use walkdir::WalkDir;

use crate::verify;

pub const BROKEN_DIR: &str = ".verify/broken";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub cmd: String,
    pub caught: bool,
}

pub fn run(root: &Path) -> Result<Vec<Verdict>> {
    let broken = root.join(BROKEN_DIR);
    if !broken.is_dir() {
        bail!("no broken copy at {BROKEN_DIR}; checks cannot be proven to fail");
    }
    verify::load_allowlist(root)
        .into_iter()
        .map(|entry| {
            let caught = caught_on_broken_copy(root, &broken, &entry.argv)?;
            Ok(Verdict { cmd: entry.display(), caught })
        })
        .collect()
}

fn caught_on_broken_copy(root: &Path, broken: &Path, argv: &[String]) -> Result<bool> {
    let copy = tempfile::TempDir::new().context("temp copy")?;
    for path in tracked_files(root)? {
        copy_into(&root.join(&path), &copy.path().join(&path))?;
    }
    for entry in WalkDir::new(broken).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
        let rel = entry.path().strip_prefix(broken)?;
        copy_into(entry.path(), &copy.path().join(rel))?;
    }
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(copy.path())
        .status()
        .with_context(|| format!("run `{}` on the broken copy", verify::render_argv(argv)))?;
    Ok(!status.success())
}

fn tracked_files(root: &Path) -> Result<Vec<PathBuf>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-files")
        .output()
        .context("list tracked files")?;
    Ok(String::from_utf8_lossy(&out.stdout).lines().map(PathBuf::from).collect())
}

fn copy_into(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to).with_context(|| format!("copy {}", from.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repo(allow: &str, broken: bool) -> TempDir {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        Command::new("git").arg("-C").arg(root).args(["init", "-q"]).status().unwrap();
        fs::create_dir_all(root.join(".verify")).unwrap();
        fs::write(root.join(".verify/verify.toml"), allow).unwrap();
        fs::write(root.join("src.txt"), "ok").unwrap();
        if broken {
            fs::create_dir_all(root.join(".verify/broken")).unwrap();
            fs::write(root.join(".verify/broken/src.txt"), "bad").unwrap();
        }
        Command::new("git").arg("-C").arg(root).args(["add", "."]).status().unwrap();
        dir
    }

    #[test]
    fn a_check_that_fails_on_the_broken_copy_is_caught() {
        let dir = repo("[verify]\ncommands = [\"grep -q ok src.txt\"]\n", true);
        let verdicts = run(dir.path()).unwrap();
        assert_eq!(verdicts, vec![Verdict { cmd: "grep -q ok src.txt".into(), caught: true }]);
    }

    #[test]
    fn a_check_that_passes_on_the_broken_copy_is_not_caught() {
        let dir = repo("[verify]\ncommands = [\"true\"]\n", true);
        let verdicts = run(dir.path()).unwrap();
        assert_eq!(verdicts, vec![Verdict { cmd: "true".into(), caught: false }]);
    }

    #[test]
    fn a_shell_entry_runs_through_sh_c() {
        let dir = repo("[verify]\ncommands = ['FOO=ok grep -q $FOO src.txt']\n", true);
        let verdicts = run(dir.path()).unwrap();
        assert_eq!(
            verdicts,
            vec![Verdict {
                cmd: "sh -c 'FOO=ok grep -q $FOO src.txt'".into(),
                caught: true
            }]
        );
    }

    #[test]
    fn without_a_broken_copy_the_checks_are_unproven() {
        let dir = repo("[verify]\ncommands = [\"true\"]\n", false);
        assert!(run(dir.path()).is_err());
    }
}
