#![cfg(target_os = "macos")]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

#[test]
fn permission_hook_notifies_even_when_tmux_reports_a_focused_client() {
    let root = tempfile::tempdir().unwrap();
    let notification_log = root.path().join("notification.log");
    // A focused tmux client can still be on another macOS Space. Stub both
    // external commands so this regression never touches live tmux or the OS.
    for (name, script) in [
        (
            "tmux",
            r#"#!/bin/sh
case "$1 $2" in
  'show -g') printf '@sidebar_notifications_events notification\n' ;;
  'list-clients -F') printf 'attached,focused,UTF-8|%%7\n' ;;
esac
exit 0
"#,
        ),
        (
            "osascript",
            r#"#!/bin/sh
case "$2" in
  'return 0') exit 0 ;;
  *frontmost*) printf '%s\n' "${SIDEBAR_TEST_FRONTMOST:-Finder}" ;;
  *) printf 'sent\n' >> "$SIDEBAR_TEST_NOTIFICATION_LOG" ;;
esac
"#,
        ),
    ] {
        let path = root.path().join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_tmux-agent-sidebar"))
        .args(["hook", "codex", "permission-request"])
        .env_clear()
        .env("PATH", root.path())
        .env("TMPDIR", root.path())
        .env("TMUX", root.path().join("unused-test-socket"))
        .env("TMUX_PANE", "%7")
        .env("SIDEBAR_TEST_NOTIFICATION_LOG", &notification_log)
        .env("SIDEBAR_TEST_FRONTMOST", "Finder")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"test-session","turn_id":"test-turn","tool_name":"shell"}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(notification_log).unwrap_or_default(),
        "sent\n",
        "a background Ghostty/Space must not suppress an actionable notification"
    );
}

#[test]
fn permission_hook_stays_silent_when_focused_in_frontmost_ghostty() {
    let root = tempfile::tempdir().unwrap();
    let notification_log = root.path().join("notification.log");
    for (name, script) in [
        (
            "tmux",
            r#"#!/bin/sh
case "$1 $2" in
  'show -g') printf '@sidebar_notifications_events notification\n' ;;
  'list-clients -F') printf 'attached,focused,UTF-8|%%7\n' ;;
esac
exit 0
"#,
        ),
        (
            "osascript",
            r#"#!/bin/sh
case "$2" in
  'return 0') exit 0 ;;
  *frontmost*) printf 'Ghostty\n' ;;
  *) printf 'sent\n' >> "$SIDEBAR_TEST_NOTIFICATION_LOG" ;;
esac
"#,
        ),
    ] {
        let path = root.path().join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_tmux-agent-sidebar"))
        .args(["hook", "codex", "permission-request"])
        .env_clear()
        .env("PATH", root.path())
        .env("TMPDIR", root.path())
        .env("TMUX", root.path().join("unused-test-socket"))
        .env("TMUX_PANE", "%7")
        .env("SIDEBAR_TEST_NOTIFICATION_LOG", &notification_log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"test-session","turn_id":"test-turn","tool_name":"shell"}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(notification_log).unwrap_or_default(),
        "",
        "the focused pane in frontmost Ghostty should not notify"
    );
}
