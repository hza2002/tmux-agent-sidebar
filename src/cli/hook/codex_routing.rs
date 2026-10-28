//! Codex compatibility routing: titles select clients, hooks own lifecycle.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::adapter::codex::identity::{self, TitleIdentity};
use crate::event::AgentEvent;
use crate::pane_lock::PaneHookLock;
use crate::process::ProcessSnapshot;
use crate::tmux;

use crate::adapter::codex::control as transport;

const FORMAT: &str = "#{q:pane_id}|#{q:pane_pid}|#{q:pane_title}|#{q:@pane_session_id}|#{q:@pane_turn_id}|#{q:@pane_completed_turn_id}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneObservation {
    pane: String,
    pid: u32,
    title: String,
    session: String,
    turn: String,
    completed: String,
}

fn parse_observation(line: &str) -> Option<PaneObservation> {
    let fields = tmux::split_tmux_fields(line, '|');
    if fields.len() != 6 || !fields[0].starts_with('%') {
        return None;
    }
    Some(PaneObservation {
        pane: fields[0].clone(),
        pid: fields[1].parse().ok()?,
        title: fields[2].clone(),
        session: fields[3].clone(),
        turn: fields[4].clone(),
        completed: fields[5].clone(),
    })
}

fn tmux_read(socket: &Path, args: &[&str], deadline: Instant) -> Result<String, String> {
    crate::bounded_command::output(
        Command::new("tmux").arg("-S").arg(socket).args(args),
        deadline,
    )
}

fn processes(deadline: Instant) -> Result<ProcessSnapshot, String> {
    crate::bounded_command::output(
        Command::new("ps").args(["-ww", "-eo", "pid=,ppid=,comm=,args="]),
        deadline,
    )
    .map(|text| ProcessSnapshot::from_ps_output(&text))
}

fn event_session(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::SessionStart { session_id, .. }
        | AgentEvent::UserPromptSubmit { session_id, .. }
        | AgentEvent::Stop { session_id, .. }
        | AgentEvent::PermissionRequest { session_id, .. }
        | AgentEvent::ActivityLog { session_id, .. } => session_id.as_deref(),
        _ => None,
    }
}

fn selected(title: &str, session: &str, ids: Option<&[String]>) -> bool {
    match identity::title_identity(title) {
        Some(TitleIdentity::Full(id)) => id == session,
        Some(TitleIdentity::Prefix(prefix)) => ids.is_some_and(|ids| {
            let mut matches = ids.iter().filter(|id| id.starts_with(&prefix));
            matches.next().is_some_and(|id| id == session) && matches.next().is_none()
        }),
        None => false,
    }
}

fn same_session_start_has_turn(event: &AgentEvent, pane: &PaneObservation) -> bool {
    matches!(event, AgentEvent::SessionStart { .. })
        && event_session(event) == Some(pane.session.as_str())
        && (!pane.turn.is_empty() || !pane.completed.is_empty())
}

fn context_unchanged(before: &PaneObservation, after: &PaneObservation) -> bool {
    before.pane == after.pane
        && before.pid == after.pid
        && before.session == after.session
        && before.turn == after.turn
        && before.completed == after.completed
}

/// Separate protocol output from pane writes even when routing fails.
pub(super) fn dispatch(mut event: AgentEvent) -> i32 {
    let response = match &mut event {
        AgentEvent::Stop { response, .. } => response.take(),
        _ => None,
    };
    let deadline = Instant::now() + Duration::from_secs(1);
    let result = match std::env::var_os("SIDEBAR_CODEX_CONTROL_SOCKET") {
        Some(control) => route(&event, PathBuf::from(control), deadline),
        None => legacy(&event, deadline),
    };
    if let Err(error) = result {
        eprintln!("codex sidebar routing: {error}");
    }
    if let Some(response) = response {
        println!("{response}");
    }
    0
}

