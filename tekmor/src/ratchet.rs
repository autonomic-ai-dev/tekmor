use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const RATCHET_FILE: &str = ".verify/ratchet.toml";
const MAX_LISTED_HITS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub pattern: String,
    #[serde(default)]
    pub paths: Vec<String>,
    pub baseline: usize,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_tests: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RatchetFile {
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub name: String,
    pub count: usize,
    pub baseline: usize,
}

impl Reading {
    pub fn passes(&self) -> bool {
        self.count <= self.baseline
    }
}

pub fn load(root: &Path) -> Result<RatchetFile> {
    let path = root.join(RATCHET_FILE);
    if !path.is_file() {
        return Ok(RatchetFile::default());
    }
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("invalid {RATCHET_FILE}"))
}

fn tracked_files(root: &Path) -> Result<Vec<String>> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files"])
        .output()
        .context("list tracked files")?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

fn in_scope(rule: &Rule, path: &str) -> bool {
    rule.paths.is_empty() || rule.paths.iter().any(|p| path.starts_with(p.as_str()))
}

fn outside_test_items(text: &str) -> Vec<(usize, &str)> {
    let mut kept = Vec::new();
    let mut pending = false;
    let mut skipping = false;
    let mut depth: i64 = 0;
    for (index, line) in text.lines().enumerate() {
        if !pending && !skipping && line.trim_start().starts_with("#[cfg(test)]") {
            pending = true;
            depth = 0;
        }
        if !pending && !skipping {
            kept.push((index, line));
            continue;
        }
        let opens = line.matches('{').count() as i64;
        depth += opens - line.matches('}').count() as i64;
        if pending && opens > 0 {
            pending = false;
            skipping = true;
        } else if pending && line.contains(';') {
            pending = false;
        }
        if skipping && depth <= 0 {
            skipping = false;
        }
    }
    kept
}

struct Hit {
    path: String,
    line_no: usize,
    line: String,
    matches: usize,
}

fn scan(root: &Path, rule: &Rule, files: &[String]) -> Result<Vec<Hit>> {
    let re = regex::Regex::new(&rule.pattern)
        .with_context(|| format!("rule {} has an invalid pattern", rule.name))?;
    let mut hits = Vec::new();
    for path in files.iter().filter(|path| in_scope(rule, path)) {
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        let lines = if rule.exclude_tests && path.ends_with(".rs") {
            outside_test_items(&text)
        } else {
            text.lines().enumerate().collect()
        };
        for (index, line) in lines {
            let matches = re.find_iter(line).count();
            if matches > 0 {
                hits.push(Hit { path: path.clone(), line_no: index + 1, line: line.trim().to_string(), matches });
            }
        }
    }
    Ok(hits)
}

fn hit_lines(root: &Path, rule: &Rule) -> Result<Vec<String>> {
    Ok(scan(root, rule, &tracked_files(root)?)?
        .into_iter()
        .map(|hit| format!("{}:{}: {}", hit.path, hit.line_no, hit.line))
        .collect())
}

pub fn read(root: &Path, file: &RatchetFile) -> Result<Vec<Reading>> {
    let files = tracked_files(root)?;
    file.rules
        .iter()
        .map(|rule| {
            Ok(Reading {
                name: rule.name.clone(),
                count: scan(root, rule, &files)?.iter().map(|hit| hit.matches).sum(),
                baseline: rule.baseline,
            })
        })
        .collect()
}

pub fn lowered(file: &RatchetFile, readings: &[Reading]) -> RatchetFile {
    RatchetFile {
        rules: file
            .rules
            .iter()
            .map(|rule| {
                let count = readings
                    .iter()
                    .find(|r| r.name == rule.name)
                    .map_or(rule.baseline, |r| r.count);
                Rule {
                    baseline: count.min(rule.baseline),
                    ..rule.clone()
                }
            })
            .collect(),
    }
}


