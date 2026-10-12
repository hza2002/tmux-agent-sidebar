//! `setup` subcommand — prints required hooks and ready-to-paste config
//! snippets for Claude Code, Codex, and Kimi Code on stdout. Pure generator:
//! reads only the adapter `HOOK_REGISTRATIONS` tables, never the user's
//! config files. Claude/Codex snippets are nested JSON; Kimi's snippet is a
//! TOML block because `~/.kimi-code/config.toml` uses flat `[[hooks]]`
//! entries.

use std::path::PathBuf;

use crate::adapter::HookRegistration;
use crate::adapter::claude::ClaudeAdapter;
use crate::adapter::codex::CodexAdapter;
use crate::adapter::kimi::KimiAdapter;

#[allow(dead_code)]
const _CLAUDE_TABLE_REACHABLE: &[HookRegistration] = ClaudeAdapter::HOOK_REGISTRATIONS;
#[allow(dead_code)]
const _CODEX_TABLE_REACHABLE: &[HookRegistration] = CodexAdapter::HOOK_REGISTRATIONS;
#[allow(dead_code)]
const _KIMI_TABLE_REACHABLE: &[HookRegistration] = KimiAdapter::HOOK_REGISTRATIONS;

/// POSIX-quote a string for safe use as a single shell argument.
///
/// Fast path: when the string consists only of characters that bash does
/// not interpret specially, it is returned as-is. This matters for the
/// common case (`/Users/alice/.../hook.sh`) because aggressive quoting
/// would suppress tilde expansion on the fallback path
/// `~/.tmux/plugins/tmux-agent-sidebar/hook.sh` and break the emitted
/// hook commands. This mirrors Python's `shlex.quote` behaviour.
///
/// Slow path: wrap the value in single quotes and escape any internal
/// single quotes as `'\''`. Safe for paths containing spaces, `$`, `;`,
/// backticks, and other shell metacharacters.
pub(crate) fn shell_quote(s: &str) -> String {
    fn is_safe(c: char) -> bool {
        c.is_ascii_alphanumeric()
            || matches!(c, '/' | '-' | '_' | '.' | '~' | '+' | '=' | ',' | '@' | ':')
    }
    if !s.is_empty() && s.chars().all(is_safe) {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Build the `bash <hook_script> <agent> <event>` command string with
/// proper POSIX quoting so arbitrary installation paths are safe.
fn format_hook_command(hook_script: &str, agent: &str, event: &str) -> String {
    format!("bash {} {} {}", shell_quote(hook_script), agent, event)
}

/// Explicit endpoints for Codex's daemon-owned hooks.
#[derive(Debug, Clone)]
struct CodexEndpoints {
    tmux: String,
    control: String,
}

fn validate_endpoints(tmux: &str, control: &str) -> Result<CodexEndpoints, &'static str> {
    if !std::path::Path::new(tmux).is_absolute()
        || !std::path::Path::new(control).is_absolute()
        || tmux.contains(',')
        || tmux.contains('\0')
        || control.contains('\0')
    {
        return Err("endpoints must be absolute paths; the tmux socket cannot contain commas");
    }
    Ok(CodexEndpoints {
        tmux: tmux.into(),
        control: control.into(),
    })
}

fn scoped_codex_snippet(hook_script: &str, endpoints: &CodexEndpoints) -> serde_json::Value {
    let mut snippet = build_agent_snippet("codex", hook_script).unwrap();
    for entries in snippet["hooks"].as_object_mut().unwrap().values_mut() {
        for entry in entries.as_array_mut().unwrap() {
            for action in entry["hooks"].as_array_mut().unwrap() {
                let command = action["command"].as_str().unwrap();
                action["command"] = serde_json::Value::String(format!(
                    "env {} {} {command}",
                    shell_quote(&format!("TMUX={},0,0", endpoints.tmux)),
                    shell_quote(&format!(
                        "SIDEBAR_CODEX_CONTROL_SOCKET={}",
                        endpoints.control
                    )),
                ));
            }
        }
    }
    snippet
}

/// Parse shell literals without evaluating substitutions or expansions. This
/// deliberately supports generated commands, not arbitrary shell programs.
fn command_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => word.push(c),
            (Some('"'), '$' | '`' | '\\') => return None,
            (Some('"'), _) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                word.push(chars.next()?);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, ';' | '|' | '&' | '<' | '>' | '`' | '$') => return None,
            (None, c)
                if c.is_ascii_alphanumeric()
                    || matches!(c, '/' | '-' | '_' | '.' | '~' | '+' | '=' | ',' | '@' | ':') =>
            {
                word.push(c);
                started = true;
            }
            _ => return None,
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