fn route(event: &AgentEvent, control: PathBuf, deadline: Instant) -> Result<(), String> {
    if !control.is_absolute() {
        return Err("control socket must be absolute; re-run setup codex".into());
    }
    let socket = crate::pane_lock::socket_path(std::env::var("TMUX").ok().as_deref())
        .ok_or("tmux server is not explicitly configured; re-run setup codex")?;
    let session = event_session(event)
        .filter(|id| identity::title_identity(id) == Some(TitleIdentity::Full((*id).into())))
        .ok_or("event has no canonical session UUID")?;
    let output = tmux_read(&socket, &["list-panes", "-a", "-F", FORMAT], deadline)?;
    let snapshot = processes(deadline)?;
    let mut panes: Vec<_> = output
        .lines()
        .filter_map(parse_observation)
        .filter_map(|pane| {
            let clients = identity::interactive_clients(&snapshot, pane.pid);
            (clients.len() == 1).then(|| (pane, clients[0]))
        })
        .collect();
    panes.sort_by(|a, b| a.0.pane.cmp(&b.0.pane));
    panes.dedup_by(|a, b| a.0.pane == b.0.pane);
    // Positive ancestry wins over titles, including when scoped hooks are used
    // by an embedded TUI. Never route that hook to another view of the same ID.
    let origin = identity::hook_origin(&snapshot, std::process::id());
    let embedded = match origin {
        identity::HookOrigin::Interactive(client) => Some(client),
        identity::HookOrigin::Unsupported => return Err("unsupported Codex hook ancestor".into()),
        _ => None,
    };
    panes.retain(|(_, client)| {
        embedded.map_or_else(
            || identity::client_requires_daemon(&snapshot, *client),
            |owner| *client == owner,
        )
    });
    observe_turn(
        event,
        embedded.is_some(),
        (origin == identity::HookOrigin::Server).then_some(control.as_path()),
        deadline,
    );
    let needs_ids = embedded.is_none() && panes.iter().any(|(pane,_)| matches!(identity::title_identity(&pane.title), Some(TitleIdentity::Prefix(ref prefix)) if session.starts_with(prefix)));
    let ids = if needs_ids {
        let lookup_deadline = deadline.min(Instant::now() + Duration::from_millis(250));
        match transport::loaded_thread_ids(&control, lookup_deadline) {
            Ok(ids) => Some(ids),
            Err(error) => {
                eprintln!("codex sidebar routing: loaded thread lookup failed: {error}");
                None
            }
        }
    } else {
        None
    };
    for (pane, client) in panes {
        if Instant::now() >= deadline {
            return Err("routing budget exhausted".into());
        }
        if embedded.is_none() && !selected(&pane.title, session, ids.as_deref()) {
            continue;
        }
        let Ok(_lock) = PaneHookLock::acquire(
            &pane.pane,
            Some(deadline.min(Instant::now() + Duration::from_millis(50))),
        ) else {
            eprintln!("codex sidebar routing: pane {} lock unavailable", pane.pane);
            continue;
        };
        let fresh = tmux_read(
            &socket,
            &["display-message", "-p", "-t", &pane.pane, FORMAT],
            deadline,
        )?;
        let Some(fresh) = parse_observation(fresh.trim_end()) else {
            continue;
        };
        if !context_unchanged(&pane, &fresh)
            || (embedded.is_none() && !selected(&fresh.title, session, ids.as_deref()))
        {
            continue;
        }
        let fresh_processes = processes(deadline)?;
        if identity::interactive_clients(&fresh_processes, fresh.pid) != vec![client] {
            continue;
        }
        if embedded.is_some()
            && !identity::hook_belongs_to_client(&fresh_processes, std::process::id(), client)
        {
            continue;
        }
        apply(
            event,
            &fresh,
            if embedded.is_some() {
                None
            } else {
                Some(client)
            },
        );
    }
    Ok(())
}

