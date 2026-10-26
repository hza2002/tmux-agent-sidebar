use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

/// The tmux socket lives at `<root>/tmux.sock`, and Unix socket paths are
/// capped at ~104 bytes — macOS `$TMPDIR` alone is already ~50 bytes. Root the
/// scratch dir in `/tmp` so the socket path stays short regardless of the
/// caller's `TMPDIR`.
fn short_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("tmux-agent-sidebar-test.")
        .tempdir_in("/tmp")
        .unwrap()
}

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
    let root = short_tempdir();
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
    // Newer tmux prints an empty pane_pid for command-less slot panes; older
    // versions print 0. Accept both and pin only what the layout relies on.
    let slot_lines: Vec<&str> = after_first_move
        .lines()
        .filter(|line| line.contains("|sidebar-slot|"))
        .collect();
    assert_eq!(slot_lines.len(), 1, "expected one slot: {after_first_move}");
    let slot_fields: Vec<&str> = slot_lines[0].split('|').collect();
    assert!(
        matches!(slot_fields[3], "" | "0"),
        "slot pane_pid should be empty or 0: {}",
        slot_lines[0]
    );
    assert_eq!(slot_fields[4], "35");
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
    let root = short_tempdir();
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

/// A real terminal client is needed to exercise client-driven window resizing.
/// Keep it on the test's private socket and drain its PTY so redraws cannot
/// block the server when the test is not reading terminal output.
struct WidthTestClient {
    child: std::process::Child,
    master: std::fs::File,
}

impl WidthTestClient {
    fn attach(root: &std::path::Path, session: &str, width: u16) -> Self {
        use std::os::fd::FromRawFd;
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 50,
            ws_col: width,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: openpty initializes both descriptors and only reads size.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut size,
                )
            },
            0,
            "open test PTY: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: both descriptors were freshly allocated by openpty above.
        let master = unsafe { std::fs::File::from_raw_fd(master) };
        let slave = unsafe { std::fs::File::from_raw_fd(slave) };
        let mut command = Command::new("tmux");
        command
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("TMUX_TMPDIR", root)
            .env("TERM", "xterm-256color")
            .arg("-S")
            .arg(root.join("tmux.sock"))
            .args(["attach-session", "-t", session])
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        // SAFETY: the child-side closure uses only async-signal-safe libc calls.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1
                    || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as _, 0) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("attach isolated terminal client");
        let mut reader = master.try_clone().unwrap();
        thread::spawn(move || {
            use std::io::Read;
            let mut buffer = [0; 8192];
            while matches!(reader.read(&mut buffer), Ok(count) if count > 0) {}
        });
        Self { child, master }
    }

    fn resize(&self, width: u16) {
        use std::os::fd::AsRawFd;
        let size = libc::winsize {
            ws_row: 50,
            ws_col: width,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: master owns a live PTY and size points to a valid winsize.
        assert_eq!(
            unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) },
            0
        );
        // SAFETY: child.id() is the tmux client process started by this helper.
        assert_eq!(
            unsafe { libc::kill(self.child.id() as i32, libc::SIGWINCH) },
            0
        );
    }
}

impl Drop for WidthTestClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_width_condition(root: &std::path::Path, description: &str, condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "{description}: {}",
            stdout(
                root,
                &[
                    "list-panes",
                    "-a",
                    "-F",
                    "#{window_id}|#{pane_id}|#{window_width}|#{pane_width}|#{window_zoomed_flag}"
                ]
            )
        );
        thread::sleep(Duration::from_millis(20));
    }
}

struct SidebarWidthFixture {
    root: tempfile::TempDir,
    window: String,
    main: String,
    sidebar: String,
    tmux_env: String,
}

