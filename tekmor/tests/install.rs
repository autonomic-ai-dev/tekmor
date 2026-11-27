use std::fs;
use std::process::Command;

#[test]
fn install_rejects_an_unknown_host_flag_and_writes_nothing() {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");

    let output = Command::new(env!("CARGO_BIN_EXE_tekmor"))
        .args(["install", "--cursor"])
        .current_dir(project.path())
        .env("TEKMOR_HOME", home.path())
        .output()
        .expect("run tekmor");

    assert!(!output.status.success(), "unknown flag must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown install option `--cursor`"), "{stderr}");
    assert!(
        !project.path().join(".claude").exists(),
        "a rejected install must not write Claude Code settings"
    );
    assert!(fs::read_dir(project.path()).unwrap().next().is_none());
}

#[test]
fn install_registers_the_session_start_bash_and_edit_gates() {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    Command::new("git").arg("-C").arg(project.path()).args(["init", "-q"]).status().expect("git init");

    let output = Command::new(env!("CARGO_BIN_EXE_tekmor"))
        .args(["install", "--quiet"])
        .current_dir(project.path())
        .env("TEKMOR_HOME", home.path())
        .output()
        .expect("run tekmor");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let raw = fs::read_to_string(project.path().join(".claude/settings.json")).expect("settings");
    let settings: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let hooks = &settings["hooks"];
    let commands = |event: &str| -> Vec<String> {
        hooks[event]
            .as_array()
            .expect("event")
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().expect("hooks").iter())
            .map(|hook| hook["command"].as_str().expect("command").to_string())
            .collect()
    };
    assert!(commands("SessionStart").iter().any(|c| c.ends_with(" session-start")), "{raw}");
    assert!(commands("Stop").iter().any(|c| c.ends_with(" stop-gate")), "{raw}");
    let pre = commands("PreToolUse");
    assert!(pre.iter().any(|c| c.ends_with(" bash-gate")), "{raw}");
    assert!(pre.iter().any(|c| c.ends_with(" edit-gate")), "{raw}");
    let edit_matcher = hooks["PreToolUse"][1]["matcher"].as_str().expect("matcher");
    assert!(edit_matcher.contains("Write") && edit_matcher.contains("Edit"), "{raw}");
}