fn canonical_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.into())
}

fn scoped_command(command: &str) -> Option<(CodexEndpoints, String)> {
    let words = command_words(command)?;
    if words.len() != 7 || words[0] != "env" || words[3] != "bash" || words[5] != "codex" {
        return None;
    }
    let tmux = words[1].strip_prefix("TMUX=")?.strip_suffix(",0,0")?;
    let control = words[2].strip_prefix("SIDEBAR_CODEX_CONTROL_SOCKET=")?;
    let endpoints = validate_endpoints(tmux, control).ok()?;
    Some((
        endpoints,
        format_hook_command(&words[4], "codex", &words[6]),
    ))
}

/// Startup prerequisite check. No socket connection or shell evaluation.
pub(crate) fn codex_endpoint_notice(
    config: &serde_json::Value,
    tmux_socket: &str,
) -> Option<String> {
    let specs = collect_hook_specs(config);
    let complete = CodexAdapter::HOOK_REGISTRATIONS.iter().all(|reg| {
        specs.iter().any(|spec| {
            spec.trigger == reg.trigger
                && spec.matcher == reg.matcher.unwrap_or("")
                && scoped_command(&spec.command).is_some_and(|(endpoints, command)| {
                    canonical_path(&endpoints.tmux) == canonical_path(tmux_socket)
                        && command_words(&command).is_some_and(|words| {
                            words
                                .last()
                                .is_some_and(|event| event == reg.kind.external_name())
                        })
                })
        })
    });
    (!complete).then(|| "Configure daemon endpoints with setup codex --tmux-socket ABS --codex-control-socket ABS".into())
}

/// Build the ready-to-paste `{ "hooks": { ... } }` JSON block for a single
/// agent. Returns `None` for unknown agent names.
///
/// Reads **only** from the adapter's `HOOK_REGISTRATIONS` table and
/// `AgentEventKind::external_name()` — no hook identity is duplicated here.
/// When `HookRegistration.matcher` is `None`, the snippet uses the empty
/// string `""` (matching Claude/Codex's "any tool" convention).
pub(crate) fn build_agent_snippet(agent: &str, hook_script: &str) -> Option<serde_json::Value> {
    let table: &[HookRegistration] = match agent {
        "claude" => ClaudeAdapter::HOOK_REGISTRATIONS,
        "codex" => CodexAdapter::HOOK_REGISTRATIONS,
        _ => return None,
    };

    let mut hooks = serde_json::Map::new();
    for reg in table {
        let matcher = reg.matcher.unwrap_or("");
        let command = format_hook_command(hook_script, agent, reg.kind.external_name());
        let entry = serde_json::json!({
            "matcher": matcher,
            "hooks": [
                { "type": "command", "command": command }
            ],
        });
        let arr = hooks
            .entry(reg.trigger.to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()))
            .as_array_mut()
            .expect("trigger entry must be an array");
        arr.push(entry);
    }

    Some(serde_json::json!({ "hooks": serde_json::Value::Object(hooks) }))
}

