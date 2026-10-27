//! Maintenance CLI must be safe even before history exists or when it is damaged.
use std::fs;
use std::process::{Command, Output};

fn run(root: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tmux-agent-sidebar"))
        .args(args)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .output()
        .unwrap()
}
#[test]
fn status_does_not_initialize_history_and_purge_requires_explicit_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let status = run(temp.path(), &["usage-history", "status"]);
    assert!(status.status.success());
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["state"], "not initialized");
    assert!(!temp.path().join("data").exists());
    let purge = run(temp.path(), &["usage-history", "purge", "--all"]);
    assert!(!purge.status.success());
    assert!(!temp.path().join("data").exists());
}
#[test]
fn corrupt_store_is_reported_without_replacement_or_output_creation() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("data/tmux-agent-sidebar");
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("usage.sqlite3");
    fs::write(&db, b"not a sqlite database").unwrap();
    let status = run(temp.path(), &["usage-history", "status"]);
    assert!(!status.status.success());
    assert!(!status.stderr.is_empty());
    assert_eq!(fs::read(&db).unwrap(), b"not a sqlite database");
    let dest = temp.path().join("backup.sqlite3");
    let backup = run(
        temp.path(),
        &["usage-history", "backup", dest.to_str().unwrap()],
    );
    assert!(!backup.status.success());
    assert!(!dest.exists());
    assert_eq!(fs::read(db).unwrap(), b"not a sqlite database");
}
