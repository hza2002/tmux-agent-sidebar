use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::process::Command;

#[test]
fn unchanged_layout_event_skips_busy_lifecycle_lock() {
    let temp = tempfile::tempdir().unwrap();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(temp.path().join(".probe.tmux-agent-sidebar.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let run = |width: &str, state: &str| {
        Command::new(env!("CARGO_BIN_EXE_tmux-agent-sidebar"))
            .env(
                "TMUX",
                format!("{},1,0", temp.path().join("probe").display()),
            )
            .args(["fit-width", "@28", width, "55", state])
            .output()
            .unwrap()
    };
    let idle = run("223", "223,55,0,1793071153265");
    assert!(
        idle.status.success(),
        "{}",
        String::from_utf8_lossy(&idle.stderr)
    );
    // Genuine resize and deferred zoom repair still require serialization.
    for (width, state) in [
        ("224", "223,55,0,1"),
        ("223", "223,55,1,1"),
        ("223", "invalid"),
    ] {
        let repair = run(width, state);
        assert!(!repair.status.success());
        assert!(String::from_utf8_lossy(&repair.stderr).contains("lifecycle lock"));
    }
}