/// Snippet value for `agent`: the nested JSON block for Claude/Codex, or a
/// paste-ready TOML string for Kimi — Kimi's `config.toml` is a flat
/// `[[hooks]]` array and cannot reuse the JSON diff machinery, so no TOML
/// drift detection exists for it (v1).
fn build_agent_snippet_value(agent: &str, hook_script: &str) -> Option<serde_json::Value> {
    match agent {
        "kimi" => Some(serde_json::Value::String(build_kimi_toml_snippet(
            hook_script,
        ))),
        _ => build_agent_snippet(agent, hook_script),
    }
}

/// Escape a value for use inside a TOML basic string (`"..."`).
fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render [`KimiAdapter::HOOK_REGISTRATIONS`] as a paste-ready TOML block
/// for `~/.kimi-code/config.toml`: one `[[hooks]]` entry per registration,
/// with the `matcher` line omitted when the registration catches all events.
pub(crate) fn build_kimi_toml_snippet(hook_script: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for reg in KimiAdapter::HOOK_REGISTRATIONS {
        out.push_str("[[hooks]]\n");
        let _ = writeln!(out, "event = \"{}\"", toml_escape(reg.trigger));
        if let Some(matcher) = reg.matcher {
            let _ = writeln!(out, "matcher = \"{}\"", toml_escape(matcher));
        }
        let command = format_hook_command(hook_script, "kimi", reg.kind.external_name());
        let _ = writeln!(out, "command = \"{}\"", toml_escape(&command));
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HookSpec {
    trigger: String,
    matcher: String,
    command: String,
}

#[allow(dead_code)]
fn normalize_matcher(value: Option<&serde_json::Value>) -> String {
    value.and_then(|v| v.as_str()).unwrap_or("").to_string()
}

#[allow(dead_code)]
fn collect_hook_specs(config: &serde_json::Value) -> Vec<HookSpec> {
    let Some(hooks) = config.get("hooks").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };

    let mut specs = Vec::new();
    for (trigger, entries) in hooks {
        let Some(entries) = entries.as_array() else {
            continue;
        };
        for entry in entries {
            let matcher = normalize_matcher(entry.get("matcher"));
            let Some(actions) = entry.get("hooks").and_then(serde_json::Value::as_array) else {
                continue;
            };
            for action in actions {
                if action.get("type").and_then(serde_json::Value::as_str) != Some("command") {
                    continue;
                }
                let command = action
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                specs.push(HookSpec {
                    trigger: trigger.clone(),
                    matcher: matcher.clone(),
                    command,
                });
            }
        }
    }

    specs
}

/// Return the trigger names that are required by `agent` but missing from
/// `current_config`.
///
/// Comparison uses `trigger`, `matcher`, and the canonicalized hook command.
/// The command path is resolved through `std::fs::canonicalize` so a
/// symlinked plugin directory (`~/.tmux/plugins/tmux-agent-sidebar/hook.sh`
/// → `~/Programming/tmux-agent-sidebar/hook.sh`) still compares equal, while
/// configs pointing at a stale or renamed checkout canonicalize to a
/// different real path (or fail to canonicalize at all) and are flagged as
/// missing.
#[allow(dead_code)]
pub(crate) fn missing_hooks(
    agent: &str,
    current_config: &serde_json::Value,
    hook_script: &str,
) -> Vec<String> {
    let tmux = std::env::var("TMUX").ok();
    missing_hooks_for_socket(
        agent,
        current_config,
        hook_script,
        tmux.as_deref().and_then(|value| value.split(',').next()),
    )
}

fn missing_hooks_for_socket(
    agent: &str,
    current_config: &serde_json::Value,
    hook_script: &str,
    tmux_socket: Option<&str>,
) -> Vec<String> {
    let Some(expected) = build_agent_snippet(agent, hook_script) else {
        return Vec::new();
    };

    let expected = collect_hook_specs(&expected);
    let actual: std::collections::HashSet<HookSpec> = collect_hook_specs(current_config)
        .into_iter()
        .filter_map(|mut spec| {
            if agent == "codex" && spec.command.starts_with("env ") {
                let (endpoints, command) = scoped_command(&spec.command)?;
                let socket = tmux_socket?;
                if canonical_path(&endpoints.tmux) != canonical_path(socket) {
                    return None;
                }
                spec.command = command;
            }
            Some(normalize_hook_spec(spec))
        })
        .collect();

    let mut missing = Vec::new();
    let mut seen_triggers = std::collections::BTreeSet::new();
    for spec in expected {
        let key = normalize_hook_spec(spec.clone());
        if actual.contains(&key) || !seen_triggers.insert(spec.trigger.clone()) {
            continue;
        }
        missing.push(spec.trigger);
    }
    missing
}

/// Normalize a hook spec so path drift that still points at the same file
/// (e.g. via a plugin-dir symlink) compares equal, while a broken or stale
/// path stays distinct.
fn normalize_hook_spec(mut spec: HookSpec) -> HookSpec {
    spec.command = normalize_hook_command(&spec.command);
    spec
}

/// Canonicalize the script path inside a `bash <path> <args...>` command.
/// Paths that fail to canonicalize (missing file, unresolved `~`) are
/// returned with tilde expansion only, so a stale config does not collapse
/// onto the expected command by accident.
///
/// `format_hook_command` POSIX-quotes paths that contain spaces or other
/// shell metacharacters (e.g. `bash '/path with spaces/hook.sh' …`), so
/// the parser must understand single-quoted scripts — splitting on raw
/// spaces would treat `'/path` as the script and break canonicalization
/// for exactly the installs that need quoting.
fn normalize_hook_command(cmd: &str) -> String {
    // Legacy setup supported quoted HOME references. Expand only those
    // fixed prefixes; never evaluate arbitrary shell expressions.
    let expanded_home = std::env::var("HOME").ok().map(|home| {
        cmd.replace("$HOME/", &format!("{home}/"))
            .replace("${HOME}/", &format!("{home}/"))
    });
    let Some(mut words) = command_words(expanded_home.as_deref().unwrap_or(cmd)) else {
        return cmd.to_string();
    };
    if words.len() < 2 || words[0] != "bash" {
        return cmd.to_string();
    }
    words[1] = canonical_path(&expand_home_reference(&words[1]));
    words
        .iter()
        .map(|word| shell_quote(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Expand the supported home-directory forms (`~/`, `$HOME/`, `${HOME}/`)
/// without pulling in a general-purpose shell expansion dependency.
fn expand_home_reference(path: &str) -> String {
    let rest = path
        .strip_prefix("~/")
        .or_else(|| path.strip_prefix("$HOME/"))
        .or_else(|| path.strip_prefix("${HOME}/"));
    let Some(rest) = rest else {
        return path.to_string();
    };
    let Some(home) = std::env::var_os("HOME") else {
        return path.to_string();
    };
    let mut p = std::path::PathBuf::from(home);
    p.push(rest);
    p.to_string_lossy().into_owned()
}

#[allow(dead_code)]
pub(crate) fn has_missing_hooks(
    agent: &str,
    current_config: &serde_json::Value,
    hook_script: &str,
) -> bool {
    !missing_hooks(agent, current_config, hook_script).is_empty()
}

/// Build the full setup output: version, resolved hook script path,
/// and a per-agent object containing `config_path`, the normalized
/// `hooks[]` array, and the ready-to-paste `snippet`.
///
/// Pure function. `hook_script` is passed in so tests can pin it.
pub(crate) fn build_setup_output(hook_script: &str) -> serde_json::Value {
    let claude = build_agent_entry(
        "claude",
        "~/.claude/settings.json",
        ClaudeAdapter::HOOK_REGISTRATIONS,
        hook_script,
    );
    let codex = build_agent_entry(
        "codex",
        "~/.codex/hooks.json",
        CodexAdapter::HOOK_REGISTRATIONS,
        hook_script,
    );
    let kimi = build_agent_entry(
        "kimi",
        "~/.kimi-code/config.toml",
        KimiAdapter::HOOK_REGISTRATIONS,
        hook_script,
    );

    serde_json::json!({
        "version": crate::VERSION,
        "hook_script": hook_script,
        "agents": {
            "claude": claude,
            "codex": codex,
            "kimi": kimi,
        },
    })
}

fn build_agent_entry(
    agent: &str,
    config_path: &str,
    table: &[HookRegistration],
    hook_script: &str,
) -> serde_json::Value {
    let hooks: Vec<serde_json::Value> = table
        .iter()
        .map(|reg| {
            let command = format_hook_command(hook_script, agent, reg.kind.external_name());
            serde_json::json!({
                "trigger": reg.trigger,
                "matcher": match reg.matcher {
                    Some(m) => serde_json::Value::String(m.to_string()),
                    None => serde_json::Value::Null,
                },
                "event": reg.kind.external_name(),
                "command": command,
            })
        })
        .collect();

    let snippet = build_agent_snippet_value(agent, hook_script)
        .expect("agent name hardcoded above, must match build_agent_snippet_value");

    serde_json::json!({
        "config_path": config_path,
        "hooks": hooks,
        "snippet": snippet,
    })
}

/// Result of attempting to locate `hook.sh` relative to the running binary.
/// The `detected` flag is `false` when the resolver could not find an
/// actual file on disk and had to return the README fallback — callers
/// should warn the user in that case because the emitted commands will be
/// wrong for non-default installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedHookScript {
    pub path: String,
    pub detected: bool,
}

const FALLBACK_HOOK_SCRIPT: &str = "~/.tmux/plugins/tmux-agent-sidebar/hook.sh";

/// Resolve the absolute path of `hook.sh` to embed in the generated
/// commands. Strategy:
///
/// 1. `std::env::current_exe()` → get the running binary path.
/// 2. Walk up at most 3 directories from its parent, checking for a
///    sibling `hook.sh` at each level. Matches the two layouts the
///    project already supports:
///      - `<plugin>/bin/tmux-agent-sidebar` → `<plugin>/hook.sh`
///      - `<plugin>/target/release/tmux-agent-sidebar` → `<plugin>/hook.sh`
/// 3. Fallback: the literal string `~/.tmux/plugins/tmux-agent-sidebar/hook.sh`
///    (tilde intentionally not expanded, matches README).
///
/// When step 1 or 2 succeeds, `detected = true`. When step 3 kicks in,
/// `detected = false` and `cmd_setup` surfaces a stderr warning. Never
/// panics.
pub(crate) fn resolve_hook_script() -> ResolvedHookScript {
    walk_up_from_exe(3, |dir| {
        let candidate = dir.join("hook.sh");
        candidate
            .is_file()
            .then(|| candidate.to_string_lossy().into_owned())
    })
    .map(|path| ResolvedHookScript {
        path,
        detected: true,
    })
    .unwrap_or_else(|| ResolvedHookScript {
        path: FALLBACK_HOOK_SCRIPT.to_string(),
        detected: false,
    })
}

/// Walk up from the running binary (`current_exe()`) and invoke `probe`
/// at each ancestor directory, stopping at the first non-`None` result
/// or after `max_depth` steps. Returns `None` when the executable path
/// cannot be resolved (e.g. exotic platforms) — callers must be prepared
/// to fall back gracefully. Used by `resolve_hook_script` and by
/// `notices::plugin_root_from_exe` so the upward-walk loop lives in one
/// place.
pub(crate) fn walk_up_from_exe<F, T>(max_depth: usize, mut probe: F) -> Option<T>
where
    F: FnMut(&std::path::Path) -> Option<T>,
{
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    for _ in 0..=max_depth {
        if let Some(value) = probe(&dir) {
            return Some(value);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// Return the default config path for `agent` under the current user's home.
#[allow(dead_code)]
pub(crate) fn config_path_for_agent(agent: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let home = PathBuf::from(home);
    match agent {
        "claude" => Some(home.join(".claude/settings.json")),
        "codex" => Some(home.join(".codex/hooks.json")),
        _ => None,
    }
}

/// Load the current config for `agent`.
///
/// Missing files, unreadable files, or invalid JSON all map to `Value::Null`
/// so callers can still use `missing_hooks()` and surface the full expected
/// hook set.
#[allow(dead_code)]
pub(crate) fn load_current_config(agent: &str) -> serde_json::Value {
    let Some(path) = config_path_for_agent(agent) else {
        return serde_json::Value::Null;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return serde_json::Value::Null;
    };
    serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
}

/// Pure dispatch core. Returns the exit code and the JSON to print
/// (or `None` if nothing should be printed, e.g. on error). Splitting
/// this out keeps `cmd_setup` a thin I/O wrapper.
fn run_setup(args: &[String], hook_script: &str) -> (i32, Option<serde_json::Value>) {
    if args.len() > 1 && args.first().is_some_and(|agent| agent == "codex") {
        let mut tmux = None;
        let mut control = None;
        let (pairs, remainder) = args[1..].as_chunks::<2>();
        for pair in pairs {
            match pair[0].as_str() {
                "--tmux-socket" if tmux.is_none() => tmux = Some(pair[1].as_str()),
                "--codex-control-socket" if control.is_none() => control = Some(pair[1].as_str()),
                _ => return (2, None),
            }
        }
        if !remainder.is_empty() {
            return (2, None);
        }
        let Some((tmux, control)) = tmux.zip(control) else {
            return (2, None);
        };
        return match validate_endpoints(tmux, control) {
            Ok(endpoints) => (0, Some(scoped_codex_snippet(hook_script, &endpoints))),
            Err(message) => {
                eprintln!("error: {message}");
                (2, None)
            }
        };
    }
    match args.len() {
        0 => (0, Some(build_setup_output(hook_script))),
        1 => match build_agent_snippet_value(&args[0], hook_script) {
            Some(snippet) => (0, Some(snippet)),
            None => {
                eprintln!(
                    "error: unknown agent '{}' (expected 'claude', 'codex', or 'kimi')",
                    args[0]
                );
                (2, None)
            }
        },
        _ => {
            eprintln!("usage: tmux-agent-sidebar setup [claude|codex|kimi]");
            (2, None)
        }
    }
}

pub(crate) fn cmd_setup(args: &[String]) -> i32 {
    let resolved = resolve_hook_script();
    if !resolved.detected {
        eprintln!(
            "warning: could not locate hook.sh relative to the running \
             binary; using fallback path {:?}. If your installation lives \
             elsewhere, hand-edit the 'command' values before pasting.",
            resolved.path
        );
    }
    let (code, json) = run_setup(args, &resolved.path);
    if code == 0 && args.first().is_some_and(|agent| agent == "codex") {
        eprintln!(
            "Codex daemon routing requires explicit --tmux-socket ABS and --codex-control-socket ABS. Legacy setup output is for --no-daemon. Merge into ~/.codex/config.toml:\n[tui]\nterminal_title = ['thread-id']\nEnable hooks, re-paste the generated hooks into ~/.codex/hooks.json, trust the changed commands, and restart Codex TUIs."
        );
    }
    if let Some(v) = json {
        // Kimi's snippet is a paste-ready TOML block; print it raw instead
        // of JSON-escaping the newlines.
        if let Some(text) = v.as_str() {
            println!("{}", text);
            return code;
        }
        match serde_json::to_string_pretty(&v) {
            Ok(s) => println!("{}", s),
            Err(e) => {
                eprintln!("error: failed to serialize setup output: {}", e);
                return 1;
            }
        }
    }
    code
}

#[cfg(test)]
mod tests;
