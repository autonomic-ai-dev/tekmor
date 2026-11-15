use std::path::Path;

use crate::feature_map::{self, FeatureMap};
use crate::verify;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub feature: String,
    pub problem: String,
}

pub fn check(root: &Path) -> Vec<Drift> {
    let features = feature_map::load(&root.join(".verify/features"));
    let mut drift = Vec::new();
    for feature in &features {
        drift.extend(feature_drift(root, feature));
    }
    drift
}

fn feature_drift(root: &Path, feature: &FeatureMap) -> Vec<Drift> {
    let mut problems = Vec::new();
    for path in &feature.code {
        if !root.join(path).exists() {
            problems.push(format!("code path `{path}` does not exist"));
        }
    }
    for cmd in &feature.commands {
        if !verify::command_is_allowlisted(cmd, Some(root)) {
            problems.push(format!("check `{cmd}` is not in the allowlist"));
        }
    }
    problems
        .into_iter()
        .map(|problem| Drift {
            feature: feature.file.clone(),
            problem,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn repo(map: &str, allow: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".verify/features")).unwrap();
        fs::write(dir.path().join(".verify/features/demo.md"), map).unwrap();
        fs::write(dir.path().join(".verify/verify.toml"), allow).unwrap();
        dir
    }

    const MAP: &str = "---\nfeature: demo\n---\n\n# demo\n\n## Code\n\n- `src/lib.rs`\n\n## Verification Suites\n\n- `true`\n";

    #[test]
    fn a_map_whose_files_and_checks_exist_has_no_drift() {
        let dir = repo(MAP, "[verify]\ncommands = [\"true\"]\n");
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        assert!(check(dir.path()).is_empty());
    }

    #[test]
    fn a_missing_code_path_is_drift() {
        let dir = repo(MAP, "[verify]\ncommands = [\"true\"]\n");
        let found = check(dir.path());
        assert_eq!(found.len(), 1);
        assert!(found[0].problem.contains("src/lib.rs"));
    }

    #[test]
    fn a_check_that_is_not_allowlisted_is_drift() {
        let dir = repo(MAP, "[verify]\ncommands = [\"false\"]\n");
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        let found = check(dir.path());
        assert!(found.iter().any(|d| d.problem.contains("not in the allowlist")));
    }
}