impl SidebarWidthFixture {
    fn new() -> Self {
        let root = short_tempdir();
        let main = stdout(
            root.path(),
            &[
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                "sidebar-width",
                "-x",
                "200",
                "-y",
                "50",
                "-P",
                "-F",
                "#{pane_id}",
                "sleep 1000",
            ],
        );
        let window = stdout(
            root.path(),
            &["display-message", "-p", "-t", &main, "#{window_id}"],
        );
        let sidebar = stdout(
            root.path(),
            &[
                "split-window",
                "-h",
                "-d",
                "-l",
                "35",
                "-t",
                &main,
                "-P",
                "-F",
                "#{pane_id}",
                "",
            ],
        );
        stdout(
            root.path(),
            &[
                "set-option",
                "-p",
                "-t",
                &sidebar,
                "@pane_role",
                "sidebar-slot",
            ],
        );
        stdout(
            root.path(),
            &["set-option", "-g", "@agent_sidebar_enabled", "on"],
        );
        let socket = stdout(root.path(), &["display-message", "-p", "#{socket_path}"]);
        let pid = stdout(root.path(), &["display-message", "-p", "#{pid}"]);
        let fixture = Self {
            root,
            window,
            main,
            sidebar,
            tmux_env: format!("{socket},{pid},0"),
        };
        fixture.run(&["fit-width", &fixture.window]);
        fixture.install_hook(false);
        fixture
    }

