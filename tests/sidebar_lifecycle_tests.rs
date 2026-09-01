use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

fn tmux(root: &std::path::Path, args: &[&str]) -> Output {
    Command::new("tmux")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("TMUX_TMPDIR", root)
        .arg("-S")
        .arg(root.join("tmux.sock"))
        .args(args)
        .output()
        .expect("run isolated tmux command")
}

fn stdout(root: &std::path::Path, args: &[&str]) -> String {
    let output = tmux(root, args);
    assert!(
        output.status.success(),
        "tmux {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("tmux output is UTF-8")
        .trim()
        .to_string()
}

#[test]
#[ignore = "requires local tmux"]
fn singleton_swaps_with_empty_slots_without_reflowing_visited_windows() {
    let root = tempfile::tempdir().unwrap();
    stdout(
        root.path(),
        &[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "sidebar-lifecycle",
            "-x",
            "100",
            "-y",
            "30",
            "sleep 1000",
        ],
    );
    let _cleanup = scopeguard::guard(root.path().to_path_buf(), |root| {
        let _ = tmux(&root, &["kill-session", "-t", "sidebar-lifecycle"]);
    });
    stdout(
        root.path(),
        &[
            "new-window",
            "-d",
            "-t",
            "sidebar-lifecycle",
            "-n",
            "second",
            "sleep 1000",
        ],
    );

    let socket = stdout(root.path(), &["display-message", "-p", "#{socket_path}"]);
    let server_pid = stdout(root.path(), &["display-message", "-p", "#{pid}"]);
    let tmux_env = format!("{socket},{server_pid},0");
    let first_window = stdout(
        root.path(),
        &[
            "display-message",
            "-p",
            "-t",
            "sidebar-lifecycle:0",
            "#{window_id}",
        ],
    );
    let second_window = stdout(
        root.path(),
        &[
            "display-message",
            "-p",
            "-t",
            "sidebar-lifecycle:1",
            "#{window_id}",
        ],
    );
    let first_pane = stdout(
        root.path(),
        &["display-message", "-p", "-t", &first_window, "#{pane_id}"],
    );
    let second_pane = stdout(
        root.path(),
        &["display-message", "-p", "-t", &second_window, "#{pane_id}"],
    );
    let binary = env!("CARGO_BIN_EXE_tmux-agent-sidebar");
    let run_sidebar = |args: &[&str]| {
        let status = Command::new(binary)
            .env("TMUX", &tmux_env)
            .env("TMUX_TMPDIR", root.path())
            .args(args)
            .status()
            .expect("run sidebar lifecycle command");
        assert!(status.success(), "sidebar command failed: {args:?}");
    };

    run_sidebar(&["toggle", &first_window, "/tmp", &first_pane]);
    thread::sleep(Duration::from_millis(100));
    run_sidebar(&["toggle", &second_window, "/tmp", &second_pane]);

    let after_first_move = stdout(
        root.path(),
        &[
            "list-panes",
            "-a",
            "-F",
            "#{window_id}|#{pane_id}|#{@pane_role}|#{pane_pid}|#{pane_width}",
        ],
    );
    assert_eq!(after_first_move.matches("|sidebar|").count(), 1);
    assert_eq!(after_first_move.matches("|sidebar-slot|0|35").count(), 1);
    assert!(after_first_move.contains(&format!("{first_window}|{first_pane}||")));
    assert!(after_first_move.contains(&format!("{second_window}|{second_pane}||")));

    let first_width = stdout(
        root.path(),
        &["display-message", "-p", "-t", &first_pane, "#{pane_width}"],
    );
    let second_width = stdout(
        root.path(),
        &["display-message", "-p", "-t", &second_pane, "#{pane_width}"],
    );
    run_sidebar(&["toggle", &first_window, "/tmp", &first_pane]);
    assert_eq!(
        stdout(
            root.path(),
            &["display-message", "-p", "-t", &first_pane, "#{pane_width}"],
        ),
        first_width
    );
    assert_eq!(
        stdout(
            root.path(),
            &["display-message", "-p", "-t", &second_pane, "#{pane_width}"],
        ),
        second_width
    );

    stdout(root.path(), &["resize-pane", "-Z", "-t", &second_pane]);
    assert_eq!(
        stdout(
            root.path(),
            &[
                "display-message",
                "-p",
                "-t",
                &second_window,
                "#{window_zoomed_flag}"
            ],
        ),
        "1"
    );
    run_sidebar(&["toggle", &second_window, "/tmp", &second_pane]);
    assert_eq!(
        stdout(
            root.path(),
            &[
                "display-message",
                "-p",
                "-t",
                &second_window,
                "#{window_zoomed_flag}"
            ],
        ),
        "0"
    );
    run_sidebar(&["toggle", &first_window, "/tmp", &first_pane]);
    assert_eq!(
        stdout(
            root.path(),
            &[
                "display-message",
                "-p",
                "-t",
                &second_window,
                "#{window_zoomed_flag}"
            ],
        ),
        "1"
    );

    run_sidebar(&["close", &first_window, &first_pane]);
    let final_panes = stdout(
        root.path(),
        &["list-panes", "-a", "-F", "#{@pane_role}|#{pane_width}"],
    );
    assert!(!final_panes.contains("sidebar"));
    assert!(final_panes.lines().all(|line| line == "|100"));
    assert_eq!(
        stdout(
            root.path(),
            &["show-option", "-gqv", "@agent_sidebar_enabled"],
        ),
        "off"
    );

    run_sidebar(&["toggle", &first_window, "/tmp", &first_pane]);
    stdout(root.path(), &["kill-pane", "-t", &first_pane]);
    run_sidebar(&["maintain"]);
    let windows = stdout(root.path(), &["list-windows", "-a", "-F", "#{window_id}"]);
    assert!(!windows.lines().any(|window| window == first_window));
    assert!(
        stdout(
            root.path(),
            &["list-panes", "-t", &second_window, "-F", "#{@pane_role}"],
        )
        .lines()
        .any(|role| role == "sidebar")
    );

    stdout(root.path(), &["kill-pane", "-t", &second_pane]);
    run_sidebar(&["maintain"]);
    assert!(!tmux(root.path(), &["has-session"]).status.success());
}

#[test]
#[ignore = "requires local tmux"]
fn singleton_preserves_user_adjusted_sidebar_geometry() {
    let root = tempfile::tempdir().unwrap();
    stdout(
        root.path(),
        &[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "sidebar-split",
            "-x",
            "100",
            "-y",
            "30",
            "sleep 1000",
        ],
    );
    let _cleanup = scopeguard::guard(root.path().to_path_buf(), |root| {
        let _ = tmux(&root, &["kill-session", "-t", "sidebar-split"]);
    });
    stdout(
        root.path(),
        &[
            "new-window",
            "-d",
            "-t",
            "sidebar-split",
            "-n",
            "second",
            "sleep 1000",
        ],
    );

    let socket = stdout(root.path(), &["display-message", "-p", "#{socket_path}"]);
    let server_pid = stdout(root.path(), &["display-message", "-p", "#{pid}"]);
    let tmux_env = format!("{socket},{server_pid},0");
    let first_window = stdout(
        root.path(),
        &[
            "display-message",
            "-p",
            "-t",
            "sidebar-split:0",
            "#{window_id}",
        ],
    );
    let second_window = stdout(
        root.path(),
        &[
            "display-message",
            "-p",
            "-t",
            "sidebar-split:1",
            "#{window_id}",
        ],
    );
    let first_pane = stdout(
        root.path(),
        &["display-message", "-p", "-t", &first_window, "#{pane_id}"],
    );
    let second_pane = stdout(
        root.path(),
        &["display-message", "-p", "-t", &second_window, "#{pane_id}"],
    );
    let binary = env!("CARGO_BIN_EXE_tmux-agent-sidebar");
    let run_sidebar = |args: &[&str]| {
        let status = Command::new(binary)
            .env("TMUX", &tmux_env)
            .env("TMUX_TMPDIR", root.path())
            .args(args)
            .status()
            .expect("run sidebar lifecycle command");
        assert!(status.success(), "sidebar command failed: {args:?}");
    };

    run_sidebar(&["toggle", &first_window, "/tmp", &first_pane]);
    thread::sleep(Duration::from_millis(100));
    let sidebar = stdout(
        root.path(),
        &[
            "list-panes",
            "-a",
            "-f",
            "#{==:#{@pane_role},sidebar}",
            "-F",
            "#{pane_id}",
        ],
    );
    let lower_pane = stdout(
        root.path(),
        &[
            "split-window",
            "-d",
            "-v",
            "-t",
            &sidebar,
            "-P",
            "-F",
            "#{pane_id}",
            "sleep 1000",
        ],
    );
    let upper_pane = stdout(
        root.path(),
        &[
            "split-window",
            "-d",
            "-v",
            "-b",
            "-t",
            &sidebar,
            "-P",
            "-F",
            "#{pane_id}",
            "sleep 1000",
        ],
    );
    stdout(root.path(), &["resize-pane", "-t", &sidebar, "-x", "42"]);
    let geometry_format = "#{pane_left},#{pane_top},#{pane_width},#{pane_height}";
    let sidebar_geometry = stdout(
        root.path(),
        &["display-message", "-p", "-t", &sidebar, geometry_format],
    );
    let lower_geometry = stdout(
        root.path(),
        &["display-message", "-p", "-t", &lower_pane, geometry_format],
    );
    let upper_geometry = stdout(
        root.path(),
        &["display-message", "-p", "-t", &upper_pane, geometry_format],
    );

    run_sidebar(&["toggle", &second_window, "/tmp", &second_pane]);
    run_sidebar(&["toggle", &first_window, "/tmp", &first_pane]);

    assert_eq!(
        stdout(
            root.path(),
            &["display-message", "-p", "-t", &sidebar, geometry_format],
        ),
        sidebar_geometry
    );
    assert_eq!(
        stdout(
            root.path(),
            &["display-message", "-p", "-t", &lower_pane, geometry_format,],
        ),
        lower_geometry
    );
    assert_eq!(
        stdout(
            root.path(),
            &["display-message", "-p", "-t", &upper_pane, geometry_format],
        ),
        upper_geometry
    );
}