/// Legacy hooks remain usable only with positive in-process ancestry proof.
fn legacy(event: &AgentEvent, deadline: Instant) -> Result<(), String> {
    let pane = super::tmux_pane();
    if pane.is_empty() {
        return Err("missing daemon routing configuration".into());
    }
    let socket = crate::pane_lock::socket_path(std::env::var("TMUX").ok().as_deref())
        .ok_or("missing tmux socket")?;
    let output = tmux_read(
        &socket,
        &["display-message", "-p", "-t", &pane, FORMAT],
        deadline,
    )?;
    let observation = parse_observation(output.trim_end()).ok_or("invalid pane observation")?;
    let snapshot = processes(deadline)?;
    let proven = identity::interactive_clients(&snapshot, observation.pid)
        .into_iter()
        .find(|&client| identity::hook_belongs_to_client(&snapshot, std::process::id(), client));
    let Some(client) = proven else {
        return Err("daemon hooks require explicit sockets; re-run setup codex".into());
    };
    let _lock = PaneHookLock::acquire(
        &pane,
        Some(deadline.min(Instant::now() + Duration::from_millis(50))),
    )
    .map_err(|_| "pane lock unavailable")?;
    let fresh = tmux_read(
        &socket,
        &["display-message", "-p", "-t", &pane, FORMAT],
        deadline,
    )?;
    let fresh = parse_observation(fresh.trim_end()).ok_or("pane disappeared")?;
    if !context_unchanged(&observation, &fresh) {
        return Ok(());
    }
    let snapshot = processes(deadline)?;
    if identity::interactive_clients(&snapshot, fresh.pid) != vec![client]
        || !identity::hook_belongs_to_client(&snapshot, std::process::id(), client)
    {
        return Ok(());
    }
    observe_turn(event, true, None, deadline);
    apply(event, &fresh, None);
    Ok(())
}

fn observe_turn(event: &AgentEvent, embedded: bool, control: Option<&Path>, deadline: Instant) {
    let (session, turn) = match event {
        AgentEvent::UserPromptSubmit {
            session_id: Some(session),
            turn_id: Some(turn),
            ..
        }
        | AgentEvent::Stop {
            session_id: Some(session),
            turn_id: Some(turn),
            ..
        } => (session, turn),
        _ => return,
    };
    if tmux::get_option(tmux::SIDEBAR_QUOTA).is_some_and(|s| {
        matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "off" | "false" | "0" | "no"
        )
    }) {
        return;
    }
    let account = if embedded {
        None
    } else {
        control.and_then(|path| {
            transport::account_id(
                path,
                deadline.min(Instant::now() + Duration::from_millis(250)),
            )
            .ok()
            .flatten()
        })
    };
    let observation = crate::usage::attribution::Observation {
        session: session.clone(),
        turn: turn.clone(),
        observed_at: crate::time::now_epoch_secs(),
        runtime: if embedded { "embedded" } else { "daemon" }.into(),
        account,
    };
    if let Err(error) = crate::usage::attribution::publish(&observation) {
        eprintln!("codex usage attribution: {error}");
    }
}