    fn run(&self, args: &[&str]) {
        let output = Command::new(env!("CARGO_BIN_EXE_tmux-agent-sidebar"))
            .env_remove("TMUX_PANE")
            .env("TMUX", &self.tmux_env)
            .env("TMUX_TMPDIR", self.root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "sidebar {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn install_hook(&self, delayed: bool) {
        let binary = env!("CARGO_BIN_EXE_tmux-agent-sidebar").replace('\'', "'\\''");
        let delay = if delayed { "sleep 0.6; " } else { "" };
        let hook = format!(
            "run-shell -b \"{delay}TMUX='{}' '{binary}' fit-width '#{{hook_window}}' '#{{window_width}}' '#{{window_height}}' '#{{@agent_sidebar_width_state}}'\"",
            self.tmux_env
        );
        stdout(
            self.root.path(),
            &["set-hook", "-g", "window-layout-changed", &hook],
        );
    }

    fn display(&self, target: &str, format: &str) -> String {
        stdout(
            self.root.path(),
            &["display-message", "-p", "-t", target, format],
        )
    }

    fn wait(&self, description: &str, condition: impl Fn() -> bool) {
        wait_width_condition(self.root.path(), description, condition);
    }

    fn wait_width(&self, width: &str) {
        self.wait(&format!("sidebar reached target width {width}"), || {
            let current = self.display(&self.window, "#{window_width},#{window_height},");
            self.display(&self.sidebar, "#{pane_width}") == width
                && self
                    .display(&self.window, "#{@agent_sidebar_width_state}")
                    .starts_with(&current)
        });
        // Reaching the target can overlap the current observer's remaining
        // command queue. Take its lifecycle lock once before the next user
        // action; older queued captured events must still be rejected by token.
        self.run(&["fit-width", &self.window]);
        assert_eq!(self.display(&self.sidebar, "#{pane_width}"), width);
    }

    fn attach(&self) -> WidthTestClient {
        let client = WidthTestClient::attach(self.root.path(), "sidebar-width", 200);
        self.wait("terminal client attached", || {
            stdout(self.root.path(), &["list-clients", "-F", "#{client_name}"])
                .lines()
                .count()
                == 1
        });
        self.wait("initial terminal size observed", || {
            self.display(&self.window, "#{@agent_sidebar_width_state}")
                .starts_with("200,49,")
        });
        client
    }
}

impl Drop for SidebarWidthFixture {
    fn drop(&mut self) {
        let _ = tmux(self.root.path(), &["kill-session", "-t", "sidebar-width"]);
        let _ = tmux(self.root.path(), &["kill-session", "-t", "linked-width"]);
    }
}

#[test]
#[ignore = "requires local tmux and a real PTY client"]
fn sidebar_width_tracks_terminal_resize_and_defers_zoom_repair() {
    let fixture = SidebarWidthFixture::new();
    let client = fixture.attach();
    client.resize(150);
    fixture.wait("window resized to150", || {
        fixture.display(&fixture.window, "#{window_width}") == "150"
    });
    fixture.wait_width("35");
    client.resize(240);
    fixture.wait("window resized to240", || {
        fixture.display(&fixture.window, "#{window_width}") == "240"
    });
    fixture.wait_width("35");

    stdout(
        fixture.root.path(),
        &["resize-pane", "-t", &fixture.sidebar, "-x", "44"],
    );
    thread::sleep(Duration::from_millis(150));
    fixture.wait_width("44");
    stdout(
        fixture.root.path(),
        &["resize-pane", "-Z", "-t", &fixture.main],
    );
    assert_eq!(
        fixture.display(&fixture.window, "#{window_zoomed_flag}"),
        "1"
    );
    stdout(
        fixture.root.path(),
        &["resize-pane", "-Z", "-t", &fixture.main],
    );
    thread::sleep(Duration::from_millis(150));
    fixture.wait_width("44");

    stdout(
        fixture.root.path(),
        &["resize-pane", "-Z", "-t", &fixture.main],
    );
    client.resize(180);
    fixture.wait("zoomed resize marked pending", || {
        fixture
            .display(&fixture.window, "#{@agent_sidebar_width_state}")
            .starts_with("180,49,1,")
    });
    assert_eq!(fixture.display(&fixture.main, "#{pane_width}"), "180");
    assert_eq!(
        fixture.display(&fixture.window, "#{window_zoomed_flag}"),
        "1"
    );
    stdout(
        fixture.root.path(),
        &["resize-pane", "-Z", "-t", &fixture.main],
    );
    fixture.wait_width("35");
    assert_eq!(
        fixture.display(&fixture.window, "#{window_zoomed_flag}"),
        "0"
    );

    // Percentage targets use the final window width, not the previous size.
    stdout(
        fixture.root.path(),
        &["set-option", "-g", "@sidebar_width", "20%"],
    );
    client.resize(250);
    fixture.wait("window resized to250", || {
        fixture.display(&fixture.window, "#{window_width}") == "250"
    });
    fixture.wait_width("50");
    stdout(
        fixture.root.path(),
        &["set-option", "-g", "@sidebar_width", "35"],
    );
    client.resize(30);
    fixture.wait("narrow window reached30", || {
        fixture.display(&fixture.window, "#{window_width}") == "30"
    });
    fixture.wait_width("28");
    client.resize(200);
    fixture.wait("window grew back200", || {
        fixture.display(&fixture.window, "#{window_width}") == "200"
    });
    fixture.wait_width("35");

    // Multiple callbacks may wait behind lifecycle work until the window has
    // returned to its original size. Captured event dimensions retain the
    // intermediate shrink, whose clamp otherwise leaves the sidebar at41.
    fixture.install_hook(true);
    client.resize(120);
    fixture.wait("burst intermediate window size120", || {
        fixture.display(&fixture.window, "#{window_width}") == "120"
    });
    client.resize(200);
    fixture.wait("burst final window size200", || {
        fixture.display(&fixture.window, "#{window_width}") == "200"
    });
    fixture.wait_width("35");
    stdout(
        fixture.root.path(),
        &["resize-pane", "-t", &fixture.sidebar, "-x", "44"],
    );
    // Let the deliberately delayed duplicate callbacks complete: consuming a
    // resize once must not let an old net-zero event overwrite a later edit.
    thread::sleep(Duration::from_millis(750));
    fixture.wait_width("44");
}

#[test]
#[ignore = "requires local tmux and real PTY clients"]
fn sidebar_width_is_window_local_and_skips_multiple_clients() {
    let fixture = SidebarWidthFixture::new();
    let client = fixture.attach();
    let inactive = stdout(
        fixture.root.path(),
        &[
            "new-window",
            "-d",
            "-t",
            "sidebar-width:",
            "-n",
            "inactive",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep 1000",
        ],
    );
    let inactive_window = fixture.display(&inactive, "#{window_id}");
    let inactive_slot = stdout(
        fixture.root.path(),
        &[
            "split-window",
            "-h",
            "-d",
            "-l",
            "35",
            "-t",
            &inactive,
            "-P",
            "-F",
            "#{pane_id}",
            "",
        ],
    );
    stdout(
        fixture.root.path(),
        &[
            "set-option",
            "-p",
            "-t",
            &inactive_slot,
            "@pane_role",
            "sidebar-slot",
        ],
    );
    fixture.run(&["fit-width", &inactive_window]);
    // A linked window has multiple session memberships but one physical layout.
    stdout(
        fixture.root.path(),
        &["new-session", "-d", "-s", "linked-width", "sleep 1000"],
    );
    stdout(
        fixture.root.path(),
        &["link-window", "-s", &fixture.window, "-t", "linked-width:1"],
    );
    stdout(
        fixture.root.path(),
        &["resize-pane", "-t", &inactive_slot, "-x", "41"],
    );
    thread::sleep(Duration::from_millis(150));
    let inactive_geometry = fixture.display(&inactive_window, "#{window_width}");
    client.resize(150);
    fixture.wait("linked active window resized150", || {
        fixture.display(&fixture.window, "#{window_width}") == "150"
    });
    fixture.wait_width("35");
    // New tmux retains an inactive window's size, while older versions resize
    // it too. Only the window that actually changed should be repaired.
    let inactive_final = fixture.display(&inactive_window, "#{window_width}");
    let expected = if inactive_final == inactive_geometry {
        "41"
    } else {
        "35"
    };
    fixture.wait("inactive window obeyed its own size event", || {
        fixture.display(&inactive_slot, "#{pane_width}") == expected
    });

    let second_client = WidthTestClient::attach(fixture.root.path(), "sidebar-width", 150);
    fixture.wait("two clients attached", || {
        stdout(
            fixture.root.path(),
            &["list-clients", "-F", "#{client_name}"],
        )
        .lines()
        .count()
            == 2
    });
    // Observe and remember a real window resize, but leave tmux's resulting
    // pane geometry untouched while the layout is shared by multiple clients.
    stdout(
        fixture.root.path(),
        &["resize-window", "-t", &fixture.window, "-x", "190"],
    );
    let width_after_tmux = fixture.display(&fixture.sidebar, "#{pane_width}");
    fixture.wait("multi-client resize observed", || {
        fixture
            .display(&fixture.window, "#{@agent_sidebar_width_state}")
            .starts_with("190,")
    });
    thread::sleep(Duration::from_millis(150));
    assert_eq!(
        fixture.display(&fixture.sidebar, "#{pane_width}"),
        width_after_tmux
    );
    assert_ne!(
        width_after_tmux, "35",
        "test must exercise a width that needs repair"
    );
    drop(second_client);
}

#[test]
#[ignore = "requires local tmux and a real PTY client"]
fn sidebar_width_config_reload_installs_one_working_observer() {
    let fixture = SidebarWidthFixture::new();
    let root = fixture.root.path();
    // Replace the test-only observer with the shipped declaration, and load it
    // twice to exercise parser quoting plus removal of older registrations.
    stdout(root, &["set-hook", "-gu", "window-layout-changed"]);
    stdout(
        root,
        &[
            "set-option",
            "-g",
            "@agent_sidebar_bin",
            env!("CARGO_BIN_EXE_tmux-agent-sidebar"),
        ],
    );
    for option in [
        "@sidebar_auto_create",
        "@sidebar_key",
        "@sidebar_key_all",
        "@sidebar_close_key",
        "@sidebar_quota",
    ] {
        stdout(root, &["set-option", "-g", option, "off"]);
    }
    stdout(root, &["resize-pane", "-t", &fixture.sidebar, "-x", "44"]);
    let config = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("agent-sidebar.conf");
    for _ in 0..2 {
        stdout(root, &["source-file", config.to_str().unwrap()]);
    }
    assert_eq!(
        fixture.display(&fixture.sidebar, "#{pane_width}"),
        "44",
        "reload must not resize existing geometry"
    );
    let hooks = stdout(root, &["show-hooks", "-g", "window-layout-changed"]);
    assert_eq!(
        hooks.matches("fit-width").count(),
        1,
        "observer duplicated on reload: {hooks}"
    );
    assert!(hooks.contains("hook_window"));
    assert!(hooks.contains("window_width") && hooks.contains("window_height"));
    assert!(hooks.contains("@agent_sidebar_width_state"));
    // This fixture has a processless slot instead of a live sidebar. Suppress
    // automatic following so attach tests the real width declaration without
    // launching the TUI or its unrelated background workers.
    for hook in [
        "after-select-window",
        "session-window-changed",
        "client-session-changed",
        "client-attached",
        "client-detached",
        "client-resized",
    ] {
        stdout(root, &["set-hook", "-gu", hook]);
    }
    let client = fixture.attach();
    fixture.wait_width("35");
    client.resize(150);
    fixture.wait("shipped hook window reached150", || {
        fixture.display(&fixture.window, "#{window_width}") == "150"
    });
    fixture.wait_width("35");
}
