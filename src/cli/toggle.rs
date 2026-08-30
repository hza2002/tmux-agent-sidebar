use std::collections::HashSet;

use crate::tmux;

trait TmuxClient {
    fn run(&self, args: &[&str]) -> Option<String>;

    fn display(&self, target: &str, format: &str) -> String {
        self.run(&["display-message", "-t", target, "-p", format])
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    }
}

struct LiveTmux;

impl TmuxClient for LiveTmux {
    fn run(&self, args: &[&str]) -> Option<String> {
        tmux::run_tmux(args)
    }
}

pub(crate) fn cmd_toggle(args: &[String]) -> i32 {
    let mut create_only = false;
    let mut positional = Vec::new();

    for arg in args {
        if arg == "--create-only" {
            create_only = true;
        } else {
            positional.push(arg.as_str());
        }
    }

    let window_id = match positional.first() {
        Some(id) => *id,
        None => return 0,
    };
    let pane_path = positional.get(1).copied().unwrap_or("~");
    let caller_pane = positional.get(2).copied().filter(|pane| !pane.is_empty());
    let client = LiveTmux;

    let existing_sidebar = match find_sidebar(&client, window_id) {
        Ok(sidebar) => sidebar,
        Err(_) => return 1,
    };
    if let Some(sidebar_pane) = existing_sidebar {
        if create_only {
            return 0;
        }
        if caller_pane.is_some_and(|pane| pane != sidebar_pane) {
            return focus_existing_sidebar(&client, &sidebar_pane, caller_pane).is_err() as i32;
        }
        return close_sidebar(&client, window_id, caller_pane).is_err() as i32;
    }

    let self_bin = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "tmux-agent-sidebar".to_string());
    let focus_sidebar = !create_only && caller_pane.is_some();

    open_sidebar(
        &client,
        OpenRequest {
            window_id,
            pane_path,
            caller_pane,
            focus_sidebar,
            self_bin: &self_bin,
        },
    )
    .is_err() as i32
}

pub(crate) fn cmd_close(args: &[String]) -> i32 {
    let Some(window_id) = args.first() else {
        return 0;
    };
    let caller_pane = args
        .get(1)
        .map(String::as_str)
        .filter(|pane| !pane.is_empty());
    close_sidebar(&LiveTmux, window_id, caller_pane).is_err() as i32
}

struct OpenRequest<'a> {
    window_id: &'a str,
    pane_path: &'a str,
    caller_pane: Option<&'a str>,
    focus_sidebar: bool,
    self_bin: &'a str,
}

fn open_sidebar(client: &impl TmuxClient, request: OpenRequest<'_>) -> Result<String, String> {
    let window_id = request.window_id;

    // Check sidebar width setting
    let sidebar_width_setting = {
        let s = client.display(window_id, &format!("#{{{}}}", tmux::SIDEBAR_WIDTH));
        if s.is_empty() { "35".to_string() } else { s }
    };

    let sidebar_width = if sidebar_width_setting.ends_with('%') {
        let window_width: u32 = client
            .display(window_id, "#{window_width}")
            .parse()
            .unwrap_or(0);
        let pct: u32 = sidebar_width_setting
            .trim_end_matches('%')
            .parse()
            .unwrap_or(15);
        if window_width > 0 && pct > 0 {
            let w = window_width * pct / 100;
            if w < 1 {
                "1".to_string()
            } else {
                w.to_string()
            }
        } else {
            sidebar_width_setting
        }
    } else {
        sidebar_width_setting
    };

    let sidebar_position = SidebarPosition::from_setting(
        &client.display(window_id, &format!("#{{{}}}", tmux::SIDEBAR_POSITION)),
    );

    let pane_geometry_output = client
        .run(&[
            "list-panes",
            "-t",
            window_id,
            "-F",
            "#{pane_left} #{pane_width} #{pane_id}",
        ])
        .unwrap_or_default();

    let target_pane = target_pane_for_position(&pane_geometry_output, sidebar_position)
        .unwrap_or_else(|| window_id.to_string());
    let split_flags = split_window_flags(sidebar_position);

    let active_pane = client.display(window_id, "#{pane_id}");
    let return_pane = request.caller_pane.unwrap_or(&active_pane);
    let saved_layout = client.display(window_id, "#{window_layout}");
    let saved_zoom = client.display(window_id, "#{window_zoomed_flag}");
    if saved_layout.is_empty()
        || !matches!(saved_zoom.as_str(), "0" | "1")
        || client
            .run(&[
                "set-option",
                "-w",
                "-t",
                window_id,
                tmux::SIDEBAR_SAVED_LAYOUT,
                &saved_layout,
            ])
            .is_none()
        || client
            .run(&[
                "set-option",
                "-w",
                "-t",
                window_id,
                tmux::SIDEBAR_SAVED_ZOOM,
                &saved_zoom,
            ])
            .is_none()
    {
        clear_saved_state(client, window_id);
        return Err("failed to save sidebar window state".into());
    }

    // Create sidebar pane
    let sidebar_pane = client
        .run(&[
            "split-window",
            split_flags,
            "-l",
            &sidebar_width,
            "-t",
            &target_pane,
            "-c",
            request.pane_path,
            "-P",
            "-F",
            "#{pane_id}",
            request.self_bin,
        ])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if sidebar_pane.is_empty() {
        clear_saved_state(client, window_id);
        return Err("failed to create sidebar pane".into());
    }
    if client
        .run(&[
            "set-option",
            "-p",
            "-t",
            &sidebar_pane,
            tmux::PANE_ROLE,
            "sidebar",
        ])
        .is_none()
    {
        let _ = client.run(&["kill-pane", "-t", &sidebar_pane]);
        clear_saved_state(client, window_id);
        return Err("failed to mark sidebar pane".into());
    }
    if !return_pane.is_empty() {
        let _ = client.run(&[
            "set-option",
            "-p",
            "-t",
            &sidebar_pane,
            tmux::SIDEBAR_RETURN_PANE,
            return_pane,
        ]);
    }
    let focus_target = if request.focus_sidebar || active_pane.is_empty() {
        sidebar_pane.as_str()
    } else {
        active_pane.as_str()
    };
    if client.run(&["select-pane", "-t", focus_target]).is_none() {
        let _ = close_sidebar(client, window_id, Some(return_pane));
        return Err("failed to select sidebar target".into());
    }
    Ok(sidebar_pane)
}

