use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::db::store::BrainStore;
use crate::feature_map;
use crate::receipt::{Receipt, Verdict};
use crate::review::KeyStore;
use crate::settings::{AgentBrainSettings, GateMode};
use crate::verify::{self, VerifySource};

const BLOCK_REASON: &str = "Verification is missing. Files changed in this repo have no passing `tekmor verify -- <cmd>` run recorded after the last edit. Run the project's checks (for example `cargo test` or the verify suite), let them pass, then finish.";
const REVIEW_COMMAND: &str = "`tekmor verify review --verdict pass --reviewer NAME`";
const STOP_BLOCK_LIMIT: u32 = 3;
const STATE_FILE: &str = "stop-gate-state.json";

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StopInput {
    pub cwd: Option<String>,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BlockState {
    session_id: String,
    edit_ms: i64,
    blocks: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopOutcome {
    Clear,
    Verified(String),
    Block(String),
    HandBack(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Block,
}

fn proof_line(cmd: &str, code_hash: Option<&str>) -> String {
    match code_hash {
        Some(hash) => format!("`{cmd}` passed on code {}", &hash[..hash.len().min(12)]),
        None => format!("`{cmd}` passed on the current code"),
    }
}

fn receipt_proof(receipt: &Receipt) -> Option<String> {
    let evidence = receipt
        .evidence
        .as_ref()
        .filter(|_| receipt.verdict == Verdict::Passed)?;
    Some(proof_line(&evidence.cmd, receipt.code_hash.as_deref()))
}

fn held_reason(receipt: &Receipt) -> Option<String> {
    let passing = receipt.evidence.as_ref().is_some_and(|e| e.exit_code == 0);
    if !passing || receipt.verdict == Verdict::Passed {
        return None;
    }
    receipt
        .review_gap
        .as_deref()
        .map(|gap| format!("The receipt for this code is NOT VERIFIED: {gap}. Record one with {REVIEW_COMMAND}."))
}

fn not_counted_reason(cmd: &str) -> String {
    format!("`{cmd}` passed, but the receipt for this code is not PASSED. Run `tekmor verify receipt` to see why, fix it, then finish.")
}

pub fn decide(verified: bool, blocks_so_far: u32) -> Decision {
    if verified || blocks_so_far >= STOP_BLOCK_LIMIT {
        Decision::Allow
    } else {
        Decision::Block
    }
}

fn prior_blocks(prior: &BlockState, session_id: &str, edit_ms: i64) -> u32 {
    if prior.session_id == session_id && prior.edit_ms == edit_ms {
        prior.blocks
    } else {
        0
    }
}

fn load_state(path: &Path) -> BlockState {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(path: &Path, state: &BlockState) -> Result<()> {
    fs::write(path, serde_json::to_string(state)?)?;
    Ok(())
}

fn git_root(cwd: &Path) -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let root = String::from_utf8(out.stdout).ok()?;
    Some(PathBuf::from(root.trim_end()))
}

pub(crate) fn changed_paths(root: &Path) -> Vec<String> {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.get(3..))
        .filter_map(|path| path.rsplit(" -> ").next())
        .map(str::to_string)
        .collect()
}

fn repo_name(repo: &Path) -> String {
    repo.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn named_reason(root: &Path, reason: &str) -> String {
    format!("Repo {} is blocked: {reason}", repo_name(root))
}

fn last_edit_ms(root: &Path, changed: &[String]) -> Option<i64> {
    changed
        .iter()
        .filter_map(|path| root.join(path).metadata().ok()?.modified().ok())
        .filter_map(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_millis() as i64)
        .max()
}

fn markdown_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".md") && name != "README.md")
        .collect()
}

fn feature_gap(root: &Path, changed: &[String]) -> Option<String> {
    let features_dir = root.join(".verify/features");
    if !features_dir.is_dir() {
        return None;
    }
    feature_map::gap(
        changed,
        &feature_map::load(&features_dir),
        &markdown_names(&root.join(".verify/knowledge-base")),
    )
}

fn claim_gap(store: &BrainStore, root: &Path, transcript: Option<&str>) -> Option<String> {
    let text = crate::claims::last_assistant_text(Path::new(transcript?))?;
    if !crate::claims::claims_passing(&text) {
        return None;
    }
    let changed = changed_paths(root)
        .iter()
        .any(|path| verify::is_code_path(path) || verify::is_proof_input(path));
    if !changed && !root.join(".verify/verify.toml").is_file() {
        return None;
    }
    let receipt = crate::receipt::receipt_for_project(store, root).ok()?;
    (receipt.verdict != crate::receipt::Verdict::Passed).then(|| {
        "Your last message says the checks pass, but the receipt for this code is not PASSED. Run `tekmor verify -- <cmd>` on the current code, or correct the claim.".to_string()
    })
}

pub fn hook_blocked(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2)
}

