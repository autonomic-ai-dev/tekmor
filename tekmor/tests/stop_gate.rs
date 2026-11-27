use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;

fn run_stop_gate(home: &Path, cwd: &Path, session_id: &str) -> std::process::Output {
    let payload = serde_json::json!({
        "cwd": cwd,
        "session_id": session_id,
        "transcript_path": cwd.join("transcript.jsonl"),
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_tekmor"))
        .arg("stop-gate")
        .env("TEKMOR_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tekmor stop-gate");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("write hook payload");
    child.wait_with_output().expect("wait for stop-gate")
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn gate_events(home: &Path) -> Vec<Value> {
    let path = home.join("logs/gate-events.jsonl");
    fs::read_to_string(path)
        .expect("gate-events.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("event is json"))
        .collect()
}

#[test]
fn every_stop_decision_is_logged_with_a_reason_code() {
    let home = TempDir::new().expect("temp tekmor home");
    let repo = TempDir::new().expect("temp repo");
    git(repo.path(), &["init", "-q"]);
    fs::create_dir_all(repo.path().join(".verify")).expect("mkdir .verify");
    fs::write(
        repo.path().join(".verify/verify.toml"),
        "[verify]\ncommands = [\"true\"]\n",
    )
    .expect("write verify.toml");
    git(repo.path(), &["add", "-A"]);
    git(
        repo.path(),
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

    let clean = run_stop_gate(home.path(), repo.path(), "s-test");
    assert!(clean.status.success(), "clean stop must pass: {clean:?}");

    fs::create_dir_all(repo.path().join("src")).expect("mkdir src");
    fs::write(repo.path().join("src/changed.rs"), "fn changed() {}").expect("edit code");
    let blocked = run_stop_gate(home.path(), repo.path(), "s-test");
    assert_eq!(blocked.status.code(), Some(2), "edited stop must block");

    let events = gate_events(home.path());
    assert_eq!(events.len(), 2, "{events:?}");

    let clear = &events[0];
    assert_eq!(clear["kind"], "stop-clear", "{clear}");
    assert_eq!(clear["reason"], "clear", "{clear}");
    assert_eq!(clear["session"], "s-test", "{clear}");
    assert_eq!(
        clear["repo"].as_str(),
        repo.path().file_name().unwrap().to_str(),
        "{clear}"
    );

    let block = &events[1];
    assert_eq!(block["kind"], "stop-block", "{block}");
    assert_eq!(block["reason"], "check-missing", "{block}");
    assert_eq!(block["session"], "s-test", "{block}");
    assert_eq!(
        block["repo"].as_str(),
        repo.path().file_name().unwrap().to_str(),
        "{block}"
    );
}

#[test]
fn an_unreadable_run_store_blocks_the_turn_instead_of_ending_it() {
    let home = TempDir::new().expect("temp tekmor home");
    fs::write(home.path().join("data"), "not a directory").expect("plant a file where the store directory goes");
    let repo = TempDir::new().expect("temp repo");
    git(repo.path(), &["init", "-q"]);
    let out = run_stop_gate(home.path(), repo.path(), "s-store");
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("run store"), "{out:?}");
}
