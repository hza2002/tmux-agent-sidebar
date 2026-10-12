//! Client-side identity evidence for Codex's daemon-backed TUI.
//! A shortened title is only a prefix, never an independently verified binding.

use crate::process::{ProcessInfo, ProcessSnapshot, command_basename};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TitleIdentity {
    Full(String),
    Prefix(String),
}

pub(crate) fn canonical_uuid(value: &str) -> bool {
    value.len() == 36 && uuid_prefix(value)
}

fn uuid_prefix(value: &str) -> bool {
    value.bytes().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            byte == b'-'
        } else {
            byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
        }
    })
}

fn identity_token(token: &str) -> Option<TitleIdentity> {
    if canonical_uuid(token) {
        Some(TitleIdentity::Full(token.to_owned()))
    } else if token.len() == 32
        && let Some(prefix) = token.strip_suffix("...")
        && uuid_prefix(prefix)
    {
        Some(TitleIdentity::Prefix(prefix.to_owned()))
    } else {
        None
    }
}

/// The thread-id item must be last; Codex can append its title-generation spinner.
/// Other configured title items are separated by ` | ` in the inspected TUI.
pub(crate) fn title_identity(title: &str) -> Option<TitleIdentity> {
    let mut found = None;
    for token in title.split_whitespace() {
        if let Some(identity) = identity_token(token) {
            if found.is_some() {
                return None;
            }
            found = Some(identity);
        }
    }
    let segment = title.rsplit(" | ").next()?.trim();
    let mut parts = segment.split_whitespace();
    let identity = identity_token(parts.next()?)?;
    if let Some(suffix) = parts.next() {
        // Progress uses a single Braille character, not arbitrary trailing text.
        let mut chars = suffix.chars();
        if !chars
            .next()
            .is_some_and(|c| ('\u{2800}'..='\u{28ff}').contains(&c))
            || chars.next().is_some()
            || parts.next().is_some()
        {
            return None;
        }
    }
    (found.as_ref() == Some(&identity)).then_some(identity)
}

/// Comparison against an already established full session ID, suitable for
/// rejecting stale projections. Routing a prefix also requires catalog uniqueness.
pub(crate) fn title_matches_session(title: &str, session: &str) -> bool {
    if !canonical_uuid(session) {
        return false;
    }
    match title_identity(title) {
        Some(TitleIdentity::Full(full)) => full == session,
        Some(TitleIdentity::Prefix(prefix)) => session.starts_with(&prefix),
        None => false,
    }
}

fn value_option(option: &str) -> bool {
    matches!(
        option,
        "-c" | "--config"
            | "-m"
            | "--model"
            | "-p"
            | "--profile"
            | "-C"
            | "--cd"
            | "-s"
            | "--sandbox"
            | "-a"
            | "--ask-for-approval"
            | "-i"
            | "--image"
            | "--add-dir"
            | "--enable"
            | "--disable"
            | "--remote"
            | "--remote-auth"
    )
}

/// Only classify known CLI argument shapes. `ps` argv is lossy, so ambiguous
/// shapes fail closed rather than trying to parse shell quoting heuristically.
fn interactive_command(info: &ProcessInfo) -> Option<bool> {
    let mut args = info.args.split_whitespace();
    let executable = args.next();
    let argv0_is_codex = executable.is_some_and(|arg| command_basename(arg) == "codex");
    // macOS truncates `comm` to 15 characters even with `ps -ww`; a long
    // absolute binary path is instead identifiable through complete argv[0].
    if command_basename(&info.comm) != "codex" && !argv0_is_codex {
        return None;
    }
    if !argv0_is_codex {
        return Some(false);
    }
    while let Some(arg) = args.next() {
        if arg == "--" {
            return Some(true);
        }
        let option = arg.split('=').next().unwrap_or(arg);
        if value_option(option) {
            if !arg.contains('=') && args.next().is_none() {
                return Some(false);
            }
            continue;
        }
        if matches!(
            arg,
            "--full-auto"
                | "--yolo"
                | "--dangerously-bypass-approvals-and-sandbox"
                | "--search"
                | "--no-alt-screen"
                | "--no-daemon"
                | "--strict-config"
                | "--oss"
                | "--ephemeral"
        ) {
            continue;
        }
        if arg.starts_with('-') {
            return Some(false);
        }
        return Some(match arg {
            "resume" | "fork" | "agents" => true,
            "exec"
            | "e"
            | "review"
            | "login"
            | "logout"
            | "mcp"
            | "plugin"
            | "app-server"
            | "remote-control"
            | "app"
            | "completion"
            | "update"
            | "doctor"
            | "sandbox"
            | "debug"
            | "execpolicy"
            | "apply"
            | "a"
            | "queue"
            | "archive"
            | "delete"
            | "migrate-rollouts"
            | "unarchive"
            | "cloud"
            | "cloud-tasks"
            | "responses-api-proxy"
            | "stdio-to-uds"
            | "exec-server"
            | "features"
            | "tcp-tunnel" => false,
            _ => true, // A default interactive prompt, not a substring command match.
        });
    }
    Some(true)
}