pub fn run_stop_gate() -> Result<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw)?;
    let input: StopInput = serde_json::from_str(&raw).unwrap_or_else(|err| {
        hook_blocked(&format!("tekmor could not read the Stop hook input ({err}); the turn is blocked until the gate can read its input"))
    });
    let transcript = input.transcript_path.clone();
    let cwd = input
        .cwd
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let config = Config::load()?;
    let store = BrainStore::open(&config.db_path)?;
    let state_file = config.db_path.with_file_name(STATE_FILE);
    let session_id = input.session_id.unwrap_or_default();
    let Some(root) = git_root(&cwd) else {
        return Ok(());
    };
    let mode = AgentBrainSettings::load(&config.home).gates.stop;
    let repo_state =
        state_file.with_file_name(format!("stop-gate-state-{}.json", repo_name(&root)));
    let keys = crate::review::default_store(&config.home);
    let outcome = match evaluate_stop(&store, &root, &keys, &session_id, &repo_state, mode)? {
        StopOutcome::Clear => claim_gap(&store, &root, transcript.as_deref()).map_or(StopOutcome::Clear, StopOutcome::Block),
        other => other,
    };
    if let Some((kind, detail)) = gate_event(&outcome, mode, &root) {
        if let Err(err) =
            crate::gate_events::record(&config.logs_dir.join("gate-events.jsonl"), kind, &detail)
        {
            tracing::warn!(error = %err, "gate event write failed");
        }
    }
    if let StopOutcome::Block(reason) = &outcome {
        hook_blocked(&named_reason(&root, reason));
    }
    if let Some(output) = hook_output(&outcome, mode, &root) {
        println!("{output}");
    }
    Ok(())
}

fn headline(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect()
}

fn gate_event(
    outcome: &StopOutcome,
    mode: GateMode,
    root: &Path,
) -> Option<(&'static str, String)> {
    match (outcome, mode) {
        (StopOutcome::Block(reason), _) => {
            Some(("stop-block", headline(&named_reason(root, reason))))
        }
        (StopOutcome::HandBack(reason), GateMode::Watch) => {
            Some(("stop-would-block", headline(&named_reason(root, reason))))
        }
        (StopOutcome::HandBack(reason), GateMode::Enforce) => {
            Some(("stop-handback", headline(&named_reason(root, reason))))
        }
        (StopOutcome::Clear | StopOutcome::Verified(_), _) => None,
    }
}

fn hook_output(outcome: &StopOutcome, mode: GateMode, root: &Path) -> Option<serde_json::Value> {
    let repo = repo_name(root);
    match (outcome, mode) {
        (StopOutcome::Clear, _) => None,
        (StopOutcome::Verified(proof), _) => Some(serde_json::json!({
            "systemMessage": format!(
                "tekmor: {repo} VERIFIED. {proof}. Details: `tekmor verify receipt`."
            )
        })),
        (StopOutcome::Block(_), _) => None,
        (StopOutcome::HandBack(reason), GateMode::Watch) => Some(serde_json::json!({
            "systemMessage": format!(
                "tekmor (watch mode): {repo} would have been blocked from finishing. {}",
                headline(reason)
            )
        })),
        (StopOutcome::HandBack(reason), GateMode::Enforce) => Some(serde_json::json!({
            "systemMessage": format!(
                "tekmor: {repo} NOT VERIFIED. The agent was blocked {STOP_BLOCK_LIMIT} times and the turn is handed back to you. {} Details: `tekmor verify receipt`.",
                headline(reason)
            )
        })),
    }
}

fn evaluate_stop(
    store: &BrainStore,
    root: &Path,
    keys: &KeyStore,
    session_id: &str,
    state_file: &Path,
    mode: GateMode,
) -> Result<StopOutcome> {
    let changed = changed_paths(root);
    let Some(edit) = last_edit_ms(root, &changed) else {
        return Ok(StopOutcome::Clear);
    };
    let gap = feature_gap(root, &changed);
    let code_changed = changed
        .iter()
        .any(|path| verify::is_code_path(path) || verify::is_proof_input(path));
    let current = if code_changed {
        Some(crate::receipt::receipt_with_keys(store, root, keys)?)
    } else {
        None
    };
    let proof = current.as_ref().and_then(receipt_proof);
    let held = current.as_ref().and_then(held_reason);
    let passed = proof.is_some();
    if (!code_changed || passed) && gap.is_none() {
        return Ok(match proof {
            Some(line) => StopOutcome::Verified(line),
            None => StopOutcome::Clear,
        });
    }
    let blocks = prior_blocks(&load_state(state_file), session_id, edit);
    let step = if code_changed && !passed && held.is_none() {
        verify::run_default_step(store, root, VerifySource::Cli)?
    } else {
        None
    };
    let reason = match (&step, &gap) {
        (Some(run), _) if !run.report.success => failure_reason(run),
        (Some(run), gap) => {
            let after = crate::receipt::receipt_with_keys(store, root, keys)?;
            match (held_reason(&after), gap) {
                (Some(held), _) => held,
                (None, Some(gap)) => gap.clone(),
                (None, None) if after.verdict == Verdict::Passed => {
                    return Ok(StopOutcome::Verified(proof_line(
                        &run.report.cmd,
                        after.code_hash.as_deref(),
                    )));
                }
                (None, None) => not_counted_reason(&run.report.cmd),
            }
        }
        (None, gap) => held
            .or_else(|| gap.clone())
            .unwrap_or_else(|| missing_verification_message(root)),
    };
    if mode == GateMode::Watch {
        return Ok(StopOutcome::HandBack(reason));
    }
    let verdict = match &gap {
        Some(_) if blocks < STOP_BLOCK_LIMIT => Decision::Block,
        Some(_) => Decision::Allow,
        None => decide(passed, blocks),
    };
    match verdict {
        Decision::Block => {
            let next = BlockState {
                session_id: session_id.to_string(),
                edit_ms: edit,
                blocks: blocks + 1,
            };
            if let Err(err) = save_state(state_file, &next) {
                eprintln!("stop-gate: could not persist block count: {err:#}");
            }
            Ok(StopOutcome::Block(reason))
        }
        Decision::Allow => Ok(StopOutcome::HandBack(reason)),
    }
}

