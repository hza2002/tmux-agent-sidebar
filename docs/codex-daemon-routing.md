# Codex daemon hook routing

Codex CLI can execute hooks in a shared daemon. Its inherited `TMUX_PANE`
does not identify the TUI that displays the event's thread. The sidebar routes
these hooks through each client's thread title and an explicitly selected tmux
server. A bounded, ID-only daemon request disambiguates shortened titles;
it does not read transcripts or replace hook-owned workflow state.

## Configure a daemon deployment

Run these read-only commands from the intended tmux server and the same Codex
home used by its TUIs:

```sh
tmux display-message -p '#{socket_path}'
codex app-server daemon version
```

The first command prints the absolute tmux server socket. In the verified
Codex 0.162.1 build, the second prints JSON containing `status` and `socketPath`.
Use that `socketPath` as the control endpoint after checking that the reported
daemon is running. If you use a separate Codex home, scope `CODEX_HOME` to the
second command. Do not guess the socket from another daemon's inherited
environment. Codex 0.162.1 has no `app-server daemon status` subcommand.

Generate the hook snippet with those two explicit paths. For example, replace
the illustrative paths below with the values you just inspected:

```sh
/absolute/path/to/tmux-agent-sidebar setup codex \
  --tmux-socket /absolute/path/to/tmux-server.sock \
  --codex-control-socket /absolute/path/to/app-server-control.sock
```

Both paths must be absolute; tmux socket paths must not contain commas. The
command prints hook JSON to stdout and prerequisite guidance to stderr. It
only generates output; it does not edit user configuration or trust records.
Paths containing spaces or shell metacharacters are quoted in generated hooks.

Merge these settings into the Codex home's `config.toml`, preserving other
settings and existing table entries:

```toml
[features]
hooks = true

[tui]
terminal_title = ['thread-id']
```

Config managers such as cc-switch can replace the active settings when a
provider is switched, so these entries must live in the manager's shared
(common) configuration to persist. Verified cc-switch 5.11.0 supports
`cc-switch --app codex config common set --file /path/to/complete-common.toml`;
that command replaces the existing common snippet, so preserve every existing
common setting and keep provider-specific configuration separate rather than
copying a whole live config into common.

After Codex's supported trust flow, the resulting hook trust state
(`[hooks.state]`) must also persist in that common configuration so trusted
hooks survive provider switches.

Merge the generated hooks into that home's `hooks.json`, preserving unrelated
hooks. Re-paste the new commands even if earlier sidebar hooks already exist.
Trust the changed hook commands through Codex's supported trust flow. Restart
Codex TUIs to load the title configuration, and restart the sidebar to refresh
its startup endpoint and hook-drift checks. Rust changes require the current
release build. These changes do not require a Claude plugin update.

For several tmux servers sharing one daemon, generate one snippet per server
using its own tmux socket and the shared control socket. Keep a separate command
for each server in every relevant hook registration. Do not replace one
server's entries when merging the next. Each command pins its own server;
there is no global socket discovery or inherited-pane fallback.

## Notices and diagnosis

The existing notices popup flags missing endpoint configuration or a command
scoped to a different server. Startup checks inspect literal commands without
executing them. Restart the sidebar after editing the hook configuration.

Existing TUIs retain the title settings loaded at startup. Restarting the
sidebar alone does not update their titles. Exit the affected TUI and use
`codex resume` to continue its conversation after configuring thread titles.

A qualified live Codex client with no usable thread title produces a title
notice after a two-second startup grace. The notice describes the observed
missing title; it does not claim which configuration file caused it. It clears
when the title becomes usable. Use the popup's prompt action for setup guidance.

Transient control-socket failures and ambiguous titles produce hook stderr
diagnostics. The sidebar skips uncertain targets and preserves Codex's hook
protocol response. A control-socket outage cannot authorize writes to an
inherited pane. Published socket paths can be symlinks; routing resolves the
selected endpoint before connecting.

## Coverage and fallback

An idle thread before its first lifecycle hook, switching threads without
submitting a prompt, or attaching another TUI mid-turn can leave a pane without
trustworthy lifecycle metadata. A subsequent accepted SessionStart or
UserPromptSubmit can establish it. Daemon-routed Stop, permission, and activity
events require an established matching session and turn, a `title-v1` routing
marker, and the selected client PID; they cannot invent a missing lifecycle
start. The sidebar does not poll daemon status to fabricate running or ready
states.

Thread titles are shortened and updated asynchronously. The router checks
observed identity and revalidates after locking, but cannot guarantee an atomic
client-selection-to-write transaction. A switch entirely between observations
is not detectable. A supported full-ID client selection signal is the durable
upstream solution.

For an intentional in-process deployment, run Codex with `--no-daemon` and use
legacy `setup codex` output. That path is intended for the supported interactive
client ancestry; it does not provide a heuristic fallback for daemon, exec-only,
or unknown process trees. Do not combine a legacy command with scoped daemon
commands for the same lifecycle registration.
