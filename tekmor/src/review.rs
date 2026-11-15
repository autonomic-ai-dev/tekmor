use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const REVIEW_CONFIG: &str = ".verify/review.toml";
pub const REVIEWS_DIR: &str = ".verify/artifacts/reviews";
pub const KEYCHAIN_SERVICE: &str = "tekmor-review-key";
const KEY_FILE: &str = "review.key";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStore {
    File(PathBuf),
    Keychain,
}

pub fn default_store(home: &Path) -> KeyStore {
    if cfg!(target_os = "macos") {
        KeyStore::Keychain
    } else {
        KeyStore::File(home.join(KEY_FILE))
    }
}

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
    pub mac: String,
}

fn random_hex() -> Result<String> {
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .context("read random bytes for the review key")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn keychain_read() -> Option<String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn key(store: &KeyStore) -> Result<Vec<u8>> {
    match store {
        KeyStore::File(path) => {
            if let Ok(existing) = fs::read(path) {
                return Ok(existing);
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let fresh = random_hex()?;
            fs::write(path, &fresh).with_context(|| format!("write {}", path.display()))?;
            Ok(fresh.into_bytes())
        }
        KeyStore::Keychain => {
            if let Some(existing) = keychain_read() {
                return Ok(existing.into_bytes());
            }
            let fresh = random_hex()?;
            let status = Command::new("security")
                .args(["add-generic-password", "-s", KEYCHAIN_SERVICE, "-a", "tekmor", "-w", &fresh, "-U"])
                .status()
                .context("store the review key in the keychain")?;
            if !status.success() {
                bail!("could not store the review key in the keychain");
            }
            Ok(fresh.into_bytes())
        }
    }
}

fn hmac(key: &[u8], message: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let inner = {
        let mut h = Sha256::new();
        h.update(k.map(|b| b ^ 0x36));
        h.update(message);
        h.finalize()
    };
    let mut h = Sha256::new();
    h.update(k.map(|b| b ^ 0x5c));
    h.update(inner);
    format!("{:x}", h.finalize())
}

fn signed_text(code_hash: &str, verdict: &str, reviewer: &str, notes: &str) -> String {
    format!("{code_hash}\n{verdict}\n{reviewer}\n{notes}")
}

fn record_path(root: &Path, code_hash: &str) -> PathBuf {
    root.join(REVIEWS_DIR).join(format!("{code_hash}.json"))
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

pub fn record(root: &Path, store: &KeyStore, code_hash: &str, verdict: &str, reviewer: &str, notes: &str) -> Result<PathBuf> {
    let key = key(store)?;
    let mac = hmac(&key, signed_text(code_hash, verdict, reviewer, notes).as_bytes());
    let path = record_path(root, code_hash);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let rec = ReviewRecord {
        code_hash: code_hash.into(),
        verdict: verdict.into(),
        reviewer: reviewer.into(),
        notes: notes.into(),
        mac,
    };
    fs::write(&path, serde_json::to_string_pretty(&rec)?).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn verified_pass(root: &Path, store: &KeyStore, code_hash: &str) -> bool {
    let Some(rec) = fs::read_to_string(record_path(root, code_hash))
        .ok()
        .and_then(|raw| serde_json::from_str::<ReviewRecord>(&raw).ok())
    else {
        return false;
    };
    let Ok(key) = key(store) else {
        return false;
    };
    let expected = hmac(&key, signed_text(&rec.code_hash, &rec.verdict, &rec.reviewer, &rec.notes).as_bytes());
    rec.code_hash == code_hash && rec.verdict == "pass" && rec.mac == expected
}

pub fn gap(root: &Path, store: &KeyStore, code_hash: Option<&str>) -> Option<String> {
    let risky = risky_changed(root);
    if risky.is_empty() {
        return None;
    }
    let passed = code_hash.is_some_and(|hash| verified_pass(root, store, hash));
    (!passed).then(|| format!("risky paths changed without an independent pass review: {}", risky.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repo() -> (TempDir, KeyStore) {
        let dir = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let root = dir.path();
        std::process::Command::new("git").arg("-C").arg(root).args(["init", "-q"]).status().unwrap();
        fs::create_dir_all(root.join(".verify")).unwrap();
        fs::write(root.join(REVIEW_CONFIG), "risky = [\"src/auth\"]\n").unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/auth.rs"), "fn login() {}").unwrap();
        let store = KeyStore::File(home.path().join(KEY_FILE));
        (dir, store)
    }

    #[test]
    fn no_risky_change_needs_no_review() {
        let dir = TempDir::new().unwrap();
        let store = KeyStore::File(dir.path().join(KEY_FILE));
        assert!(gap(dir.path(), &store, Some("h")).is_none());
    }

    #[test]
    fn a_risky_change_without_a_review_is_a_gap() {
        let (dir, store) = repo();
        assert!(gap(dir.path(), &store, Some("h")).unwrap().contains("src/auth.rs"));
    }

    #[test]
    fn a_signed_pass_review_for_this_tree_closes_the_gap() {
        let (dir, store) = repo();
        record(dir.path(), &store, "h", "pass", "reviewer", "ok").unwrap();
        assert!(gap(dir.path(), &store, Some("h")).is_none());
    }

    #[test]
    fn a_fail_review_or_a_review_for_another_tree_is_a_gap() {
        let (dir, store) = repo();
        record(dir.path(), &store, "h", "fail", "reviewer", "bad").unwrap();
        assert!(gap(dir.path(), &store, Some("h")).is_some());
        record(dir.path(), &store, "other", "pass", "reviewer", "ok").unwrap();
        assert!(gap(dir.path(), &store, Some("h")).is_some());
    }

    #[test]
    fn a_hand_written_review_without_the_key_is_a_gap() {
        let (dir, store) = repo();
        let forged = ReviewRecord {
            code_hash: "h".into(),
            verdict: "pass".into(),
            reviewer: "fresh-agent".into(),
            notes: "looks fine".into(),
            mac: "not-a-real-mac".into(),
        };
        fs::create_dir_all(dir.path().join(REVIEWS_DIR)).unwrap();
        fs::write(record_path(dir.path(), "h"), serde_json::to_string(&forged).unwrap()).unwrap();
        assert!(gap(dir.path(), &store, Some("h")).is_some());
    }

    #[test]
    fn editing_a_signed_review_breaks_its_signature() {
        let (dir, store) = repo();
        let path = record(dir.path(), &store, "h", "fail", "reviewer", "bad").unwrap();
        let mut rec: ReviewRecord = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        rec.verdict = "pass".into();
        fs::write(&path, serde_json::to_string(&rec).unwrap()).unwrap();
        assert!(gap(dir.path(), &store, Some("h")).is_some());
    }
}