pub(crate) fn cmd_toggle_all(_args: &[String]) -> i32 {
    toggle_all(&LiveTmux).is_err() as i32
}

pub(crate) fn cmd_restart_sidebars(_args: &[String]) -> i32 {
    restart_sidebars(&LiveTmux).is_err() as i32
}

fn restart_sidebars(client: &impl TmuxClient) -> Result<(), String> {
    let format = format!("#{{pane_id}}|#{{{}}}", tmux::PANE_ROLE);
    let output = client
        .run(&["list-panes", "-a", "-F", &format])
        .ok_or_else(|| "failed to query existing sidebars".to_string())?;
    let specs = sidebar_restart_specs(&output);
    if specs.is_empty() {
        return Ok(());
    }

    client
        .run(&["set-option", "-g", tmux::SIDEBAR_FILTER, "all"])
        .ok_or_else(|| "failed to reset sidebar status filter".to_string())?;

    let self_bin =
        std::env::current_exe().map_err(|_| "failed to resolve current executable".to_string())?;
    let self_bin = self_bin
        .to_str()
        .ok_or_else(|| "current executable path is not valid UTF-8".to_string())?;
    let shell_command = crate::cli::setup::shell_quote(self_bin);
    let mut first_error = None;
    for sidebar_pane in specs {
        if client
            .run(&["respawn-pane", "-k", "-t", &sidebar_pane, &shell_command])
            .is_none()
            && first_error.is_none()
        {
            first_error = Some(format!("failed to restart sidebar pane {sidebar_pane}"));
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn sidebar_restart_specs(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let (pane_id, role) = line.split_once('|')?;
            (role == "sidebar").then(|| pane_id.to_string())
        })
        .collect()
}

fn toggle_all(client: &impl TmuxClient) -> Result<(), String> {
    let pane_id_role_format = pane_id_role_format();
    let has_sidebar = client
        .run(&["list-panes", "-a", "-F", &pane_id_role_format])
        .map(|output| any_sidebar_pane(&output))
        .ok_or_else(|| "failed to query sidebars".to_string())?;

    if has_sidebar {
        let format = format!("#{{window_id}}|{}", pane_id_role_format);
        let all_panes = client
            .run(&["list-panes", "-a", "-F", &format])
            .ok_or_else(|| "failed to query sidebar windows".to_string())?;
        for line in all_panes.lines() {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() == 3 && parts[2] == "sidebar" {
                close_sidebar(client, parts[0], None)?;
            }
        }
    } else {
        let all_windows = client
            .run(&[
                "list-panes",
                "-a",
                "-F",
                "#{window_id}|#{pane_current_path}",
            ])
            .ok_or_else(|| "failed to query windows".to_string())?;
        for (window_id, pane_path) in unique_window_paths(&all_windows) {
            let self_bin = std::env::current_exe()
                .ok()
                .and_then(|path| path.to_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| "tmux-agent-sidebar".to_string());
            open_sidebar(
                client,
                OpenRequest {
                    window_id: &window_id,
                    pane_path: &pane_path,
                    caller_pane: None,
                    focus_sidebar: false,
                    self_bin: &self_bin,
                },
            )?;
        }
    }

    Ok(())
}

fn any_sidebar_pane(output: &str) -> bool {
    output.lines().any(|line| {
        let parts: Vec<&str> = line.splitn(2, '|').collect();
        parts.len() >= 2 && parts[1] == "sidebar"
    })
}

fn find_sidebar(client: &impl TmuxClient, window_id: &str) -> Result<Option<String>, String> {
    let format = pane_id_role_format();
    let output = client
        .run(&["list-panes", "-t", window_id, "-F", &format])
        .ok_or_else(|| "failed to query sidebar panes".to_string())?;
    Ok(output.lines().find_map(|line| {
        let (pane_id, role) = line.split_once('|')?;
        (role == "sidebar").then(|| pane_id.to_string())
    }))
}

fn focus_existing_sidebar(
    client: &impl TmuxClient,
    sidebar_pane: &str,
    caller_pane: Option<&str>,
) -> Result<(), String> {
    if let Some(caller) = caller_pane {
        client
            .run(&[
                "set-option",
                "-p",
                "-t",
                sidebar_pane,
                tmux::SIDEBAR_RETURN_PANE,
                caller,
            ])
            .ok_or_else(|| "failed to update sidebar return pane".to_string())?;
    }
    client
        .run(&["select-pane", "-t", sidebar_pane])
        .ok_or_else(|| "failed to focus sidebar".to_string())?;
    Ok(())
}

fn close_sidebar(
    client: &impl TmuxClient,
    window_id: &str,
    caller_pane: Option<&str>,
) -> Result<(), String> {
    let Some(sidebar_pane) = find_sidebar(client, window_id)? else {
        clear_saved_state(client, window_id);
        return Ok(());
    };
    let saved_layout = show_window_option(client, window_id, tmux::SIDEBAR_SAVED_LAYOUT);
    let saved_zoom = show_window_option(client, window_id, tmux::SIDEBAR_SAVED_ZOOM);
    let return_pane = show_pane_option(client, &sidebar_pane, tmux::SIDEBAR_RETURN_PANE);
    let mut target_pane = caller_pane
        .filter(|pane| *pane != sidebar_pane)
        .unwrap_or(&return_pane)
        .to_string();

    let pane_count = client
        .display(window_id, "#{window_panes}")
        .parse::<usize>()
        .map_err(|_| "failed to query sidebar pane count".to_string())?;
    if pane_count == 1 {
        let cwd = client.display(&sidebar_pane, "#{pane_current_path}");
        target_pane = client
            .run(&[
                "split-window",
                "-d",
                "-t",
                &sidebar_pane,
                "-c",
                if cwd.is_empty() { "~" } else { &cwd },
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .map(|pane| pane.trim().to_string())
            .filter(|pane| !pane.is_empty())
            .ok_or_else(|| "failed to create replacement pane".to_string())?;
    }

    client
        .run(&["kill-pane", "-t", &sidebar_pane])
        .ok_or_else(|| "failed to close sidebar pane".to_string())?;
    if !saved_layout.is_empty() {
        let _ = client.run(&["select-layout", "-t", window_id, &saved_layout]);
    }
    clear_saved_state(client, window_id);

    if pane_exists(client, &target_pane) {
        let _ = client.run(&["select-pane", "-t", &target_pane]);
        if saved_zoom == "1" {
            let _ = client.run(&["resize-pane", "-Z", "-t", &target_pane]);
        }
    }
    Ok(())
}

fn clear_saved_state(client: &impl TmuxClient, window_id: &str) {
    let _ = client.run(&[
        "set-option",
        "-w",
        "-u",
        "-t",
        window_id,
        tmux::SIDEBAR_SAVED_LAYOUT,
    ]);
    let _ = client.run(&[
        "set-option",
        "-w",
        "-u",
        "-t",
        window_id,
        tmux::SIDEBAR_SAVED_ZOOM,
    ]);
}

fn show_window_option(client: &impl TmuxClient, target: &str, key: &str) -> String {
    client
        .run(&["show-option", "-wqv", "-t", target, key])
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn show_pane_option(client: &impl TmuxClient, target: &str, key: &str) -> String {
    client
        .run(&["show-option", "-pqv", "-t", target, key])
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn pane_exists(client: &impl TmuxClient, pane: &str) -> bool {
    !pane.is_empty()
        && client
            .run(&["display-message", "-p", "-t", pane, "#{pane_id}"])
            .is_some()
}

fn unique_window_paths(output: &str) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut windows = Vec::new();

    for line in output.lines() {
        let Some((window_id, pane_path)) = line.split_once('|') else {
            continue;
        };
        if seen.insert(window_id.to_string()) {
            windows.push((window_id.to_string(), pane_path.to_string()));
        }
    }

    windows
}

/// Which side of the window the sidebar pane is created on, driven by
/// the `@sidebar_position` tmux option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SidebarPosition {
    Left,
    Right,
}

impl SidebarPosition {
    /// Parse the raw `@sidebar_position` option value. Only an explicit
    /// (case-insensitive, whitespace-tolerant) `right` selects the right
    /// side; everything else — including unset, empty, or invalid values
    /// — falls back to the historical default of `left`, so a typo never
    /// moves the sidebar somewhere unexpected.
    fn from_setting(setting: &str) -> Self {
        if setting.trim().eq_ignore_ascii_case("right") {
            Self::Right
        } else {
            Self::Left
        }
    }
}

/// Horizontal placement of one pane, parsed from a
/// `#{pane_left} #{pane_width} #{pane_id}` formatted `list-panes` line.
#[derive(Debug, Eq, PartialEq)]
struct PaneGeometry {
    left: u32,
    width: u32,
    pane_id: String,
}

/// Parse a single `list-panes` output line into a [`PaneGeometry`].
/// Returns `None` for malformed lines so callers can simply skip them.
fn parse_pane_geometry(line: &str) -> Option<PaneGeometry> {
    let mut parts = line.split_whitespace();
    let left = parts.next()?.parse().ok()?;
    let width = parts.next()?.parse().ok()?;
    let pane_id = parts.next()?.to_string();
    Some(PaneGeometry {
        left,
        width,
        pane_id,
    })
}

/// Pick the pane the sidebar splits from: the leftmost pane for a left
/// sidebar, or the pane with the largest right edge (`left + width`) for
/// a right sidebar, so the new pane always lands at the window's outer
/// edge. Returns `None` when no line of `output` parses as geometry.
fn target_pane_for_position(output: &str, position: SidebarPosition) -> Option<String> {
    let panes = output.lines().filter_map(parse_pane_geometry);
    match position {
        SidebarPosition::Left => panes.min_by_key(|pane| pane.left),
        SidebarPosition::Right => panes.max_by_key(|pane| pane.left.saturating_add(pane.width)),
    }
    .map(|pane| pane.pane_id)
}

/// `split-window` flags for each placement: `-hfb` inserts the new pane
/// before the target (left of it), `-hf` after it (right of it). Both
/// `f` variants span the full window height.
fn split_window_flags(position: SidebarPosition) -> &'static str {
    match position {
        SidebarPosition::Left => "-hfb",
        SidebarPosition::Right => "-hf",
    }
}

/// Decide whether `cmd_auto_close` should kill the window, given the raw
/// outputs of the tmux queries it performs. Extracted as a pure function
/// so the guard logic is directly unit-testable without a running tmux
/// server.
///
/// - `list_panes_output`: `Some(stdout)` from `list-panes -F <pane role format>`,
///   or `None` if the tmux call failed.
/// - `session_windows`: parsed value of `#{session_windows}`, or `None`
///   if the tmux call failed or the value was unparseable.
/// - `session_attached`: parsed value of `#{session_attached}`, or `None`
///   if the tmux call failed or the value was unparseable.
fn should_kill_window(
    list_panes_output: Option<&str>,
    session_windows: Option<u32>,
    session_attached: Option<u32>,
) -> bool {
    // `list-panes` failed or returned nothing: the window is either gone
    // already or tmux is too busy to answer. Do NOT treat "no output"
    // as "no non-sidebar panes" — that would let us kill a live window
    // whose query happened to race with another tmux command.
    let Some(output) = list_panes_output else {
        return false;
    };
    if output.trim().is_empty() {
        return false;
    }

    let non_sidebar = output.lines().filter(|line| *line != "sidebar").count();
    if non_sidebar != 0 {
        return false;
    }

    let Some(windows) = session_windows else {
        return false;
    };

    // Last window in the session: killing it destroys the session and
    // drops every attached client. One attached client is fine — that
    // matches normal tmux `exit` behaviour on the last pane. Two or
    // more means a shared session (e.g. several terminal tabs attached
    // to `main`) where we cannot tell which clients are "wanted", so
    // preserve the sidebar instead. A missing `session_attached` errs
    // on the side of preservation.
    match windows {
        0 => false,
        1 => matches!(session_attached, Some(n) if n <= 1),
        _ => true,
    }
}

pub(crate) fn cmd_auto_close(args: &[String]) -> i32 {
    let window_id = match args.first() {
        Some(id) => id.as_str(),
        None => return 0,
    };

    let pane_role_format = format!("#{{{}}}", tmux::PANE_ROLE);
    let list_panes_output =
        tmux::run_tmux(&["list-panes", "-t", window_id, "-F", &pane_role_format]);

    let session_windows = tmux::run_tmux(&[
        "display-message",
        "-t",
        window_id,
        "-p",
        "#{session_windows}",
    ])
    .and_then(|s| s.trim().parse().ok());

    let session_attached = tmux::run_tmux(&[
        "display-message",
        "-t",
        window_id,
        "-p",
        "#{session_attached}",
    ])
    .and_then(|s| s.trim().parse().ok());

    if should_kill_window(
        list_panes_output.as_deref(),
        session_windows,
        session_attached,
    ) {
        let _ = tmux::run_tmux(&["kill-window", "-t", window_id]);
    }

    0
}

fn pane_id_role_format() -> String {
    format!("#{{pane_id}}|#{{{}}}", tmux::PANE_ROLE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque};

    #[derive(Default)]
    struct FixtureTmux {
        responses: RefCell<VecDeque<Option<String>>>,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl FixtureTmux {
        fn with_responses(responses: impl IntoIterator<Item = Option<&'static str>>) -> Self {
            Self {
                responses: RefCell::new(
                    responses
                        .into_iter()
                        .map(|response| response.map(str::to_string))
                        .collect(),
                ),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn called(&self, expected: &[&str]) -> bool {
            self.calls
                .borrow()
                .iter()
                .any(|call| call.iter().map(String::as_str).eq(expected.iter().copied()))
        }
    }

    impl TmuxClient for FixtureTmux {
        fn run(&self, args: &[&str]) -> Option<String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|arg| arg.to_string()).collect());
            self.responses.borrow_mut().pop_front().flatten()
        }
    }

    #[test]
    fn open_saves_state_marks_return_pane_and_focuses_sidebar() {
        let tmux = FixtureTmux::with_responses([
            Some(""),
            Some("left"),
            Some("0 120 %1"),
            Some("%1"),
            Some("layout-before"),
            Some("1"),
            Some(""),
            Some(""),
            Some("%9"),
            Some(""),
            Some(""),
            Some(""),
        ]);

        let pane = open_sidebar(
            &tmux,
            OpenRequest {
                window_id: "@1",
                pane_path: "/repo",
                caller_pane: Some("%1"),
                focus_sidebar: true,
                self_bin: "/bin/sidebar",
            },
        )
        .unwrap();

        assert_eq!(pane, "%9");
        assert!(tmux.called(&[
            "split-window",
            "-hfb",
            "-l",
            "35",
            "-t",
            "%1",
            "-c",
            "/repo",
            "-P",
            "-F",
            "#{pane_id}",
            "/bin/sidebar",
        ]));
        assert!(tmux.called(&[
            "set-option",
            "-w",
            "-t",
            "@1",
            tmux::SIDEBAR_SAVED_LAYOUT,
            "layout-before"
        ]));
        assert!(tmux.called(&[
            "set-option",
            "-p",
            "-t",
            "%9",
            tmux::SIDEBAR_RETURN_PANE,
            "%1"
        ]));
        assert!(tmux.called(&["select-pane", "-t", "%9"]));
    }

    #[test]
    fn create_only_restores_original_focus() {
        let tmux = FixtureTmux::with_responses([
            Some(""),
            Some("left"),
            Some("0 120 %1"),
            Some("%1"),
            Some("layout-before"),
            Some("0"),
            Some(""),
            Some(""),
            Some("%9"),
            Some(""),
            Some(""),
            Some(""),
        ]);
        open_sidebar(
            &tmux,
            OpenRequest {
                window_id: "@1",
                pane_path: "/repo",
                caller_pane: Some("%1"),
                focus_sidebar: false,
                self_bin: "/bin/sidebar",
            },
        )
        .unwrap();
        assert!(tmux.called(&["select-pane", "-t", "%1"]));
    }

    #[test]
    fn focus_existing_updates_return_pane_without_closing() {
        let tmux = FixtureTmux::with_responses([Some(""), Some("")]);
        focus_existing_sidebar(&tmux, "%9", Some("%2")).unwrap();
        assert!(tmux.called(&[
            "set-option",
            "-p",
            "-t",
            "%9",
            tmux::SIDEBAR_RETURN_PANE,
            "%2"
        ]));
        assert!(tmux.called(&["select-pane", "-t", "%9"]));
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "kill-pane")
        );
    }

    fn close_fixture(target_exists: bool) -> FixtureTmux {
        FixtureTmux::with_responses([
            Some("%9|sidebar"),
            Some("layout-before"),
            Some("1"),
            Some("%1"),
            Some("2"),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            target_exists.then_some("%1"),
            target_exists.then_some(""),
            target_exists.then_some(""),
        ])
    }

    #[test]
    fn close_restores_layout_return_pane_and_zoom() {
        let tmux = close_fixture(true);
        close_sidebar(&tmux, "@1", Some("%9")).unwrap();
        assert!(tmux.called(&["select-layout", "-t", "@1", "layout-before"]));
        assert!(tmux.called(&["select-pane", "-t", "%1"]));
        assert!(tmux.called(&["resize-pane", "-Z", "-t", "%1"]));
        assert!(tmux.called(&[
            "set-option",
            "-w",
            "-u",
            "-t",
            "@1",
            tmux::SIDEBAR_SAVED_LAYOUT
        ]));
    }

    #[test]
    fn close_ignores_stale_return_pane() {
        let tmux = close_fixture(false);
        close_sidebar(&tmux, "@1", None).unwrap();
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "select-pane")
        );
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "resize-pane")
        );
    }

    #[test]
    fn close_query_failure_preserves_saved_state() {
        let tmux = FixtureTmux::with_responses([None]);
        assert!(close_sidebar(&tmux, "@1", None).is_err());
        assert_eq!(tmux.calls.borrow().len(), 1);
    }

    #[test]
    fn open_split_failure_clears_saved_state() {
        let tmux = FixtureTmux::with_responses([
            Some(""),
            Some("left"),
            Some("0 120 %1"),
            Some("%1"),
            Some("layout-before"),
            Some("0"),
            Some(""),
            Some(""),
            None,
            Some(""),
            Some(""),
        ]);
        assert!(
            open_sidebar(
                &tmux,
                OpenRequest {
                    window_id: "@1",
                    pane_path: "/repo",
                    caller_pane: Some("%1"),
                    focus_sidebar: true,
                    self_bin: "/bin/sidebar",
                },
            )
            .is_err()
        );
        assert!(tmux.called(&[
            "set-option",
            "-w",
            "-u",
            "-t",
            "@1",
            tmux::SIDEBAR_SAVED_LAYOUT
        ]));
    }

    #[test]
    fn open_focus_failure_closes_sidebar_and_clears_saved_state() {
        let tmux = FixtureTmux::with_responses([
            Some(""),
            Some("left"),
            Some("0 120 %1"),
            Some("%1"),
            Some("layout-before"),
            Some("0"),
            Some(""),
            Some(""),
            Some("%9"),
            Some(""),
            Some(""),
            None,
            Some("%9|sidebar"),
            Some("layout-before"),
            Some("0"),
            Some("%1"),
            Some("2"),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
        ]);

        assert!(
            open_sidebar(
                &tmux,
                OpenRequest {
                    window_id: "@1",
                    pane_path: "/repo",
                    caller_pane: Some("%1"),
                    focus_sidebar: true,
                    self_bin: "/bin/sidebar",
                },
            )
            .is_err()
        );
        assert!(tmux.called(&["kill-pane", "-t", "%9"]));
        assert!(tmux.called(&[
            "set-option",
            "-w",
            "-u",
            "-t",
            "@1",
            tmux::SIDEBAR_SAVED_LAYOUT
        ]));
    }

    #[test]
    fn close_last_sidebar_creates_shell_replacement_first() {
        let tmux = FixtureTmux::with_responses([
            Some("%9|sidebar"),
            Some("layout-before"),
            Some("0"),
            Some(""),
            Some("1"),
            Some("/repo"),
            Some("%10"),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some("%10"),
            Some(""),
        ]);
        close_sidebar(&tmux, "@1", Some("%9")).unwrap();
        let calls = tmux.calls.borrow();
        let split = calls
            .iter()
            .position(|call| call[0] == "split-window")
            .unwrap();
        let kill = calls
            .iter()
            .position(|call| call[0] == "kill-pane")
            .unwrap();
        assert!(split < kill);
        assert!(tmux.called(&["select-pane", "-t", "%10"]));
    }

    #[test]
    fn toggle_all_close_path_uses_layout_restoring_helper() {
        let tmux = FixtureTmux::with_responses([
            Some("%9|sidebar"),
            Some("@1|%9|sidebar"),
            Some("%9|sidebar"),
            Some("layout-before"),
            Some("0"),
            Some("%1"),
            Some("2"),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
        ]);
        toggle_all(&tmux).unwrap();
        assert!(tmux.called(&["select-layout", "-t", "@1", "layout-before"]));
    }

    #[test]
    fn any_sidebar_pane_detects_sidebar_anywhere() {
        let output = "%1|pane\n%2|sidebar\n%3|pane";
        assert!(any_sidebar_pane(output));
    }

    #[test]
    fn any_sidebar_pane_returns_false_without_sidebar() {
        let output = "%1|pane\n%2|main";
        assert!(!any_sidebar_pane(output));
    }

    #[test]
    fn unique_window_paths_deduplicates_windows_and_keeps_spaces() {
        let output = "%1|/Users/me/My Project\n%1|/Users/me/My Project\n%2|/tmp/another project";
        assert_eq!(
            unique_window_paths(output),
            vec![
                ("%1".to_string(), "/Users/me/My Project".to_string()),
                ("%2".to_string(), "/tmp/another project".to_string()),
            ]
        );
    }

    #[test]
    fn unique_window_paths_skips_malformed_lines() {
        let output = "bad-line\n%1|/tmp";
        assert_eq!(
            unique_window_paths(output),
            vec![("%1".to_string(), "/tmp".to_string())]
        );
    }

    // ─── sidebar placement ───────────────────────────────────────────

    #[test]
    fn sidebar_position_parses_right_only() {
        assert_eq!(
            SidebarPosition::from_setting("right"),
            SidebarPosition::Right
        );
        assert_eq!(
            SidebarPosition::from_setting(" RIGHT "),
            SidebarPosition::Right
        );
        assert_eq!(SidebarPosition::from_setting("left"), SidebarPosition::Left);
        assert_eq!(SidebarPosition::from_setting(""), SidebarPosition::Left);
        assert_eq!(
            SidebarPosition::from_setting("invalid"),
            SidebarPosition::Left
        );
    }

    #[test]
    fn target_pane_for_left_position_uses_leftmost_pane() {
        let output = "40 80 %3\n0 20 %1\n20 20 %2";

        assert_eq!(
            target_pane_for_position(output, SidebarPosition::Left),
            Some("%1".to_string())
        );
    }

    #[test]
    fn target_pane_for_right_position_uses_largest_right_edge() {
        let output = "0 20 %1\n20 20 %2\n40 80 %3";

        assert_eq!(
            target_pane_for_position(output, SidebarPosition::Right),
            Some("%3".to_string())
        );
    }

    #[test]
    fn target_pane_for_position_skips_malformed_lines() {
        let output = "bad-line\n0 nope %1\n12 30 %2";

        assert_eq!(
            target_pane_for_position(output, SidebarPosition::Left),
            Some("%2".to_string())
        );
        assert_eq!(target_pane_for_position("", SidebarPosition::Right), None);
    }

    #[test]
    fn split_window_flags_match_tmux_side_semantics() {
        assert_eq!(split_window_flags(SidebarPosition::Left), "-hfb");
        assert_eq!(split_window_flags(SidebarPosition::Right), "-hf");
    }

    // ─── should_kill_window ───────────────────────────────────────────

    #[test]
    fn should_kill_window_kills_when_only_sidebar_and_other_windows_exist() {
        // Classic intended path: sidebar alone in a window, session has
        // other windows to fall back on. Attached-client count is
        // irrelevant because killing this window does not end the
        // session.
        assert!(should_kill_window(Some("sidebar"), Some(2), None));
        assert!(should_kill_window(Some("sidebar"), Some(2), Some(0)));
        assert!(should_kill_window(Some("sidebar"), Some(2), Some(5)));
    }

    #[test]
    fn should_kill_window_skips_when_non_sidebar_pane_remains() {
        // Another pane with `@pane_role` explicitly set to something
        // non-sidebar (e.g. a spawn-marked pane) keeps the window alive.
        assert!(!should_kill_window(Some("sidebar\npane"), Some(5), Some(1)));
        // `@pane_role` unset renders as an empty line — that pane is
        // a regular user pane, not a sidebar, so the window must stay.
        // The real tmux output for [sidebar pane, regular pane] is
        // "sidebar\n\n" (sidebar's role, then the regular pane's empty
        // role followed by the final record separator).
        assert!(!should_kill_window(Some("sidebar\n\n"), Some(5), Some(1)));
        assert!(!should_kill_window(Some("\nsidebar\n"), Some(5), Some(1)));
    }

    #[test]
    fn should_kill_window_skips_when_list_panes_failed() {
        // `list-panes` failure must never be treated as "window is empty" —
        // that used to let a busy-tmux race kill a live window.
        assert!(!should_kill_window(None, Some(5), Some(1)));
    }

    #[test]
    fn should_kill_window_skips_when_list_panes_empty() {
        // Whitespace-only output (e.g. window already gone) must not
        // trigger a kill either.
        assert!(!should_kill_window(Some(""), Some(5), Some(1)));
        assert!(!should_kill_window(Some("   \n"), Some(5), Some(1)));
    }

    #[test]
    fn should_kill_window_kills_last_window_when_single_client_attached() {
        // One client attached to a single-window session: destroying
        // the session only detaches the same client that just kept the
        // session alive, which matches tmux's standard `exit` behaviour
        // on the last pane — the user expects the sidebar to go with it.
        assert!(should_kill_window(Some("sidebar"), Some(1), Some(1)));
    }

    #[test]
    fn should_kill_window_kills_last_window_when_detached() {
        // No clients attached: killing the session harms no one, and
        // a stranded sidebar in a detached session is pointless anyway.
        assert!(should_kill_window(Some("sidebar"), Some(1), Some(0)));
    }

    #[test]
    fn should_kill_window_preserves_last_window_when_multiple_clients_attached() {
        // Core regression guard (0dc6e99): killing the last window of
        // a session drops every attached client. With multiple terminal
        // tabs sharing a single `main` session, that manifested as every
        // tab dying at once. Keep the sidebar stranded rather than nuke
        // the session.
        assert!(!should_kill_window(Some("sidebar"), Some(1), Some(2)));
        assert!(!should_kill_window(Some("sidebar"), Some(1), Some(7)));
    }

    #[test]
    fn should_kill_window_preserves_last_window_when_attached_query_failed() {
        // Without knowing how many clients are attached we cannot prove
        // the kill is safe. Better a lingering sidebar pane than a
        // mass-disconnect.
        assert!(!should_kill_window(Some("sidebar"), Some(1), None));
    }

    #[test]
    fn should_kill_window_skips_when_session_windows_query_failed() {
        // If we cannot prove the session has other windows, err on the
        // side of preservation. Better to leave a lingering sidebar
        // pane than to destroy a live workspace.
        assert!(!should_kill_window(Some("sidebar"), None, Some(1)));
        assert!(!should_kill_window(Some("sidebar"), Some(0), Some(1)));
    }

    #[test]
    fn restart_specs_include_only_existing_sidebar_panes() {
        let output = "%1|\n%9|sidebar\n%2|\n%8|sidebar";
        assert_eq!(
            sidebar_restart_specs(output),
            vec!["%9".to_string(), "%8".to_string()]
        );
    }

    #[test]
    fn restart_respawns_in_place_and_resets_status_filter() {
        let tmux = FixtureTmux::with_responses([Some("%1|\n%9|sidebar"), Some(""), Some("")]);

        restart_sidebars(&tmux).unwrap();

        assert!(tmux.called(&["set-option", "-g", tmux::SIDEBAR_FILTER, "all",]));
        assert!(tmux.calls.borrow().iter().any(|call| {
            call.first().map(String::as_str) == Some("respawn-pane")
                && call.get(3).map(String::as_str) == Some("%9")
        }));
        assert!(tmux.calls.borrow().iter().all(|call| {
            !matches!(
                call.first().map(String::as_str),
                Some("select-pane" | "select-window" | "switch-client")
            )
        }));
    }

    #[test]
    fn restart_continues_after_one_sidebar_respawn_fails() {
        let tmux = FixtureTmux::with_responses([
            Some("%9|sidebar\n%10|sidebar"),
            Some(""),
            None,
            Some(""),
        ]);

        assert!(restart_sidebars(&tmux).is_err());
        assert!(tmux.calls.borrow().iter().any(|call| {
            call.first().map(String::as_str) == Some("respawn-pane")
                && call.get(3).map(String::as_str) == Some("%10")
        }));
    }
}