/// Prerequisite notices exempt only an explicitly requested embedded TUI.
/// Stop at prompt boundaries because `ps` cannot preserve argument quoting.
pub(crate) fn client_requires_daemon(snapshot: &ProcessSnapshot, client: u32) -> bool {
    let Some(info) = snapshot.info_by_pid.get(&client) else {
        return true;
    };
    if interactive_command(info) != Some(true) {
        return true;
    }
    let mut args = info.args.split_whitespace().skip(1);
    let mut session_command = false;
    let mut session_argument = false;
    let mut no_daemon = false;
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if value_option(arg.split('=').next().unwrap_or(arg)) {
            if !arg.contains('=') && args.next().is_none() {
                return true;
            }
            continue;
        }
        if arg == "--no-daemon" {
            no_daemon = true;
        } else if arg.starts_with('-') {
            continue;
        } else if !session_command && matches!(arg, "resume" | "fork") {
            // The pinned CLI accepts --no-daemon both before and after these
            // subcommands (see resume_and_fork_preserve_no_daemon upstream).
            session_command = true;
        } else if arg == "agents" && !session_command {
            return true; // This overview explicitly requires a shared daemon.
        } else if session_command && !session_argument {
            session_argument = true;
        } else {
            break;
        }
    }
    !no_daemon
}

/// Walk wrappers to the first Codex process on each branch. Never descend into
/// a Codex process: its daemon, exec, or tool children cannot prove pane ownership.
pub(crate) fn interactive_clients(snapshot: &ProcessSnapshot, pane_pid: u32) -> Vec<u32> {
    let mut pending = vec![pane_pid];
    let mut seen = HashSet::new();
    let mut clients = Vec::new();
    while let Some(pid) = pending.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(info) = snapshot.info_by_pid.get(&pid)
            && let Some(interactive) = interactive_command(info)
        {
            if interactive {
                clients.push(pid);
            }
            continue;
        }
        if let Some(children) = snapshot.children_of.get(&pid) {
            pending.extend(children);
        }
    }
    clients.sort_unstable();
    clients
}

