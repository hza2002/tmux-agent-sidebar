use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::tmux;

mod topology;

const DEFAULT_SIDEBAR_WIDTH_COLUMNS: u32 = 35;
const LIFECYCLE_LOCK_RETRIES: usize = 100;
const LIFECYCLE_LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);

pub(super) trait TmuxClient {
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

struct LifecycleLock {
    _file: File,
}

impl LifecycleLock {
    fn acquire() -> Result<Self, String> {
        let socket_path = std::env::var("TMUX")
            .ok()
            .and_then(|value| value.split(',').next().map(str::to_string))
            .filter(|value| !value.is_empty())
            .or_else(|| tmux::run_tmux(&["display-message", "-p", "#{socket_path}"]))
            .ok_or_else(|| "failed to resolve tmux socket for sidebar lock".to_string())?;
        let socket_path = Path::new(socket_path.trim());
        let parent = socket_path
            .parent()
            .ok_or_else(|| "tmux socket has no parent directory".to_string())?;
        let socket_name = socket_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "tmux socket name is not valid UTF-8".to_string())?;
        let lock_path = parent.join(format!(".{socket_name}.tmux-agent-sidebar.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|error| format!("failed to open sidebar lock: {error}"))?;

        for _ in 0..LIFECYCLE_LOCK_RETRIES {
            let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if result == 0 {
                return Ok(Self { _file: file });
            }
            let error = std::io::Error::last_os_error();
            if !matches!(
                error.raw_os_error(),
                Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
            ) {
                return Err(format!("failed to acquire sidebar lock: {error}"));
            }
            thread::sleep(LIFECYCLE_LOCK_RETRY_DELAY);
        }
        Err("timed out acquiring sidebar lifecycle lock".into())
    }
}

fn acquire_lifecycle_lock() -> Result<LifecycleLock, i32> {
    LifecycleLock::acquire().map_err(|_| 1)
}

pub(crate) fn cmd_toggle(args: &[String]) -> i32 {
    let Ok(_lock) = acquire_lifecycle_lock() else {
        return 1;
    };
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

    let sidebars = match list_sidebars(&client) {
        Ok(sidebars) => sidebars,
        Err(_) => return 1,
    };
    if let Some(sidebar) = canonical_sidebar(&sidebars, window_id) {
        if ensure_enabled_intent(&client).is_err() {
            return 1;
        }
        if consolidate_sidebars(&client, &sidebars, &sidebar.pane_id).is_err() {
            return 1;
        }
        if recover_sidebar_process(&client, &sidebar).is_err() {
            return 1;
        }
        if create_only {
            return 0;
        }
        if sidebar.window_id != window_id {
            return move_sidebar(&client, &sidebar, window_id, caller_pane, true).is_err() as i32;
        }
        if caller_pane.is_some_and(|pane| pane != sidebar.pane_id) {
            return focus_existing_sidebar(&client, &sidebar.pane_id, caller_pane).is_err() as i32;
        }
        return disable_sidebar(&client, Some(window_id), caller_pane).is_err() as i32;
    }

    if set_enabled_intent(&client, true).is_err() {
        return 1;
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

pub(crate) fn cmd_follow(args: &[String]) -> i32 {
    let Ok(_lock) = acquire_lifecycle_lock() else {
        return 1;
    };
    follow_sidebar(
        &LiveTmux,
        args.first().map(String::as_str).unwrap_or_default(),
        args.get(1).map(String::as_str).unwrap_or_default(),
    )
    .is_err() as i32
}

pub(crate) fn cmd_close(args: &[String]) -> i32 {
    let Ok(_lock) = acquire_lifecycle_lock() else {
        return 1;
    };
    let window_id = args.first().map(String::as_str).unwrap_or_default();
    let caller_pane = args
        .get(1)
        .map(String::as_str)
        .filter(|pane| !pane.is_empty());
    let client = LiveTmux;
    disable_sidebar(&client, Some(window_id), caller_pane).is_err() as i32
}

pub(crate) fn cmd_maintain(args: &[String]) -> i32 {
    let Ok(_lock) = acquire_lifecycle_lock() else {
        return 1;
    };
    maintain_sidebar(&LiveTmux, args).is_err() as i32
}

fn maintain_sidebar(client: &impl TmuxClient, args: &[String]) -> Result<(), String> {
    topology::cleanup_slot_only_windows(client)?;
    let sidebars = list_sidebars(client)?;
    match enabled_intent(client)? {
        SidebarIntent::Disabled => return disable_sidebar(client, None, None),
        SidebarIntent::Enabled => {}
        SidebarIntent::Unset => {
            return follow_sidebar(
                client,
                args.first().map(String::as_str).unwrap_or_default(),
                args.get(1).map(String::as_str).unwrap_or_default(),
            );
        }
    }
    if let Some(sidebar) = canonical_sidebar(&sidebars, "") {
        if recover_sidebar_process(client, &sidebar).is_err() {
            return Err("failed to recover sidebar process".into());
        }
        match topology::window_has_ordinary_pane(client, &sidebar.window_id) {
            Ok(true) => {}
            Ok(false) => match topology::first_ordinary_target(client, &sidebar.window_id) {
                Ok(Some((window_id, pane_id))) => {
                    if move_sidebar(client, &sidebar, &window_id, Some(&pane_id), false).is_err() {
                        return Err("failed to move sidebar to an ordinary pane".into());
                    }
                }
                Ok(None) => {
                    let _ = client.run(&["kill-pane", "-t", &sidebar.pane_id]);
                    return Ok(());
                }
                Err(error) => return Err(error),
            },
            Err(error) => return Err(error),
        }
    }
    follow_sidebar(
        client,
        args.first().map(String::as_str).unwrap_or_default(),
        args.get(1).map(String::as_str).unwrap_or_default(),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SidebarPane {
    pane_id: String,
    window_id: String,
    process_alive: bool,
}

fn recover_sidebar_process(client: &impl TmuxClient, sidebar: &SidebarPane) -> Result<(), String> {
    if sidebar.process_alive {
        return Ok(());
    }
    let self_bin =
        std::env::current_exe().map_err(|_| "failed to resolve current executable".to_string())?;
    let self_bin = self_bin
        .to_str()
        .ok_or_else(|| "current executable path is not valid UTF-8".to_string())?;
    let shell_command = crate::cli::setup::shell_quote(self_bin);
    mark_sidebar_owned(client, &sidebar.pane_id)?;
    client
        .run(&["respawn-pane", "-k", "-t", &sidebar.pane_id, &shell_command])
        .ok_or_else(|| format!("failed to recover sidebar pane {}", sidebar.pane_id))?;
    Ok(())
}

fn set_enabled_intent(client: &impl TmuxClient, enabled: bool) -> Result<(), String> {
    client
        .run(&[
            "set-option",
            "-g",
            tmux::SIDEBAR_ENABLED,
            if enabled { "on" } else { "off" },
        ])
        .ok_or_else(|| "failed to update sidebar enabled state".to_string())?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SidebarIntent {
    Enabled,
    Disabled,
    Unset,
}

fn enabled_intent(client: &impl TmuxClient) -> Result<SidebarIntent, String> {
    client
        .run(&["show-option", "-gqv", tmux::SIDEBAR_ENABLED])
        .map(|value| match value.trim() {
            "on" => SidebarIntent::Enabled,
            "off" => SidebarIntent::Disabled,
            _ => SidebarIntent::Unset,
        })
        .ok_or_else(|| "failed to query sidebar enabled state".to_string())
}

fn ensure_enabled_intent(client: &impl TmuxClient) -> Result<(), String> {
    if enabled_intent(client)? == SidebarIntent::Enabled {
        return Ok(());
    }
    set_enabled_intent(client, true)
}

fn list_sidebars(client: &impl TmuxClient) -> Result<Vec<SidebarPane>, String> {
    topology::list_sidebars(client)
}

fn canonical_sidebar(sidebars: &[SidebarPane], preferred_window: &str) -> Option<SidebarPane> {
    sidebars
        .iter()
        .find(|sidebar| sidebar.window_id == preferred_window)
        .or_else(|| sidebars.first())
        .cloned()
}

fn consolidate_sidebars(
    client: &impl TmuxClient,
    sidebars: &[SidebarPane],
    canonical_pane: &str,
) -> Result<(), String> {
    for sidebar in sidebars {
        if sidebar.pane_id != canonical_pane {
            discard_sidebar_pane(client, sidebar)?;
        }
    }
    Ok(())
}

fn follow_sidebar(
    client: &impl TmuxClient,
    requested_client: &str,
    requested_window: &str,
) -> Result<(), String> {
    let clients = client
        .run(&["list-clients", "-F", "#{client_name}"])
        .ok_or_else(|| "failed to query attached clients".to_string())?;
    let clients: Vec<&str> = clients.lines().filter(|name| !name.is_empty()).collect();
    if clients.len() != 1 {
        return Ok(());
    }
    let client_name = if requested_client.is_empty() {
        clients[0]
    } else if clients[0] == requested_client {
        requested_client
    } else {
        return Ok(());
    };
    let (window_id, caller_pane) = if requested_window.is_empty() {
        let target = client
            .run(&[
                "display-message",
                "-c",
                client_name,
                "-p",
                "#{window_id}|#{pane_id}",
            ])
            .ok_or_else(|| "failed to resolve client window".to_string())?;
        let Some((window_id, caller_pane)) = target.trim().split_once('|') else {
            return Err("invalid client window response".into());
        };
        (window_id.to_string(), caller_pane.to_string())
    } else {
        if requested_client.is_empty() {
            let client_session = client
                .run(&["display-message", "-c", client_name, "-p", "#{session_id}"])
                .unwrap_or_default();
            let target_session = client.display(requested_window, "#{session_id}");
            if client_session.trim() != target_session {
                return Ok(());
            }
        }
        let caller_pane = client.display(requested_window, "#{pane_id}");
        if caller_pane.is_empty() {
            return Err("failed to resolve hook window pane".into());
        }
        (requested_window.to_string(), caller_pane)
    };
    let sidebars = list_sidebars(client)?;
    match enabled_intent(client)? {
        SidebarIntent::Disabled => return Ok(()),
        SidebarIntent::Enabled => {}
        SidebarIntent::Unset if sidebars.is_empty() => return Ok(()),
        SidebarIntent::Unset => set_enabled_intent(client, true)?,
    }
    let Some(sidebar) = canonical_sidebar(&sidebars, &window_id) else {
        let pane_path = client.display(&caller_pane, "#{pane_current_path}");
        let self_bin = std::env::current_exe()
            .ok()
            .and_then(|path| path.to_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "tmux-agent-sidebar".to_string());
        open_sidebar(
            client,
            OpenRequest {
                window_id: &window_id,
                pane_path: if pane_path.is_empty() {
                    "~"
                } else {
                    &pane_path
                },
                caller_pane: Some(&caller_pane),
                focus_sidebar: false,
                self_bin: &self_bin,
            },
        )?;
        return Ok(());
    };
    consolidate_sidebars(client, &sidebars, &sidebar.pane_id)?;
    recover_sidebar_process(client, &sidebar)?;
    if sidebar.window_id == window_id {
        notify_sidebar(client, &sidebar.pane_id);
        return Ok(());
    }
    move_sidebar(client, &sidebar, &window_id, Some(&caller_pane), false)
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
    let active_pane = client.display(window_id, "#{pane_id}");
    let requested_return = request.caller_pane.unwrap_or(&active_pane);
    let saved_zoom = client.display(window_id, "#{window_zoomed_flag}");
    if !matches!(saved_zoom.as_str(), "0" | "1")
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

    let target = topology::ensure_slot(client, window_id, requested_return).inspect_err(|_| {
        clear_saved_state(client, window_id);
        restore_pane_and_zoom(client, requested_return, &saved_zoom);
    })?;
    let sidebar_pane = target.slot.pane_id;
    let return_pane = target.return_pane;
    if mark_sidebar_owned(client, &sidebar_pane).is_err() {
        clear_saved_state(client, window_id);
        restore_pane_and_zoom(client, &return_pane, &saved_zoom);
        return Err("failed to mark sidebar ownership".into());
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
        clear_saved_state(client, window_id);
        restore_pane_and_zoom(client, &return_pane, &saved_zoom);
        return Err("failed to mark sidebar pane".into());
    }
    let shell_command = crate::cli::setup::shell_quote(request.self_bin);
    if client
        .run(&[
            "respawn-pane",
            "-k",
            "-c",
            request.pane_path,
            "-t",
            &sidebar_pane,
            &shell_command,
        ])
        .is_none()
    {
        let _ = client.run(&[
            "set-option",
            "-p",
            "-t",
            &sidebar_pane,
            tmux::PANE_ROLE,
            "sidebar-slot",
        ]);
        clear_saved_state(client, window_id);
        restore_pane_and_zoom(client, &return_pane, &saved_zoom);
        return Err("failed to start sidebar pane".into());
    }
    if !return_pane.is_empty() {
        let _ = client.run(&[
            "set-option",
            "-p",
            "-t",
            &sidebar_pane,
            tmux::SIDEBAR_RETURN_PANE,
            &return_pane,
        ]);
    }
    let focus_target = if request.focus_sidebar || active_pane.is_empty() {
        sidebar_pane.as_str()
    } else {
        active_pane.as_str()
    };
    if client.run(&["select-pane", "-t", focus_target]).is_none() {
        let _ = close_sidebar(client, window_id, Some(&return_pane));
        return Err("failed to select sidebar target".into());
    }
    Ok(sidebar_pane)
}

pub(super) fn resolve_sidebar_width(client: &impl TmuxClient, window_id: &str) -> String {
    let setting = client.display(window_id, &format!("#{{{}}}", tmux::SIDEBAR_WIDTH));
    let setting = if setting.is_empty() {
        DEFAULT_SIDEBAR_WIDTH_COLUMNS.to_string()
    } else {
        setting
    };
    let Some(percent) = setting.strip_suffix('%') else {
        return setting;
    };
    let window_width = client
        .display(window_id, "#{window_width}")
        .parse::<u32>()
        .ok();
    let percent = percent.parse::<u32>().ok();
    match (window_width, percent) {
        (Some(window_width), Some(percent)) if window_width > 0 && percent > 0 => {
            (window_width.saturating_mul(percent) / 100)
                .max(1)
                .to_string()
        }
        _ => setting,
    }
}

fn move_sidebar(
    client: &impl TmuxClient,
    sidebar: &SidebarPane,
    target_window: &str,
    caller_pane: Option<&str>,
    focus_sidebar: bool,
) -> Result<(), String> {
    let target_active = client.display(target_window, "#{pane_id}");
    let mut target_return = caller_pane
        .filter(|pane| **pane != sidebar.pane_id)
        .unwrap_or(&target_active)
        .to_string();
    let target_zoom = client.display(target_window, "#{window_zoomed_flag}");
    if !matches!(target_zoom.as_str(), "0" | "1") {
        return Err("failed to query target zoom state".into());
    }

    let source_active = client.display(&sidebar.window_id, "#{pane_id}");
    let stored_return = show_pane_option(client, &sidebar.pane_id, tmux::SIDEBAR_RETURN_PANE);
    let source_return = if !source_active.is_empty() && source_active != sidebar.pane_id {
        source_active.clone()
    } else {
        stored_return.clone()
    };
    let source_zoom = show_window_option(client, &sidebar.window_id, tmux::SIDEBAR_SAVED_ZOOM);
    let slot = match topology::ensure_slot(client, target_window, &target_return) {
        Ok(target) => {
            target_return = target.return_pane;
            target.slot
        }
        Err(error) => {
            restore_pane_and_zoom(client, &target_return, &target_zoom);
            return Err(error);
        }
    };

    if client
        .run(&[
            "set-option",
            "-w",
            "-t",
            target_window,
            tmux::SIDEBAR_SAVED_ZOOM,
            &target_zoom,
        ])
        .is_none()
    {
        restore_pane_and_zoom(client, &target_return, &target_zoom);
        return Err("failed to save target zoom state".into());
    }
    if !target_return.is_empty()
        && client
            .run(&[
                "set-option",
                "-p",
                "-t",
                &sidebar.pane_id,
                tmux::SIDEBAR_RETURN_PANE,
                &target_return,
            ])
            .is_none()
    {
        clear_saved_state(client, target_window);
        restore_pane_and_zoom(client, &target_return, &target_zoom);
        return Err("failed to update sidebar return pane".into());
    }

    if source_active == sidebar.pane_id {
        select_existing_pane(client, &source_return);
    }

    if client
        .run(&[
            "swap-pane",
            "-d",
            "-s",
            &sidebar.pane_id,
            "-t",
            &slot.pane_id,
        ])
        .is_none()
    {
        let sidebar_window = client.display(&sidebar.pane_id, "#{window_id}");
        if sidebar_window == target_window {
            // tmux committed the swap even though the client did not receive a
            // successful response. Reconcile from authoritative pane state.
        } else {
            clear_saved_state(client, target_window);
            if stored_return.is_empty() {
                let _ = client.run(&[
                    "set-option",
                    "-p",
                    "-u",
                    "-t",
                    &sidebar.pane_id,
                    tmux::SIDEBAR_RETURN_PANE,
                ]);
            } else {
                let _ = client.run(&[
                    "set-option",
                    "-p",
                    "-t",
                    &sidebar.pane_id,
                    tmux::SIDEBAR_RETURN_PANE,
                    &stored_return,
                ]);
            }
            restore_pane_and_zoom(client, &target_return, &target_zoom);
            if source_active == sidebar.pane_id {
                select_existing_pane(client, &sidebar.pane_id);
            }
            return Err("failed to swap sidebar pane".to_string());
        }
    }

    clear_saved_state(client, &sidebar.window_id);
    restore_pane_and_zoom(client, &source_return, &source_zoom);
    topology::cleanup_slot_only_windows(client)?;
    let focus_target = if focus_sidebar {
        sidebar.pane_id.as_str()
    } else {
        target_return.as_str()
    };
    select_existing_pane(client, focus_target);
    notify_sidebar(client, &sidebar.pane_id);
    Ok(())
}

fn select_existing_pane(client: &impl TmuxClient, pane: &str) {
    if pane_exists(client, pane) {
        let _ = client.run(&["select-pane", "-t", pane]);
    }
}

fn restore_pane_and_zoom(client: &impl TmuxClient, pane: &str, saved_zoom: &str) {
    if !pane_exists(client, pane) {
        return;
    }
    let _ = client.run(&["select-pane", "-t", pane]);
    if saved_zoom == "1" {
        let zoomed = client.display(pane, "#{window_zoomed_flag}");
        if zoomed != "1" {
            let _ = client.run(&["resize-pane", "-Z", "-t", pane]);
        }
    }
}

fn notify_sidebar(client: &impl TmuxClient, sidebar_pane: &str) {
    let pid = client.display(sidebar_pane, &format!("#{{{}}}", tmux::SIDEBAR_PID));
    if pid.parse::<u32>().is_ok() {
        let command = format!("kill -USR1 {pid} 2>/dev/null; true");
        let _ = client.run(&["run-shell", &command]);
    }
}

fn mark_sidebar_owned(client: &impl TmuxClient, sidebar_pane: &str) -> Result<(), String> {
    client
        .run(&[
            "set-option",
            "-p",
            "-t",
            sidebar_pane,
            tmux::SIDEBAR_OWNER,
            topology::SIDEBAR_OWNER_TOKEN,
        ])
        .ok_or_else(|| format!("failed to mark sidebar ownership on {sidebar_pane}"))?;
    Ok(())
}

pub(crate) fn cmd_toggle_all(args: &[String]) -> i32 {
    cmd_toggle(args)
}

pub(crate) fn cmd_restart_sidebars(_args: &[String]) -> i32 {
    let Ok(_lock) = acquire_lifecycle_lock() else {
        return 1;
    };
    restart_sidebars(&LiveTmux).is_err() as i32
}

fn restart_sidebars(client: &impl TmuxClient) -> Result<(), String> {
    let sidebars = list_sidebars(client)?;
    client
        .run(&["set-option", "-g", tmux::SIDEBAR_FILTER, "all"])
        .ok_or_else(|| "failed to reset sidebar status filter".to_string())?;
    let Some(sidebar) = preferred_sidebar_for_restart(client, &sidebars) else {
        if enabled_intent(client)? == SidebarIntent::Enabled {
            return follow_sidebar(client, "", "");
        }
        return Ok(());
    };
    ensure_enabled_intent(client)?;
    consolidate_sidebars(client, &sidebars, &sidebar.pane_id)?;

    let self_bin =
        std::env::current_exe().map_err(|_| "failed to resolve current executable".to_string())?;
    let self_bin = self_bin
        .to_str()
        .ok_or_else(|| "current executable path is not valid UTF-8".to_string())?;
    let shell_command = crate::cli::setup::shell_quote(self_bin);
    mark_sidebar_owned(client, &sidebar.pane_id)?;
    client
        .run(&["respawn-pane", "-k", "-t", &sidebar.pane_id, &shell_command])
        .ok_or_else(|| format!("failed to restart sidebar pane {}", sidebar.pane_id))?;
    Ok(())
}

fn preferred_sidebar_for_restart(
    client: &impl TmuxClient,
    sidebars: &[SidebarPane],
) -> Option<SidebarPane> {
    let clients = client
        .run(&["list-clients", "-F", "#{client_name}"])
        .unwrap_or_default();
    let clients: Vec<&str> = clients.lines().filter(|name| !name.is_empty()).collect();
    if clients.len() == 1 {
        let window_id = client
            .run(&["display-message", "-c", clients[0], "-p", "#{window_id}"])?
            .trim()
            .to_string();
        return canonical_sidebar(sidebars, &window_id);
    }
    sidebars.first().cloned()
}

fn find_sidebar(client: &impl TmuxClient, window_id: &str) -> Result<Option<String>, String> {
    let format = format!("#{{pane_id}}|#{{{}}}", tmux::PANE_ROLE);
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
    close_sidebar_pane(
        client,
        &SidebarPane {
            pane_id: sidebar_pane,
            window_id: window_id.to_string(),
            process_alive: true,
        },
        caller_pane,
    )
}

fn disable_sidebar(
    client: &impl TmuxClient,
    preferred_window: Option<&str>,
    caller_pane: Option<&str>,
) -> Result<(), String> {
    // Intent changes first so cleanup-triggered hooks cannot recreate panes.
    set_enabled_intent(client, false)?;
    let sidebars = list_sidebars(client)?;
    let slot_result = topology::cleanup_slots(client);
    let canonical = preferred_window
        .and_then(|window| canonical_sidebar(&sidebars, window))
        .or_else(|| sidebars.first().cloned());
    let mut close_result = Ok(());
    if let Some(canonical) = canonical {
        for sidebar in &sidebars {
            let result = if sidebar.pane_id == canonical.pane_id {
                close_sidebar_pane(client, sidebar, caller_pane)
            } else {
                discard_sidebar_pane(client, sidebar)
            };
            if close_result.is_ok() {
                close_result = result;
            }
        }
    }
    slot_result.and(close_result)
}

fn close_sidebar_pane(
    client: &impl TmuxClient,
    sidebar: &SidebarPane,
    caller_pane: Option<&str>,
) -> Result<(), String> {
    let saved_zoom = show_window_option(client, &sidebar.window_id, tmux::SIDEBAR_SAVED_ZOOM);
    let return_pane = show_pane_option(client, &sidebar.pane_id, tmux::SIDEBAR_RETURN_PANE);
    let source_active = client.display(&sidebar.window_id, "#{pane_id}");
    let caller_in_source = caller_pane.filter(|pane| {
        **pane != sidebar.pane_id && client.display(pane, "#{window_id}") == sidebar.window_id
    });
    let mut target_pane = caller_in_source
        .or_else(|| {
            (!source_active.is_empty() && source_active != sidebar.pane_id)
                .then_some(source_active.as_str())
        })
        .unwrap_or(&return_pane)
        .to_string();

    let pane_count = client
        .display(&sidebar.window_id, "#{window_panes}")
        .parse::<usize>()
        .map_err(|_| "failed to query sidebar pane count".to_string())?;
    if pane_count == 1 {
        let cwd = client.display(&sidebar.pane_id, "#{pane_current_path}");
        target_pane = client
            .run(&[
                "split-window",
                "-d",
                "-t",
                &sidebar.pane_id,
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

    let kill_succeeded = client.run(&["kill-pane", "-t", &sidebar.pane_id]).is_some();
    if !kill_succeeded && pane_exists(client, &sidebar.pane_id) {
        return Err("failed to close sidebar pane".to_string());
    }
    clear_saved_state(client, &sidebar.window_id);
    restore_pane_and_zoom(client, &target_pane, &saved_zoom);
    Ok(())
}

fn discard_sidebar_pane(client: &impl TmuxClient, sidebar: &SidebarPane) -> Result<(), String> {
    let saved_zoom = show_window_option(client, &sidebar.window_id, tmux::SIDEBAR_SAVED_ZOOM);
    let return_pane = show_pane_option(client, &sidebar.pane_id, tmux::SIDEBAR_RETURN_PANE);
    let source_active = client.display(&sidebar.window_id, "#{pane_id}");
    let mut target_pane = if !source_active.is_empty() && source_active != sidebar.pane_id {
        source_active
    } else {
        return_pane
    };
    let pane_count = client
        .display(&sidebar.window_id, "#{window_panes}")
        .parse::<usize>()
        .map_err(|_| "failed to query duplicate sidebar pane count".to_string())?;
    if pane_count == 1 {
        let cwd = client.display(&sidebar.pane_id, "#{pane_current_path}");
        target_pane = client
            .run(&[
                "split-window",
                "-d",
                "-t",
                &sidebar.pane_id,
                "-c",
                if cwd.is_empty() { "~" } else { &cwd },
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .map(|pane| pane.trim().to_string())
            .filter(|pane| !pane.is_empty())
            .ok_or_else(|| "failed to create duplicate replacement pane".to_string())?;
    }
    let kill_succeeded = client.run(&["kill-pane", "-t", &sidebar.pane_id]).is_some();
    if !kill_succeeded && pane_exists(client, &sidebar.pane_id) {
        return Err("failed to discard duplicate sidebar pane".to_string());
    }
    clear_saved_state(client, &sidebar.window_id);
    if saved_zoom == "1"
        && !target_pane.is_empty()
        && client.display(&sidebar.window_id, "#{window_zoomed_flag}") != "1"
    {
        let _ = client.run(&["resize-pane", "-Z", "-t", &target_pane]);
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
            .is_some_and(|pane_id| !pane_id.trim().is_empty())
}

/// Which side of the window the sidebar pane is created on, driven by
/// the `@sidebar_position` tmux option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SidebarPosition {
    Left,
    Right,
}

impl SidebarPosition {
    /// Parse the raw `@sidebar_position` option value. Only an explicit
    /// (case-insensitive, whitespace-tolerant) `right` selects the right
    /// side; everything else — including unset, empty, or invalid values
    /// — falls back to the historical default of `left`, so a typo never
    /// moves the sidebar somewhere unexpected.
    pub(super) fn from_setting(setting: &str) -> Self {
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
pub(super) fn target_pane_for_position(output: &str, position: SidebarPosition) -> Option<String> {
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
pub(super) fn split_window_flags(position: SidebarPosition) -> &'static str {
    match position {
        SidebarPosition::Left => "-hfb",
        SidebarPosition::Right => "-hf",
    }
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
    fn open_saves_zoom_marks_return_pane_and_focuses_sidebar() {
        let tmux = FixtureTmux::with_responses([
            Some("%1"),
            Some("1"),
            Some(""),
            Some("%1|@1||101|0|/dev/ttys001"),
            Some("left"),
            Some("0 120 %1"),
            Some("35"),
            Some("%9"),
            Some(""),
            Some("%1|@1||101|0|/dev/ttys001\n%9|@1|sidebar-slot|0|0|"),
            Some(""),
            Some(""),
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
            "-d",
            "-l",
            "35",
            "-t",
            "%1",
            "-P",
            "-F",
            "#{pane_id}",
            "",
        ]));
        assert!(tmux.called(&[
            "respawn-pane",
            "-k",
            "-c",
            "/repo",
            "-t",
            "%9",
            "/bin/sidebar",
        ]));
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| { call.iter().any(|arg| arg == tmux::SIDEBAR_SAVED_LAYOUT) })
        );
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
            Some("%1"),
            Some("0"),
            Some(""),
            Some("%1|@1||101|0|/dev/ttys001"),
            Some("left"),
            Some("0 120 %1"),
            Some("35"),
            Some("%9"),
            Some(""),
            Some("%1|@1||101|0|/dev/ttys001\n%9|@1|sidebar-slot|0|0|"),
            Some(""),
            Some(""),
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

    #[test]
    fn dead_sidebar_pane_is_respawned_in_place() {
        let tmux = FixtureTmux::with_responses([Some(""), Some("")]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: false,
        };

        recover_sidebar_process(&tmux, &sidebar).unwrap();

        assert!(tmux.calls.borrow().iter().any(|call| {
            call.first().map(String::as_str) == Some("respawn-pane")
                && call.get(3).map(String::as_str) == Some("%9")
        }));
    }

    fn close_fixture(target_exists: bool) -> FixtureTmux {
        FixtureTmux::with_responses([
            Some("%9|sidebar"),
            Some("1"),
            Some("%1"),
            Some("%9"),
            Some("2"),
            Some(""),
            Some(""),
            Some(""),
            target_exists.then_some("%1"),
            target_exists.then_some(""),
            target_exists.then_some("0"),
            target_exists.then_some(""),
        ])
    }

    #[test]
    fn close_restores_return_pane_and_zoom_without_replaying_layout() {
        let tmux = close_fixture(true);
        close_sidebar(&tmux, "@1", Some("%9")).unwrap();
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "select-layout")
        );
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
            Some("0"),
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
            Some("%1"),
            Some("0"),
            Some(""),
            Some("%1|@1||101|0|/dev/ttys001"),
            Some("left"),
            Some("0 120 %1"),
            Some("35"),
            Some("%9"),
            Some(""),
            Some("%1|@1||101|0|/dev/ttys001\n%9|@1|sidebar-slot|0|0|"),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            None,
            Some("%9|sidebar"),
            Some("0"),
            Some("%1"),
            Some("%9"),
            Some("@1"),
            Some("2"),
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
        assert!(
            tmux.called(&["kill-pane", "-t", "%9"]),
            "calls: {:?}",
            tmux.calls.borrow()
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
    fn close_last_sidebar_creates_shell_replacement_first() {
        let tmux = FixtureTmux::with_responses([
            Some("%9|sidebar"),
            Some("0"),
            Some(""),
            Some("%9"),
            Some("1"),
            Some("/repo"),
            Some("%10"),
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
    fn close_last_sidebar_preserves_replacement_when_kill_fails() {
        let tmux = FixtureTmux::with_responses([
            Some("0"),
            Some(""),
            Some("%9"),
            Some("1"),
            Some("/repo"),
            Some("%10"),
            None,
            Some("%9"),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        assert!(close_sidebar_pane(&tmux, &sidebar, None).is_err());
        assert!(tmux.called(&["kill-pane", "-t", "%9"]));
        assert!(!tmux.called(&["kill-pane", "-t", "%10"]));
    }

    #[test]
    fn discard_last_sidebar_preserves_replacement_when_kill_fails() {
        let tmux = FixtureTmux::with_responses([
            Some("1"),
            Some(""),
            Some("%9"),
            Some("1"),
            Some("/repo"),
            Some("%10"),
            None,
            Some("%9"),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        assert!(discard_sidebar_pane(&tmux, &sidebar).is_err());
        assert!(tmux.called(&["kill-pane", "-t", "%9"]));
        assert!(!tmux.called(&["kill-pane", "-t", "%10"]));
    }

    #[test]
    fn close_last_sidebar_preserves_replacement_when_sidebar_disappears() {
        let tmux = FixtureTmux::with_responses([
            Some("0"),
            Some(""),
            Some("%9"),
            Some("1"),
            Some("/repo"),
            Some("%10"),
            None,
            None,
            Some(""),
            Some(""),
            Some("%10"),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        close_sidebar_pane(&tmux, &sidebar, None).unwrap();
        assert!(!tmux.called(&["kill-pane", "-t", "%10"]));
    }

    #[test]
    fn discard_last_sidebar_preserves_replacement_when_sidebar_disappears() {
        let tmux = FixtureTmux::with_responses([
            Some("0"),
            Some(""),
            Some("%9"),
            Some("1"),
            Some("/repo"),
            Some("%10"),
            None,
            None,
            Some(""),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        discard_sidebar_pane(&tmux, &sidebar).unwrap();
        assert!(!tmux.called(&["kill-pane", "-t", "%10"]));
    }

    #[test]
    fn list_sidebars_filters_roles_and_deduplicates_linked_windows() {
        let tmux = FixtureTmux::with_responses([Some(
            "%1|@1||101|0|/dev/ttys001\n%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%9|@2|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%2|@2|main|102|0|/dev/ttys002",
        )]);
        assert_eq!(
            list_sidebars(&tmux).unwrap(),
            vec![SidebarPane {
                pane_id: "%9".into(),
                window_id: "@1".into(),
                process_alive: true,
            }]
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

    #[test]
    fn sidebar_width_uses_configured_columns() {
        let tmux = FixtureTmux::with_responses([Some("42")]);
        assert_eq!(resolve_sidebar_width(&tmux, "@1"), "42");
    }

    #[test]
    fn sidebar_width_resolves_configured_percentage() {
        let tmux = FixtureTmux::with_responses([Some("20%"), Some("180")]);
        assert_eq!(resolve_sidebar_width(&tmux, "@1"), "36");
    }

    #[test]
    fn sidebar_width_uses_named_default_when_option_is_unset() {
        let tmux = FixtureTmux::with_responses([Some("")]);
        assert_eq!(
            resolve_sidebar_width(&tmux, "@1"),
            DEFAULT_SIDEBAR_WIDTH_COLUMNS.to_string()
        );
    }

    #[test]
    fn move_sidebar_reuses_owned_slot_and_preserves_focus() {
        let tmux = FixtureTmux::with_responses([
            Some("%2"),
            Some("0"),
            Some("%9"),
            Some("%1"),
            Some("0"),
            Some(
                "%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%1|@1||101|0|/dev/ttys001\n%3|@2|sidebar-slot|0|0|\n%2|@2||102|0|/dev/ttys002",
            ),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
            Some(
                "%3|@1|sidebar-slot|0|0|\n%1|@1||101|0|/dev/ttys001\n%9|@2|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%2|@2||102|0|/dev/ttys002",
            ),
            Some("%2"),
            Some(""),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        move_sidebar(&tmux, &sidebar, "@2", Some("%2"), false).unwrap();

        assert!(tmux.called(&["swap-pane", "-d", "-s", "%9", "-t", "%3",]));
        assert!(tmux.called(&["select-pane", "-t", "%1"]));
        assert!(tmux.called(&["select-pane", "-t", "%2"]));
    }

    #[test]
    fn move_sidebar_defers_target_zoom_until_sidebar_leaves() {
        let tmux = FixtureTmux::with_responses([
            Some("%2"),
            Some("1"),
            Some("%1"),
            Some("%1"),
            Some("0"),
            Some(
                "%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%1|@1||101|0|/dev/ttys001\n%3|@2|sidebar-slot|0|0|\n%2|@2||102|0|/dev/ttys002",
            ),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
            Some(
                "%3|@1|sidebar-slot|0|0|\n%1|@1||101|0|/dev/ttys001\n%9|@2|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%2|@2||102|0|/dev/ttys002",
            ),
            Some("%2"),
            Some(""),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        move_sidebar(&tmux, &sidebar, "@2", Some("%2"), false).unwrap();

        assert!(!tmux.calls.borrow().iter().any(|call| {
            call.first().map(String::as_str) == Some("resize-pane")
                && call.iter().any(|arg| arg == "-Z")
        }));
        assert!(tmux.called(&[
            "set-option",
            "-w",
            "-t",
            "@2",
            tmux::SIDEBAR_SAVED_ZOOM,
            "1",
        ]));
    }

    #[test]
    fn move_failure_rolls_back_metadata_and_preserves_source_replacement() {
        let tmux = FixtureTmux::with_responses([
            Some("%2"),
            Some("0"),
            Some("%9"),
            Some("%1"),
            Some("1"),
            Some(
                "%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%1|@1||101|0|/dev/ttys001\n%3|@2|sidebar-slot|0|0|\n%2|@2||102|0|/dev/ttys002",
            ),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
            None,
            Some("@1"),
            Some(""),
            Some(""),
            Some(""),
            Some("%2"),
            Some(""),
            Some("%9"),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        assert!(move_sidebar(&tmux, &sidebar, "@2", Some("%2"), false).is_err());
        assert!(!tmux.called(&["kill-pane", "-t", "%10"]));
        assert!(tmux.called(&[
            "set-option",
            "-p",
            "-t",
            "%9",
            tmux::SIDEBAR_RETURN_PANE,
            "%1",
        ]));
        assert!(!tmux.called(&["kill-pane", "-t", "%9"]));
    }

    #[test]
    fn move_reconciles_committed_pane_after_missing_response() {
        let tmux = FixtureTmux::with_responses([
            Some("%2"),
            Some("0"),
            Some("%1"),
            Some("%1"),
            Some("0"),
            Some(
                "%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%1|@1||101|0|/dev/ttys001\n%3|@2|sidebar-slot|0|0|\n%2|@2||102|0|/dev/ttys002",
            ),
            Some(""),
            Some(""),
            None,
            Some("@2"),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
            Some(
                "%3|@1|sidebar-slot|0|0|\n%1|@1||101|0|/dev/ttys001\n%9|@2|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%2|@2||102|0|/dev/ttys002",
            ),
            Some("%2"),
            Some(""),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        move_sidebar(&tmux, &sidebar, "@2", Some("%2"), false).unwrap();
        assert!(tmux.called(&["select-pane", "-t", "%1"]));
        assert!(tmux.called(&["select-pane", "-t", "%2"]));
    }

    #[test]
    fn move_failure_preserves_source_and_slot_when_sidebar_disappears() {
        let tmux = FixtureTmux::with_responses([
            Some("%2"),
            Some("0"),
            Some("%9"),
            Some("%1"),
            Some("1"),
            Some(
                "%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%1|@1||101|0|/dev/ttys001\n%3|@2|sidebar-slot|0|0|\n%2|@2||102|0|/dev/ttys002",
            ),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
            None,
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some("%2"),
            Some(""),
            Some("%9"),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        assert!(move_sidebar(&tmux, &sidebar, "@2", Some("%2"), false).is_err());
        assert!(!tmux.called(&["kill-pane", "-t", "%9"]));
        assert!(tmux.called(&[
            "set-option",
            "-w",
            "-u",
            "-t",
            "@2",
            tmux::SIDEBAR_SAVED_ZOOM,
        ]));
    }

    #[test]
    fn close_from_another_window_restores_zoom_only_in_sidebar_window() {
        let tmux = FixtureTmux::with_responses([
            Some("1"),
            Some("%1"),
            Some("%9"),
            Some("@2"),
            Some("2"),
            Some(""),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
            Some("0"),
            Some(""),
        ]);
        let sidebar = SidebarPane {
            pane_id: "%9".into(),
            window_id: "@1".into(),
            process_alive: true,
        };

        close_sidebar_pane(&tmux, &sidebar, Some("%2")).unwrap();

        assert!(tmux.called(&["resize-pane", "-Z", "-t", "%1"]));
        assert!(!tmux.called(&["resize-pane", "-Z", "-t", "%2"]));
    }

    #[test]
    fn follow_noops_when_multiple_clients_are_attached() {
        let tmux = FixtureTmux::with_responses([Some("client-a\nclient-b")]);
        follow_sidebar(&tmux, "client-a", "@2").unwrap();
        assert_eq!(tmux.calls.borrow().len(), 1);
    }

    #[test]
    fn maintain_disabled_intent_removes_stale_sidebar_and_slots() {
        let topology = "%1|@1||101|0|/dev/ttys001|\n%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%3|@2|sidebar-slot|0|0||\n%2|@2||102|0|/dev/ttys002|";
        let tmux = FixtureTmux::with_responses([
            Some(topology),
            Some(topology),
            Some("off"),
            Some(""),
            Some(topology),
            Some(topology),
            Some(""),
            Some("0"),
            Some("%1"),
            Some("%1"),
            Some("2"),
            Some(""),
            Some(""),
            Some(""),
            Some("%1"),
            Some(""),
        ]);

        maintain_sidebar(&tmux, &[]).unwrap();

        assert!(tmux.called(&["kill-pane", "-t", "%3"]));
        assert!(tmux.called(&["kill-pane", "-t", "%9"]));
    }

    #[test]
    fn follow_preserves_width_when_sidebar_is_already_visible() {
        let tmux = FixtureTmux::with_responses([
            Some("client-a"),
            Some("%2"),
            Some(
                "%9|@2|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%2|@2||102|0|/dev/ttys002",
            ),
            Some("on"),
            Some(""),
        ]);
        follow_sidebar(&tmux, "client-a", "@2").unwrap();
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "resize-pane")
        );
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "swap-pane")
        );
    }

    #[test]
    fn restart_respawns_in_place_and_resets_status_filter() {
        let tmux = FixtureTmux::with_responses([
            Some(
                "%1|@1||101|0|/dev/ttys001\n%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar",
            ),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
        ]);

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
    fn restart_keeps_visible_sidebar_and_discards_duplicates_without_selecting() {
        let tmux = FixtureTmux::with_responses([
            Some(
                "%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar\n%10|@2|sidebar|901|0|/dev/ttys010|901|tmux-agent-sidebar",
            ),
            Some(""),
            Some("client-a"),
            Some("@2"),
            Some(""),
            Some(""),
            Some("0"),
            Some("%1"),
            Some("%1"),
            Some("2"),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
        ]);

        restart_sidebars(&tmux).unwrap();

        assert!(tmux.called(&["kill-pane", "-t", "%9"]));
        assert!(tmux.calls.borrow().iter().any(|call| {
            call.first().map(String::as_str) == Some("respawn-pane")
                && call.get(3).map(String::as_str) == Some("%10")
        }));
        assert!(
            !tmux
                .calls
                .borrow()
                .iter()
                .any(|call| call[0] == "select-pane")
        );
    }

    #[test]
    fn restart_reports_respawn_failure() {
        let tmux = FixtureTmux::with_responses([
            Some("%9|@1|sidebar|900|0|/dev/ttys009|900|tmux-agent-sidebar"),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            Some(""),
            None,
        ]);
        assert!(restart_sidebars(&tmux).is_err());
    }
}