fn apply(event: &AgentEvent, pane: &PaneObservation, client: Option<u32>) {
    // Title selection establishes presentation identity, not lifecycle state.
    // Mid-turn attachments must wait for a prompt/start instead of bootstrapping
    // metadata from an approval, tool event, or completion.
    if let Some(client) = client
        && let AgentEvent::PermissionRequest {
            session_id,
            turn_id,
            ..
        }
        | AgentEvent::ActivityLog {
            session_id,
            turn_id,
            ..
        }
        | AgentEvent::Stop {
            session_id,
            turn_id,
            ..
        } = event
        && (pane.session.is_empty()
            || session_id.as_deref() != Some(pane.session.as_str())
            || pane.turn.is_empty()
            || turn_id.as_deref() != Some(pane.turn.as_str())
            || tmux::get_pane_option_value(&pane.pane, tmux::PANE_CODEX_ROUTING) != "title-v1"
            || tmux::get_pane_option_value(&pane.pane, tmux::PANE_CODEX_CLIENT_PID)
                != client.to_string())
    {
        return;
    }
    if same_session_start_has_turn(event, pane) {
        return;
    }
    super::handle_event(&pane.pane, "codex", event.clone());
    if matches!(
        event,
        AgentEvent::SessionStart { .. } | AgentEvent::UserPromptSubmit { .. }
    ) && event_session(event)
        == Some(tmux::get_pane_option_value(&pane.pane, tmux::PANE_SESSION_ID).as_str())
    {
        if let Some(client) = client {
            tmux::set_pane_option(&pane.pane, tmux::PANE_CODEX_ROUTING, "title-v1");
            tmux::set_pane_option(&pane.pane, tmux::PANE_CODEX_CLIENT_PID, &client.to_string());
        } else {
            tmux::unset_pane_option(&pane.pane, tmux::PANE_CODEX_ROUTING);
            tmux::unset_pane_option(&pane.pane, tmux::PANE_CODEX_CLIENT_PID);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const ID: &str = "01a12ada-edfc-75a3-a451-d6312dd93d6d";
    fn event(name: &str) -> AgentEvent {
        crate::event::resolve_adapter("codex")
            .unwrap()
            .parse(
                name,
                &json!({"session_id":ID,"turn_id":"t1","prompt":"hello","cwd":"/tmp","tool_name":"Bash","tool_input":{"command":"true"}}),
            )
            .unwrap()
    }
    fn pane() -> PaneObservation {
        PaneObservation {
            pane: "%ROUTING_TEST".into(),
            pid: 1,
            title: ID.into(),
            session: ID.into(),
            turn: String::new(),
            completed: String::new(),
        }
    }

    #[test]
    fn selected_client_cannot_bootstrap_permission_activity_or_stop() {
        let _guard = tmux::test_mock::install();
        for name in ["permission-request", "activity-log", "stop"] {
            for (session, turn) in [("", ""), (ID, ""), (ID, "different"), ("other", "t1")] {
                let mut pane = pane();
                pane.session = session.into();
                pane.turn = turn.into();
                tmux::test_mock::set(&pane.pane, tmux::PANE_STATUS, "idle");
                tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_ROUTING, "title-v1");
                tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_CLIENT_PID, "2");
                apply(&event(name), &pane, Some(2));
                assert!(
                    !tmux::test_mock::contains(&pane.pane, tmux::PANE_AGENT),
                    "{name}: {session}/{turn}"
                );
                assert_eq!(
                    tmux::test_mock::get(&pane.pane, tmux::PANE_STATUS).as_deref(),
                    Some("idle")
                );
                assert!(!tmux::test_mock::contains(
                    &pane.pane,
                    tmux::PANE_WAIT_REASON
                ));
                assert!(!tmux::test_mock::contains(
                    &pane.pane,
                    tmux::PANE_COMPLETED_TURN_ID
                ));
            }
            let mut pane = pane();
            pane.turn = "t1".into();
            tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_ROUTING, "title-v1");
            tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_CLIENT_PID, "2");
            let mut without_turn = event(name);
            match &mut without_turn {
                AgentEvent::PermissionRequest { turn_id, .. }
                | AgentEvent::ActivityLog { turn_id, .. }
                | AgentEvent::Stop { turn_id, .. } => *turn_id = None,
                _ => unreachable!(),
            }
            apply(&without_turn, &pane, Some(2));
            assert!(
                !tmux::test_mock::contains(&pane.pane, tmux::PANE_AGENT),
                "{name} without event turn"
            );
        }
    }

    #[test]
    fn selected_client_permission_accepts_established_matching_turn() {
        let _guard = tmux::test_mock::install();
        let mut pane = pane();
        pane.turn = "t1".into();
        tmux::test_mock::set(&pane.pane, tmux::PANE_SESSION_ID, ID);
        tmux::test_mock::set(&pane.pane, tmux::PANE_TURN_ID, "t1");
        tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_ROUTING, "title-v1");
        tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_CLIENT_PID, "2");
        apply(&event("permission-request"), &pane, Some(2));
        assert_eq!(
            tmux::test_mock::get(&pane.pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some("permission")
        );
    }

    #[test]
    fn selected_client_rejects_legacy_or_previous_client_context() {
        let _guard = tmux::test_mock::install();
        let mut pane = pane();
        pane.turn = "t1".into();
        tmux::test_mock::set(&pane.pane, tmux::PANE_SESSION_ID, ID);
        tmux::test_mock::set(&pane.pane, tmux::PANE_TURN_ID, "t1");
        for (marker, pid) in [
            ("", "2"),
            ("unknown", "2"),
            ("title-v1", ""),
            ("title-v1", "3"),
            ("title-v1", "bad"),
        ] {
            tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_ROUTING, marker);
            tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_CLIENT_PID, pid);
            for name in ["permission-request", "activity-log", "stop"] {
                apply(&event(name), &pane, Some(2));
                assert!(!tmux::test_mock::contains(
                    &pane.pane,
                    tmux::PANE_WAIT_REASON
                ));
                assert!(!tmux::test_mock::contains(
                    &pane.pane,
                    tmux::PANE_COMPLETED_TURN_ID
                ));
                assert!(!tmux::test_mock::contains(&pane.pane, tmux::PANE_AGENT));
            }
        }
    }

    #[test]
    fn prefix_collision_and_absence_fail_closed() {
        let title = format!("{}... ⠋", &ID[..29]);
        assert!(selected(&title, ID, Some(&[ID.into()])));
        assert!(!selected(&title, ID, None));
        assert!(!selected(&title, ID, Some(&[])));
        assert!(!selected(
            &title,
            ID,
            Some(&[ID.into(), format!("{}eeeeeee", &ID[..29])])
        ));
        assert!(selected(ID, ID, None));
    }

    #[test]
    fn late_start_preserves_turn_and_review_state() {
        let _guard = tmux::test_mock::install();
        let mut pane = pane();
        pane.turn = "newer".into();
        tmux::test_mock::set(&pane.pane, tmux::PANE_TURN_ID, "newer");
        tmux::test_mock::set(
            &pane.pane,
            tmux::PANE_WAIT_REASON,
            tmux::WAIT_REASON_RESPONSE_REVIEWING,
        );
        apply(&event("session-start"), &pane, Some(2));
        assert_eq!(
            tmux::test_mock::get(&pane.pane, tmux::PANE_TURN_ID).as_deref(),
            Some("newer")
        );
        assert_eq!(
            tmux::test_mock::get(&pane.pane, tmux::PANE_WAIT_REASON).as_deref(),
            Some(tmux::WAIT_REASON_RESPONSE_REVIEWING)
        );
    }

    #[test]
    fn prompt_establishes_routing_provenance_but_stop_does_not_bootstrap() {
        let _guard = tmux::test_mock::install();
        let pane = pane();
        let mut stop = event("stop");
        if let AgentEvent::Stop { response, .. } = &mut stop {
            *response = None;
        }
        apply(&stop, &pane, Some(2));
        assert!(!tmux::test_mock::contains(
            &pane.pane,
            tmux::PANE_CODEX_ROUTING
        ));
        apply(&event("user-prompt-submit"), &pane, Some(2));
        assert_eq!(
            tmux::test_mock::get(&pane.pane, tmux::PANE_CODEX_ROUTING).as_deref(),
            Some("title-v1")
        );
        assert_eq!(
            tmux::test_mock::get(&pane.pane, tmux::PANE_CODEX_CLIENT_PID).as_deref(),
            Some("2")
        );
    }

    #[test]
    fn locked_dispatch_rejects_changed_lifecycle_context() {
        let pane = pane();
        assert!(context_unchanged(&pane, &pane));
        for field in 0..3 {
            let mut fresh = pane.clone();
            match field {
                0 => fresh.session = "new".into(),
                1 => fresh.turn = "new".into(),
                _ => fresh.completed = "new".into(),
            }
            assert!(!context_unchanged(&pane, &fresh));
        }
    }

    #[test]
    fn legacy_prompt_removes_previous_title_provenance() {
        let _guard = tmux::test_mock::install();
        let pane = pane();
        tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_ROUTING, "title-v1");
        tmux::test_mock::set(&pane.pane, tmux::PANE_CODEX_CLIENT_PID, "2");
        apply(&event("user-prompt-submit"), &pane, None);
        assert!(!tmux::test_mock::contains(
            &pane.pane,
            tmux::PANE_CODEX_ROUTING
        ));
        assert!(!tmux::test_mock::contains(
            &pane.pane,
            tmux::PANE_CODEX_CLIENT_PID
        ));
    }
}