pub fn add(root: &Path, name: &str, pattern: &str, paths: Vec<String>, baseline: Option<usize>) -> Result<Rule> {
    regex::Regex::new(pattern).with_context(|| format!("invalid pattern `{pattern}`"))?;
    let mut file = load(root)?;
    if file.rules.iter().any(|rule| rule.name == name) {
        anyhow::bail!("a rule named {name} already exists in {RATCHET_FILE}");
    }
    let mut rule = Rule {
        name: name.to_string(),
        pattern: pattern.to_string(),
        paths,
        baseline: 0,
        exclude_tests: false,
    };
    rule.baseline = match baseline {
        Some(value) => value,
        None => read(root, &RatchetFile { rules: vec![rule.clone()] })?[0].count,
    };
    file.rules.push(rule.clone());
    let path = root.join(RATCHET_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string(&file).context("serialize ratchet")?;
    std::fs::write(&path, text).with_context(|| format!("write {RATCHET_FILE}"))?;
    Ok(rule)
}

pub fn run(root: &Path, update: bool) -> Result<bool> {
    let file = load(root)?;
    let readings = read(root, &file)?;
    for r in &readings {
        let verdict = if r.passes() { "ok" } else { "over baseline" };
        println!("{:<32} {:>6} / baseline {:<6} {verdict}", r.name, r.count, r.baseline);
    }
    for reading in readings.iter().filter(|r| !r.passes()) {
        let Some(rule) = file.rules.iter().find(|rule| rule.name == reading.name) else {
            continue;
        };
        for line in hit_lines(root, rule)?.iter().take(MAX_LISTED_HITS) {
            println!("  {line}");
        }
        println!("  rule {}: remove the new matches above. The baseline only goes down; the project owner raises it in {RATCHET_FILE}.", rule.name);
    }
    let ok = readings.iter().all(Reading::passes);
    if update {
        let lowered = lowered(&file, &readings);
        let text = toml::to_string(&lowered).context("serialize ratchet")?;
        std::fs::write(root.join(RATCHET_FILE), text).with_context(|| format!("write {RATCHET_FILE}"))?;
        println!("baselines lowered where counts went down; never raised");
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, baseline: usize) -> Rule {
        Rule {
            name: "r".into(),
            pattern: pattern.into(),
            paths: vec![],
            baseline,
            exclude_tests: false,
        }
    }

    #[test]
    fn a_count_at_or_below_the_baseline_passes() {
        assert!(Reading { name: "r".into(), count: 2, baseline: 2 }.passes());
        assert!(Reading { name: "r".into(), count: 1, baseline: 2 }.passes());
    }

    #[test]
    fn a_count_above_the_baseline_fails() {
        assert!(!Reading { name: "r".into(), count: 3, baseline: 2 }.passes());
    }

    #[test]
    fn lowering_never_raises_a_baseline() {
        let file = RatchetFile { rules: vec![rule("x", 5)] };
        let up = [Reading { name: "r".into(), count: 9, baseline: 5 }];
        assert_eq!(lowered(&file, &up).rules[0].baseline, 5);
        let down = [Reading { name: "r".into(), count: 2, baseline: 5 }];
        assert_eq!(lowered(&file, &down).rules[0].baseline, 2);
    }

    #[test]
    fn counts_matches_across_tracked_files_only() {
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git").arg("-C").arg(dir.path()).args(["init", "-q"]).status().unwrap();
        std::fs::write(dir.path().join("a.rs"), "x.unwrap(); y.unwrap();").unwrap();
        std::fs::write(dir.path().join("untracked.rs"), "z.unwrap();").unwrap();
        std::process::Command::new("git").arg("-C").arg(dir.path()).args(["add", "a.rs"]).status().unwrap();
        let file = RatchetFile { rules: vec![rule(r"\.unwrap\(\)", 0)] };
        let readings = read(dir.path(), &file).unwrap();
        assert_eq!(readings[0].count, 2);
    }

    #[test]
    fn an_invalid_pattern_is_an_error_not_a_pass() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = RatchetFile { rules: vec![rule("(", 0)] };
        assert!(read(dir.path(), &file).is_err());
    }

    fn tracked_repo(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git").arg("-C").arg(dir.path()).args(["init", "-q"]).status().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
            std::process::Command::new("git").arg("-C").arg(dir.path()).args(["add", name]).status().unwrap();
        }
        dir
    }

    #[test]
    fn a_correction_rule_starts_at_the_current_count_so_only_new_matches_fail() {
        let dir = tracked_repo(&[("a.rs", "let t = Utc::now();")]);
        add(dir.path(), "no-utc-now", r"Utc::now\(\)", vec![], None).unwrap();
        assert!(run(dir.path(), false).unwrap());
        std::fs::write(dir.path().join("a.rs"), "let t = Utc::now(); let u = Utc::now();").unwrap();
        std::process::Command::new("git").arg("-C").arg(dir.path()).args(["add", "a.rs"]).status().unwrap();
        assert!(!run(dir.path(), false).unwrap());
    }

    #[test]
    fn a_rule_over_its_baseline_lists_each_new_match_with_its_location() {
        let dir = tracked_repo(&[("a.rs", "let t = Utc::now();\nlet n = 1;\n")]);
        add(dir.path(), "no-utc-now", r"Utc::now\(\)", vec![], None).unwrap();
        std::fs::write(dir.path().join("a.rs"), "let t = Utc::now();\nlet u = Utc::now();\n").unwrap();
        std::process::Command::new("git").arg("-C").arg(dir.path()).args(["add", "a.rs"]).status().unwrap();
        let rule = load(dir.path()).unwrap().rules.remove(0);
        assert_eq!(hit_lines(dir.path(), &rule).unwrap(), vec!["a.rs:1: let t = Utc::now();", "a.rs:2: let u = Utc::now();"]);
    }

    #[test]
    fn adding_a_rule_with_a_duplicate_name_is_refused() {
        let dir = tracked_repo(&[]);
        add(dir.path(), "r", "x", vec![], Some(0)).unwrap();
        assert!(add(dir.path(), "r", "y", vec![], Some(0)).is_err());
    }

    #[test]
    fn adding_a_rule_with_an_invalid_pattern_writes_nothing() {
        let dir = tracked_repo(&[]);
        assert!(add(dir.path(), "bad", "(", vec![], None).is_err());
        assert!(!dir.path().join(RATCHET_FILE).exists());
    }

    const MIXED: &str = "fn prod() { a.unwrap(); }\n\n#[cfg(test)]\nmod tests {\n    fn t() {\n        b.unwrap();\n        c.unwrap();\n    }\n}\n\nfn after() { d.unwrap(); }\n";

    fn unwrap_rule(exclude_tests: bool) -> Rule {
        Rule { exclude_tests, ..rule(r"\.unwrap\(\)", 10) }
    }

    #[test]
    fn excluding_tests_skips_a_cfg_test_module_and_resumes_after_it() -> Result<()> {
        let dir = tracked_repo(&[("a.rs", MIXED)]);
        let with = read(dir.path(), &RatchetFile { rules: vec![unwrap_rule(true)] })?;
        let without = read(dir.path(), &RatchetFile { rules: vec![unwrap_rule(false)] })?;
        assert_eq!(with[0].count, 2);
        assert_eq!(without[0].count, 4);
        Ok(())
    }

    #[test]
    fn excluding_tests_skips_a_cfg_test_function_and_a_one_line_item() {
        let text = "#[cfg(test)]\nfn helper() {\n    x.unwrap();\n}\n#[cfg(test)]\nuse foo::bar;\nfn kept() {}\n";
        let kept: Vec<&str> = outside_test_items(text).into_iter().map(|(_, line)| line).collect();
        assert_eq!(kept, ["fn kept() {}"]);
    }

    #[test]
    fn excluding_tests_only_applies_to_rust_files() -> Result<()> {
        let dir = tracked_repo(&[("a.txt", MIXED)]);
        let found = read(dir.path(), &RatchetFile { rules: vec![unwrap_rule(true)] })?;
        assert_eq!(found[0].count, 4);
        Ok(())
    }
}
