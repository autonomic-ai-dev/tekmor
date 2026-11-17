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