fn missing_verification_message(root: &Path) -> String {
    let commands = verify::load_command_allowlist(root);
    if commands.is_empty() {
        return BLOCK_REASON.to_string();
    }
    let runnable = commands
        .iter()
        .map(|cmd| format!("`tekmor verify -- {cmd}`"))
        .collect::<Vec<_>>()
        .join(" or ");
    format!(
        "Verification is missing. Files changed in this repo have no passing run recorded after the last edit. Run exactly one of: {runnable}. Let it pass, then finish."
    )
}

fn failure_reason(run: &verify::StepRun) -> String {
    format!(
        "Verification failed: `{}` exited with {}. Fix the failures below, then finish again.\n\n{}",
        run.report.cmd,
        run.report.exit_code,
        run.output.trim_end()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn blocks_code_change_without_a_run_for_its_tree() {
        assert!(matches!(decide(false, 0), Decision::Block));
    }

    #[test]
    fn a_pass_claim_without_a_receipt_is_blocked_even_when_no_code_changed() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(dir.path(), Some("[verify]\ncommands = [\"true\"]\n"));
        let transcript = aux.path().join("t.jsonl");
        fs::write(
            &transcript,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"All tests pass."}]}}"#,
        )
        .unwrap();
        assert!(claim_gap(&store, dir.path(), transcript.to_str()).is_some());
    }

    #[test]
    fn keeps_blocking_below_the_block_limit() {
        assert!(matches!(
            decide(false, STOP_BLOCK_LIMIT - 1),
            Decision::Block
        ));
    }

    #[test]
    fn allows_stop_once_block_limit_is_reached_without_verification() {
        assert_eq!(decide(false, STOP_BLOCK_LIMIT), Decision::Allow);
    }

    #[test]
    fn missing_verification_names_the_allowlisted_command() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".verify")).unwrap();
        fs::write(
            dir.path().join(".verify/verify.toml"),
            "[verify]\ncommands = [\"cargo test\"]\n",
        )
        .unwrap();
        let message = missing_verification_message(dir.path());
        assert!(message.contains("`tekmor verify -- cargo test`"), "{message}");
        let bare = TempDir::new().unwrap();
        assert_eq!(missing_verification_message(bare.path()), BLOCK_REASON);
    }

    #[test]
    fn only_code_paths_need_a_verification_run() {
        assert!(verify::is_code_path("tekmor/src/engine.rs"));
        assert!(verify::is_code_path("tekmor/src/stop_gate.rs"));
        assert!(verify::is_code_path("tekmor/Cargo.toml"));
        assert!(!verify::is_code_path("tekmor/.verify/features/x.md"));
        assert!(!verify::is_code_path("tekmor/README.md"));
    }

    #[test]
    fn source_in_any_language_needs_a_verification_run() {
        assert!(verify::is_code_path("yt_dlp/utils/_utils.py"));
        assert!(verify::is_code_path("lib/clamp.js"));
        assert!(verify::is_code_path("main.go"));
        assert!(!verify::is_code_path("docs/guide.md"));
        assert!(!verify::is_code_path(".verify/verify.toml"));
        assert!(!verify::is_code_path(".claude/settings.json"));
    }

    #[test]
    fn generated_caches_are_not_code() {
        assert!(!verify::is_code_path("pkg/__pycache__/mod.cpython-315.pyc"));
        assert!(!verify::is_code_path("pkg/mod.pyc"));
        assert!(!verify::is_code_path(".pytest_cache/v/cache/lastfailed"));
        assert!(!verify::is_code_path("web/node_modules/x/index.js"));
        assert!(!verify::is_code_path("target/debug/build/x.rs"));
    }

    #[test]
    fn counts_blocks_only_for_the_same_session_and_edit() {
        let prior = BlockState {
            session_id: "s1".into(),
            edit_ms: 2_000,
            blocks: 2,
        };
        assert_eq!(prior_blocks(&prior, "s1", 2_000), 2);
        assert_eq!(prior_blocks(&prior, "s2", 2_000), 0);
        assert_eq!(prior_blocks(&prior, "s1", 2_500), 0);
    }

    #[test]
    fn reports_edit_time_for_untracked_file() {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap();
        std::fs::write(dir.path().join("note.txt"), "edit").unwrap();
        assert!(last_edit_ms(dir.path(), &changed_paths(dir.path())).is_some_and(|ms| ms > 0));
    }

    #[test]
    fn reports_no_edit_for_clean_repo() {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert_eq!(last_edit_ms(dir.path(), &changed_paths(dir.path())), None);
    }

    fn edited_repo(dir: &Path, verify_toml: Option<&str>) {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .status()
            .unwrap();
        if let Some(body) = verify_toml {
            fs::create_dir_all(dir.join(".verify")).unwrap();
            fs::write(dir.join(".verify/verify.toml"), body).unwrap();
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "add", ".verify/verify.toml"])
                .status()
                .unwrap();
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "baseline"])
                .status()
                .unwrap();
        }
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/changed.rs"), b"fn changed() {}").unwrap();
    }

    fn doc_edited_repo(dir: &Path, verify_toml: &str) {
        edited_repo(dir, Some(verify_toml));
        fs::remove_file(dir.join("src/changed.rs")).unwrap();
        fs::write(dir.join("changed.md"), b"edit").unwrap();
    }

    fn test_keys(dir: &Path) -> KeyStore {
        KeyStore::File(dir.join("review.key"))
    }

    #[test]
    fn a_verified_stop_is_allowed() {
        assert_eq!(decide(true, 0), Decision::Allow);
    }

    fn gate_store(dir: &Path) -> BrainStore {
        BrainStore::open(&dir.join("t.db")).unwrap()
    }

    fn block_reason(
        store: &BrainStore,
        root: &Path,
        session_id: &str,
        state_file: &Path,
    ) -> Result<Option<String>> {
        Ok(
            match evaluate_stop(store, root, &KeyStore::File(state_file.with_file_name("review.key")), session_id, state_file, GateMode::Enforce)? {
                StopOutcome::Block(reason) => Some(reason),
                StopOutcome::Clear | StopOutcome::Verified(_) | StopOutcome::HandBack(_) => None,
            },
        )
    }

    const FAILING_STEP: &str = "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"echo broken; exit 3\"]\n";
    const PASSING_STEP: &str = "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n";

    #[test]
    fn the_turn_is_handed_back_with_the_failure_once_the_block_limit_is_reached() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(dir.path(), Some(FAILING_STEP));
        let state = aux.path().join("state.json");
        for _ in 0..STOP_BLOCK_LIMIT {
            assert!(matches!(
                evaluate_stop(&store, dir.path(), &test_keys(aux.path()), "s1", &state, GateMode::Enforce).unwrap(),
                StopOutcome::Block(_)
            ));
        }

        let outcome = evaluate_stop(&store, dir.path(), &test_keys(aux.path()), "s1", &state, GateMode::Enforce).unwrap();
        let StopOutcome::HandBack(reason) = &outcome else {
            panic!("expected a hand-back, got {outcome:?}");
        };
        assert!(reason.contains("exit 3"));

        let output = hook_output(&outcome, GateMode::Enforce, Path::new("/work/shop")).unwrap();
        assert!(output.get("decision").is_none());
        let message = output["systemMessage"].as_str().unwrap();
        assert!(message.contains("shop NOT VERIFIED"));
        assert!(message.contains("handed back to you"));
        assert_eq!(
            gate_event(&outcome, GateMode::Enforce, Path::new("/work/shop")).map(|(kind, _)| kind),
            Some("stop-handback")
        );
    }

    #[test]
    fn watch_mode_reports_what_would_block_and_never_blocks() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(dir.path(), Some(FAILING_STEP));
        let state = aux.path().join("state.json");
        for _ in 0..=STOP_BLOCK_LIMIT {
            let outcome = evaluate_stop(&store, dir.path(), &test_keys(aux.path()), "s1", &state, GateMode::Watch).unwrap();
            assert!(matches!(outcome, StopOutcome::HandBack(_)), "{outcome:?}");
            let output = hook_output(&outcome, GateMode::Watch, Path::new("/work/shop")).unwrap();
            assert!(output.get("decision").is_none());
            assert!(output["systemMessage"]
                .as_str()
                .unwrap()
                .contains("would have been blocked"));
            assert_eq!(
                gate_event(&outcome, GateMode::Watch, Path::new("/work/shop"))
                    .map(|(kind, _)| kind),
                Some("stop-would-block")
            );
        }
        assert!(!state.exists());
    }

    #[test]
    fn a_passing_check_on_changed_code_ends_the_turn_with_a_receipt_line() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(dir.path(), Some(PASSING_STEP));
        let state = aux.path().join("state.json");

        let outcome = evaluate_stop(&store, dir.path(), &test_keys(aux.path()), "s1", &state, GateMode::Enforce).unwrap();
        let StopOutcome::Verified(line) = &outcome else {
            panic!("expected VERIFIED, got {outcome:?}");
        };
        assert!(line.starts_with("`sh -c true` passed on code "), "{line}");
        let hash_prefix = line.rsplit(' ').next().unwrap();
        assert_eq!(hash_prefix.len(), 12, "{line}");
        let output = hook_output(&outcome, GateMode::Enforce, Path::new("/work/shop")).unwrap();
        let message = output["systemMessage"].as_str().unwrap();
        assert!(message.contains("shop VERIFIED"), "{message}");
        assert!(message.contains("`sh -c true` passed"), "{message}");
        assert_eq!(
            gate_event(&outcome, GateMode::Enforce, Path::new("/work/shop")),
            None
        );
    }

    #[test]
    fn a_clean_or_docs_only_tree_prints_nothing() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        doc_edited_repo(dir.path(), PASSING_STEP);
        let state = aux.path().join("state.json");

        let outcome = evaluate_stop(&store, dir.path(), &test_keys(aux.path()), "s1", &state, GateMode::Enforce).unwrap();
        assert_eq!(outcome, StopOutcome::Clear);
        assert_eq!(
            hook_output(&outcome, GateMode::Enforce, Path::new("/work/shop")),
            None
        );
    }

    #[test]
    fn passing_default_step_lets_the_stop_through_and_is_recorded() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(
            dir.path(),
            Some("[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n"),
        );
        let state = aux.path().join("state.json");
        assert_eq!(
            block_reason(&store, dir.path(), "s1", &state).unwrap(),
            None
        );
        assert_eq!(store.list_verify_outcomes(10).unwrap().len(), 1);
    }

    #[test]
    fn failing_default_step_blocks_with_its_command_and_output() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(
            dir.path(),
            Some("[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"echo broken; exit 3\"]\n"),
        );
        let state = aux.path().join("state.json");
        let reason = block_reason(&store, dir.path(), "s1", &state)
            .unwrap()
            .expect("failing step must block");
        assert!(reason.contains("exit 3"));
        assert!(reason.contains("broken"));
    }

    #[test]
    fn stop_is_allowed_after_the_retry_limit_while_the_step_still_fails() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(
            dir.path(),
            Some("[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"exit 1\"]\n"),
        );
        let state = aux.path().join("state.json");
        for _ in 0..STOP_BLOCK_LIMIT {
            assert!(block_reason(&store, dir.path(), "s1", &state)
                .unwrap()
                .is_some());
        }
        assert_eq!(
            block_reason(&store, dir.path(), "s1", &state).unwrap(),
            None
        );
    }

    #[test]
    fn edits_without_a_default_step_still_block() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(dir.path(), None);
        let state = aux.path().join("state.json");
        assert_eq!(
            block_reason(&store, dir.path(), "s1", &state).unwrap(),
            Some(BLOCK_REASON.to_string())
        );
    }

    #[test]
    fn doc_only_edit_runs_no_verify_step() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        doc_edited_repo(
            dir.path(),
            "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"exit 1\"]\n",
        );
        let state = aux.path().join("state.json");
        assert_eq!(
            block_reason(&store, dir.path(), "s1", &state).unwrap(),
            None
        );
        assert!(store.list_verify_outcomes(10).unwrap().is_empty());
    }

    #[test]
    fn code_tree_hash_moves_on_any_non_doc_file() {
        let dir = TempDir::new().unwrap();
        edited_repo(dir.path(), None);
        let before = verify::code_tree_hash(dir.path()).unwrap();
        fs::write(dir.path().join(".pass"), "").unwrap();
        assert_ne!(verify::code_tree_hash(dir.path()).unwrap(), before);
    }

    #[test]
    fn code_tree_hash_ignores_generated_output() -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        edited_repo(dir.path(), None);
        let before = verify::code_tree_hash(dir.path()).ok_or_else(|| anyhow::anyhow!("no hash"))?;
        fs::create_dir_all(dir.path().join("src/__pycache__"))?;
        fs::write(dir.path().join("src/__pycache__/m.cpython-315.pyc"), b"bytecode")?;
        fs::write(dir.path().join("cached.pyc"), b"bytecode")?;
        assert_eq!(verify::code_tree_hash(dir.path()).as_deref(), Some(before.as_str()));
        Ok(())
    }

    #[test]
    fn code_tree_hash_moves_on_proof_input_edits() {
        let dir = TempDir::new().unwrap();
        edited_repo(dir.path(), Some("[verify]\ncommands = [\"true\"]\n"));
        let before = verify::code_tree_hash(dir.path()).unwrap();
        fs::write(dir.path().join(".verify/verify.toml"), "[verify]\ncommands = [\"true\", \"echo ok\"]\n").unwrap();
        assert_ne!(verify::code_tree_hash(dir.path()).unwrap(), before);
    }

    #[test]
    fn code_tree_hash_moves_on_code_edits_and_not_on_doc_edits() {
        let dir = TempDir::new().unwrap();
        edited_repo(dir.path(), None);
        let before = verify::code_tree_hash(dir.path()).unwrap();
        fs::write(dir.path().join("changed.md"), b"doc edit").unwrap();
        assert_eq!(verify::code_tree_hash(dir.path()).unwrap(), before);
        fs::write(dir.path().join("src/changed.rs"), b"fn edited() {}").unwrap();
        assert_ne!(verify::code_tree_hash(dir.path()).unwrap(), before);
    }

    #[test]
    fn a_proof_input_edit_forces_a_fresh_run() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(
            dir.path(),
            Some("[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n"),
        );
        let state = aux.path().join("state.json");
        assert_eq!(block_reason(&store, dir.path(), "s1", &state).unwrap(), None);
        assert_eq!(store.list_verify_outcomes(10).unwrap().len(), 1);
        fs::write(
            dir.path().join(".verify/verify.toml"),
            "[verify]\ncommands = [\"sh -c ...\", \"true\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n",
        )
        .unwrap();
        assert_eq!(block_reason(&store, dir.path(), "s1", &state).unwrap(), None);
        assert_eq!(store.list_verify_outcomes(10).unwrap().len(), 2);
    }

    #[test]
    fn code_edit_after_a_passing_run_needs_another_run() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(
            dir.path(),
            Some("[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n"),
        );
        let state = aux.path().join("state.json");
        assert_eq!(
            block_reason(&store, dir.path(), "s1", &state).unwrap(),
            None
        );
        fs::write(dir.path().join("src/changed.rs"), b"fn changed_again() {}").unwrap();
        assert_eq!(
            block_reason(&store, dir.path(), "s1", &state).unwrap(),
            None
        );
        assert_eq!(store.list_verify_outcomes(10).unwrap().len(), 2);
    }

    #[test]
    fn unclaimed_code_blocks_even_after_verification_passes() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(
            dir.path(),
            Some("[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n"),
        );
        fs::create_dir_all(dir.path().join(".verify/features")).unwrap();
        let state = aux.path().join("state.json");
        let reason = block_reason(&store, dir.path(), "s1", &state)
            .unwrap()
            .expect("unclaimed code must block");
        assert!(reason.contains("`src/changed.rs`"));
    }

    #[test]
    fn verified_stop_passes_once_the_owning_feature_is_updated() {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        edited_repo(
            dir.path(),
            Some("[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n"),
        );
        fs::create_dir_all(dir.path().join(".verify/features")).unwrap();
        fs::write(
            dir.path().join(".verify/features/gate.md"),
            b"# gate\n\n## Code\n\n- `src/changed.rs`\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join(".verify/knowledge-base")).unwrap();
        fs::write(dir.path().join(".verify/knowledge-base/gate.md"), b"# gate").unwrap();
        let state = aux.path().join("state.json");
        assert_eq!(
            block_reason(&store, dir.path(), "s1", &state).unwrap(),
            None
        );
    }

    const MATRIX_VERIFY: &str = "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"true\"]\n";
    const MATRIX_FAILING_VERIFY: &str = "[verify]\ncommands = [\"sh -c ...\"]\n\n[verify.default]\ncommand = [\"sh\", \"-c\", \"exit 1\"]\n";
    const GATE_MAP: &str = ".verify/features/gate.md";
    const GATE_KB: &str = ".verify/knowledge-base/gate.md";

    struct MatrixRepo {
        dir: TempDir,
        aux: TempDir,
        store: BrainStore,
    }

    fn write_file(root: &Path, path: &str, text: &str) {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn git_in(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn committed_repo(verify_toml: &str, baseline: Vec<(&str, String)>) -> MatrixRepo {
        let dir = TempDir::new().unwrap();
        let aux = TempDir::new().unwrap();
        let store = gate_store(aux.path());
        git_in(dir.path(), &["init", "-q"]);
        write_file(dir.path(), ".verify/verify.toml", verify_toml);
        for (path, text) in &baseline {
            write_file(dir.path(), path, text);
        }
        git_in(dir.path(), &["add", "-A"]);
        git_in(
            dir.path(),
            &[
                "-c",
                "user.name=gate",
                "-c",
                "user.email=gate@test",
                "commit",
                "-qm",
                "baseline",
            ],
        );
        MatrixRepo { dir, aux, store }
    }

    fn evaluate(repo: &MatrixRepo) -> Option<String> {
        let state = repo.aux.path().join("state.json");
        block_reason(&repo.store, repo.dir.path(), "s1", &state).unwrap()
    }

    fn stop_after(repo: &MatrixRepo, changes: Vec<(&str, String)>) -> Option<String> {
        for (path, text) in &changes {
            write_file(repo.dir.path(), path, text);
        }
        evaluate(repo)
    }

    fn reviewed_repo() -> MatrixRepo {
        committed_repo(
            MATRIX_VERIFY,
            vec![(".verify/review.toml", "risky = [\"src/\"]\n".to_string())],
        )
    }

    #[test]
    fn risky_path_change_with_a_passing_run_but_no_review_blocks_with_the_review_gap() -> Result<()> {
        let repo = reviewed_repo();
        let first = stop_after(&repo, vec![("src/a.py", "x = 1\n".to_string())])
            .ok_or_else(|| anyhow::anyhow!("must block"))?;
        assert!(first.contains("without an independent pass review: src/a.py"), "{first}");
        assert!(first.contains("tekmor verify review --verdict pass"), "{first}");
        let again = evaluate(&repo).ok_or_else(|| anyhow::anyhow!("must still block"))?;
        assert!(again.contains("without an independent pass review"), "{again}");
        assert_eq!(repo.store.list_verify_outcomes(10)?.len(), 1);
        Ok(())
    }

    #[test]
    fn risky_path_change_with_a_pass_review_is_verified() -> Result<()> {
        let repo = reviewed_repo();
        write_file(repo.dir.path(), "src/a.py", "x = 1\n");
        let hash = verify::code_tree_hash(repo.dir.path())
            .ok_or_else(|| anyhow::anyhow!("no code hash"))?;
        crate::review::record(repo.dir.path(), &test_keys(repo.aux.path()), &hash, "pass", "reviewer", "ok")?;
        let state = repo.aux.path().join("state.json");
        let outcome = evaluate_stop(
            &repo.store,
            repo.dir.path(),
            &test_keys(repo.aux.path()),
            "s1",
            &state,
            GateMode::Enforce,
        )?;
        assert!(matches!(outcome, StopOutcome::Verified(_)), "{outcome:?}");
        Ok(())
    }

    fn feature_map(claims: &[&str]) -> String {
        let mut text = String::from("# feature\n\n## Code\n\n");
        for claim in claims {
            text.push_str(&format!("- `{claim}`\n"));
        }
        text.push_str("\n## How to get to it\n");
        text
    }

    fn gate_baseline() -> Vec<(&'static str, String)> {
        vec![
            (GATE_MAP, feature_map(&["src/a.rs"])),
            (GATE_KB, "# gate".to_string()),
        ]
    }

    #[test]
    fn matrix_no_change_allows_the_stop() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        assert_eq!(stop_after(&repo, vec![]), None);
    }

    #[test]
    fn matrix_docs_only_change_allows_the_stop() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        assert_eq!(
            stop_after(&repo, vec![("README.md", "docs".to_string())]),
            None
        );
    }

    #[test]
    fn matrix_unclaimed_code_is_named_in_the_block() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        let reason = stop_after(&repo, vec![("src/b.rs", "fn b() {}".to_string())]).unwrap();
        assert!(reason.contains("`src/b.rs`"));
    }

    #[test]
    fn matrix_owned_code_without_its_map_update_is_blocked() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        let reason = stop_after(&repo, vec![("src/a.rs", "fn a() {}".to_string())]).unwrap();
        assert!(reason.contains("`gate.md`"));
    }

    #[test]
    fn matrix_owned_code_with_map_but_no_entry_is_blocked() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        let reason = stop_after(
            &repo,
            vec![
                ("src/a.rs", "fn a() {}".to_string()),
                (GATE_MAP, feature_map(&["src/a.rs", "src/c.rs"])),
            ],
        )
        .unwrap();
        assert!(reason.contains("`gate.md`"));
    }

    #[test]
    fn matrix_owned_code_with_map_and_entry_passes() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        let changes = vec![
            ("src/a.rs", "fn a() {}".to_string()),
            (GATE_MAP, feature_map(&["src/a.rs", "src/c.rs"])),
            (GATE_KB, "# gate, updated".to_string()),
        ];
        assert_eq!(stop_after(&repo, changes), None);
    }

    #[test]
    fn matrix_shared_file_needs_every_owner_updated() {
        let repo = committed_repo(
            MATRIX_VERIFY,
            vec![
                (".verify/features/one.md", feature_map(&["src/shared.rs"])),
                (".verify/features/two.md", feature_map(&["src"])),
                (".verify/knowledge-base/one.md", "# one".to_string()),
                (".verify/knowledge-base/two.md", "# two".to_string()),
            ],
        );
        let reason = stop_after(
            &repo,
            vec![
                ("src/shared.rs", "fn s() {}".to_string()),
                (
                    ".verify/features/one.md",
                    feature_map(&["src/shared.rs"]) + "\n",
                ),
                (
                    ".verify/knowledge-base/one.md",
                    "# one, updated".to_string(),
                ),
            ],
        )
        .unwrap();
        assert!(reason.contains("`two.md`"));
        assert!(!reason.contains("`one.md`"));
    }

    #[test]
    fn matrix_shared_file_with_every_owner_updated_passes() {
        let repo = committed_repo(
            MATRIX_VERIFY,
            vec![
                (".verify/features/one.md", feature_map(&["src/shared.rs"])),
                (".verify/features/two.md", feature_map(&["src"])),
                (".verify/knowledge-base/one.md", "# one".to_string()),
                (".verify/knowledge-base/two.md", "# two".to_string()),
            ],
        );
        let changes = vec![
            ("src/shared.rs", "fn s() {}".to_string()),
            (
                ".verify/features/one.md",
                feature_map(&["src/shared.rs"]) + "\n",
            ),
            (
                ".verify/knowledge-base/one.md",
                "# one, updated".to_string(),
            ),
            (".verify/features/two.md", feature_map(&["src"]) + "\n"),
            (
                ".verify/knowledge-base/two.md",
                "# two, updated".to_string(),
            ),
        ];
        assert_eq!(stop_after(&repo, changes), None);
    }

    #[test]
    fn matrix_new_feature_for_new_code_passes_when_complete() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        let changes = vec![
            ("src/new.rs", "fn n() {}".to_string()),
            (".verify/features/new.md", feature_map(&["src/new.rs"])),
            (".verify/knowledge-base/new.md", "# new".to_string()),
        ];
        assert_eq!(stop_after(&repo, changes), None);
    }

    #[test]
    fn matrix_new_feature_without_its_entry_is_blocked() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        let reason = stop_after(
            &repo,
            vec![
                ("src/new.rs", "fn n() {}".to_string()),
                (".verify/features/new.md", feature_map(&["src/new.rs"])),
            ],
        )
        .unwrap();
        assert!(reason.contains("`new.md`"));
    }

    #[test]
    fn matrix_feature_without_an_entry_on_disk_is_named() {
        let repo = committed_repo(
            MATRIX_VERIFY,
            vec![
                (GATE_MAP, feature_map(&["src/a.rs"])),
                (GATE_KB, "# gate".to_string()),
                (".verify/features/other.md", feature_map(&["src/other.rs"])),
            ],
        );
        let changes = vec![
            ("src/a.rs", "fn a() {}".to_string()),
            (GATE_MAP, feature_map(&["src/a.rs"]) + "\n"),
            (GATE_KB, "# gate, updated".to_string()),
        ];
        let reason = stop_after(&repo, changes).unwrap();
        assert!(reason.contains("`other.md`"));
        assert!(reason.contains("no knowledge-base entry"));
    }

    #[test]
    fn matrix_new_untracked_directory_names_each_file() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        let reason = stop_after(&repo, vec![("src/newmod/x.rs", "fn x() {}".to_string())]).unwrap();
        assert!(reason.contains("`src/newmod/x.rs`"));
    }

    #[test]
    fn matrix_a_claim_is_a_path_prefix_not_a_string_prefix() {
        let repo = committed_repo(
            MATRIX_VERIFY,
            vec![
                (GATE_MAP, feature_map(&["src"])),
                (GATE_KB, "# gate".to_string()),
            ],
        );
        let reason = stop_after(&repo, vec![("srcx/file.rs", "fn f() {}".to_string())]).unwrap();
        assert!(reason.contains("`srcx/file.rs`"));
    }

    #[test]
    fn matrix_paths_outside_the_code_section_claim_nothing() {
        let repo = committed_repo(
            MATRIX_VERIFY,
            vec![
                (
                    GATE_MAP,
                    "# gate\n\n## Gotchas\n\n- `src/a.rs`\n".to_string(),
                ),
                (GATE_KB, "# gate".to_string()),
            ],
        );
        let reason = stop_after(&repo, vec![("src/a.rs", "fn a() {}".to_string())]).unwrap();
        assert!(reason.contains("`src/a.rs`"));
        assert!(reason.contains("outside every feature's"));
    }

    #[test]
    fn matrix_failing_verification_blocks_even_when_owned_work_is_complete() {
        let repo = committed_repo(MATRIX_FAILING_VERIFY, gate_baseline());
        let changes = vec![
            ("src/a.rs", "fn a() {}".to_string()),
            (GATE_MAP, feature_map(&["src/a.rs"]) + "\n"),
            (GATE_KB, "# gate, updated".to_string()),
        ];
        let reason = stop_after(&repo, changes).unwrap();
        assert!(reason.contains("exited with 1"));
    }

    #[test]
    fn matrix_stop_is_allowed_after_the_block_limit_while_code_stays_unclaimed() {
        let repo = committed_repo(MATRIX_VERIFY, gate_baseline());
        let first = stop_after(&repo, vec![("src/b.rs", "fn b() {}".to_string())]);
        assert!(first.is_some());
        for _ in 1..STOP_BLOCK_LIMIT {
            assert!(evaluate(&repo).is_some());
        }
        assert_eq!(evaluate(&repo), None);
    }

    #[test]
    fn hook_outside_a_repo_checks_no_child_repos() {
        let root = TempDir::new().unwrap();
        let changed = root.path().join("changed");
        fs::create_dir_all(&changed).unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&changed)
            .args(["init", "-q"])
            .status()
            .unwrap();
        fs::write(changed.join("a.rs"), "fn a() {}").unwrap();
        assert!(git_root(root.path()).is_none());
        assert_eq!(
            git_root(&changed).map(|root| repo_name(&root)),
            Some("changed".to_string())
        );
    }

    #[test]
    fn blocking_reason_names_the_repo() {
        let reason = named_reason(Path::new("/work/agentrun"), "Verification is missing.");
        assert_eq!(reason, "Repo agentrun is blocked: Verification is missing.");
    }
}