/// Legacy hooks can trust a pane only when their nearest Codex ancestor is
/// that pane's qualified client. A daemon or nested tool Codex breaks proof.
pub(crate) fn hook_belongs_to_client(
    snapshot: &ProcessSnapshot,
    hook_pid: u32,
    client_pid: u32,
) -> bool {
    let parents: std::collections::HashMap<u32, u32> = snapshot
        .children_of
        .iter()
        .flat_map(|(&parent, children)| children.iter().map(move |&child| (child, parent)))
        .collect();
    let mut current = hook_pid;
    let mut seen = HashSet::new();
    while seen.len() < 256 && seen.insert(current) {
        let Some(&parent) = parents.get(&current) else {
            return false;
        };
        if let Some(info) = snapshot.info_by_pid.get(&parent)
            && let Some(interactive) = interactive_command(info)
        {
            return parent == client_pid && interactive;
        }
        current = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "01a12998-a18f-7f00-896a-c86b7bc58803";
    const SHORT: &str = "01a12998-a18f-7f00-896a-c86b7...";

    #[test]
    fn recognizes_last_title_item_and_progress() {
        assert_eq!(
            title_identity(SESSION),
            Some(TitleIdentity::Full(SESSION.into()))
        );
        assert_eq!(
            title_identity(SHORT),
            Some(TitleIdentity::Prefix(SESSION[..29].into()))
        );
        assert!(title_matches_session(&format!("repo | {SHORT} ⠋"), SESSION));
        assert!(!title_matches_session(
            SHORT,
            "01a12999-a18f-7f00-896a-c86b7bc58803"
        ));
    }

    #[test]
    fn rejects_ambiguous_or_malformed_titles() {
        for title in [
            format!("{SESSION} | {SHORT}"),
            format!("{SHORT} trailing"),
            format!("{SHORT} | repo"),
            format!("directory-{SESSION}"),
            format!("{SHORT} ⠋ ⠙"),
            SESSION.to_uppercase(),
            "01a12998-a18f-7f00-896a-c86b...".into(),
            "01a12998_a18f-7f00-896a-c86b7bc...".into(),
        ] {
            assert_eq!(title_identity(&title), None, "{title}");
        }
        assert!(!title_matches_session(SHORT, "not-a-session"));
    }

    #[test]
    fn qualifies_interactive_modes_and_option_values() {
        for args in [
            "codex",
            "codex --full-auto",
            "codex --no-daemon",
            "codex -m gpt-6.1-sol --config foo=app-server resume --last",
            "codex --model=gpt-6.1-sol fork --last",
            "codex agents",
            "codex -- please explain exec and app-server",
            "codex explain app-server",
        ] {
            let snapshot = ProcessSnapshot::from_ps_output(&format!("10 1 codex {args}\n"));
            assert_eq!(interactive_clients(&snapshot, 10), vec![10], "{args}");
        }
    }

    #[test]
    fn rejects_noninteractive_and_unknown_cli_shapes() {
        for args in [
            "codex app-server daemon run",
            "codex -c foo=bar exec true",
            "codex e true",
            "codex review --uncommitted",
            "codex --help",
            "codex --unknown value",
            "codex --model",
            "codex stdio-to-uds",
            "codex remote-control",
        ] {
            let snapshot = ProcessSnapshot::from_ps_output(&format!("10 1 codex {args}\n"));
            assert!(interactive_clients(&snapshot, 10).is_empty(), "{args}");
        }
    }

    #[test]
    fn qualifies_argv0_when_macos_comm_is_truncated() {
        let snapshot = ProcessSnapshot::from_ps_output(
            "1 0 zsh zsh\n2 1 /opt/homebrew/b /opt/homebrew/bin/codex --no-daemon\n3 1 /tmp/sidebar-ro /tmp/sidebar-route.test/codex resume --last\n4 1 /Users/ghot/.cod /Users/ghot/.codex/packages/bin/codex app-server --managed-daemon\n5 4 codex codex\n6 1 python3 python3 script.py codex resume\n",
        );
        assert_eq!(interactive_clients(&snapshot, 1), vec![2, 3]);
        assert!(!hook_belongs_to_client(&snapshot, 5, 2));
        assert_eq!(
            interactive_command(&ProcessInfo {
                comm: "python3".into(),
                args: "python3 script.py codex resume".into(),
            }),
            None,
        );
    }

    #[test]
    fn daemon_notice_exempts_only_explicit_flag_before_prompt() {
        for (args, requires) in [
            ("codex", true),
            ("codex --no-daemon", false),
            ("codex -c foo=--no-daemon", true),
            ("codex --config=foo=--no-daemon", true),
            ("codex -m --no-daemon", true),
            ("codex prompt --no-daemon", true),
            ("codex -- prompt --no-daemon", true),
            ("codex --no-daemon prompt", false),
            ("codex resume --last --no-daemon", false),
            ("codex --no-daemon resume --last", false),
            ("codex fork --last --no-daemon", false),
            ("codex resume session-name --no-daemon", false),
            ("codex resume session-name prompt --no-daemon", true),
            ("codex agents --no-daemon", true),
            ("codex app-server --no-daemon", true),
        ] {
            let snapshot = ProcessSnapshot::from_ps_output(&format!("10 1 codex {args}\n"));
            assert_eq!(client_requires_daemon(&snapshot, 10), requires, "{args}");
        }
        assert!(client_requires_daemon(
            &ProcessSnapshot::from_ps_output(""),
            10
        ));
    }

    #[test]
    fn prunes_daemons_exec_and_tool_children() {
        let snapshot = ProcessSnapshot::from_ps_output(
            "1 0 zsh zsh\n2 1 codex codex --full-auto\n3 2 codex codex app-server daemon run\n4 3 sh sh -c codex\n5 4 codex codex\n6 2 sh sh -c codex\n7 6 codex codex\n8 1 codex codex exec true\n9 8 codex codex\n",
        );
        assert_eq!(interactive_clients(&snapshot, 1), vec![2]);
        assert!(interactive_clients(&snapshot, 3).is_empty());
        assert!(interactive_clients(&snapshot, 8).is_empty());
    }

    #[test]
    fn walks_wrappers_stably_without_cycles() {
        let snapshot = ProcessSnapshot::from_ps_output(
            "1 3 zsh zsh\n3 1 sh sh -c wrapper\n4 3 codex /opt/homebrew/bin/codex resume --last\n5 1 codex codex fork --last\n",
        );
        assert_eq!(interactive_clients(&snapshot, 1), vec![4, 5]);
    }

    #[test]
    fn legacy_hook_requires_nearest_interactive_client_ancestor() {
        let snapshot = ProcessSnapshot::from_ps_output(
            "1 0 zsh zsh\n2 1 codex codex --no-daemon\n3 2 sh sh hook.sh\n4 3 tmux-agent-sidebar tmux-agent-sidebar hook codex stop\n5 2 codex codex app-server daemon run\n6 5 sh sh hook.sh\n7 6 tmux-agent-sidebar tmux-agent-sidebar hook codex stop\n8 2 codex codex exec true\n9 8 tmux-agent-sidebar tmux-agent-sidebar hook codex stop\n10 2 codex codex\n11 10 tmux-agent-sidebar tmux-agent-sidebar hook codex stop\n",
        );
        assert!(hook_belongs_to_client(&snapshot, 4, 2));
        assert!(!hook_belongs_to_client(&snapshot, 4, 99));
        assert!(!hook_belongs_to_client(&snapshot, 7, 2));
        assert!(!hook_belongs_to_client(&snapshot, 9, 2));
        assert!(!hook_belongs_to_client(&snapshot, 11, 2));
        assert!(!hook_belongs_to_client(&snapshot, 999, 2));
    }
}
