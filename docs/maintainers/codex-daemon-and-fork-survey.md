# Codex Daemon Hooks & Upstream Fork Survey

Snapshot date: 2026-10-11. Author: research session (no code changed).
Status: historical research snapshot. The daemon fix was completed in commit
`7533e59` using title routing, a bounded ID-only daemon query, and hook-owned
state, without a daemon status poller. See the [implemented design](../plans/2026-10-11-codex-daemon-routing-design.md)
and [deployment guide](../codex-daemon-routing.md).

This document is the fixed record of two investigations:

1. Why Codex panes stopped appearing in the sidebar (root cause, evidence).
2. What the upstream fork ecosystem has built, as a menu for future fork work.

Use the implemented design and deployment guide for current routing behavior.
This snapshot preserves the original investigation; section 4 is a dated menu
for future evaluation.

---

## 1. Root cause: Codex hooks fire inside the managed daemon

### Symptom

A Codex TUI session running in a tmux pane never appears in the sidebar. The
pane carries no `@pane_*` options at all, while kimi/claude panes in the same
window are fully populated.

### Mechanism

Daemon mode was first observed locally with Codex CLI 0.160.0; this does not
establish the first affected release. The verified TUI sessions are hosted by a shared
long-running `codex app-server --managed-daemon` process. Hook commands are
executed **by the daemon**, with the daemon's environment — not by the TUI
process in the user's pane. Before the fix, our hook resolved the target pane solely from
`$TMUX_PANE` (`src/cli/mod.rs`, `tmux_pane()`), which in daemon mode contains
whatever pane the daemon happened to be started from (stale, possibly dead).
All pane-option writes then fail silently (`run_tmux` errors are discarded).

Verified live on this machine (2026-10-10) with a temporary instrumentation
line in `hook.sh`:

```
pid=61037 ppid=33750 args=codex user-prompt-submit TMUX_PANE=%6   # %6 long dead
pid=61699 ppid=33750 args=codex stop              TMUX_PANE=%6
```

PID 33750 is the managed daemon. Thread logs in `~/.codex/logs_2.sqlite`
(`session_loop{thread_id=...}`) for the affected session are written by the
daemon's `process_uuid`, confirming daemon hosting.

### What is NOT the cause

- **Codex version.** 0.162.0 and 0.162.1 reproduced the same routing failure;
  TUI hooks run in the daemon, and `codex exec` hooks run in-process.
  (An early hypothesis blaming the pre-upgrade 0.162.0
  binary was disproved by downloading the release binary and testing it.)
- **Hook trust / `hooks.json` drift.** Trust hashes in `~/.codex/config.toml`
  match; `codex exec` fires all hooks (SessionStart/UserPromptSubmit/Stop)
  successfully with the same config.
- **Sidebar rendering or tmux write path.** kimi panes work; manual hook
  invocation with a correct `TMUX_PANE` writes state correctly.

### Scope

- Broken: all Codex **TUI** sessions (daemon-hosted). Unaffected:
  `codex exec` (in-process), kimi, claude (both execute hooks in-process).
- Timeline on this machine: daemon package first installed Oct 2 13:58
  (`~/.codex/packages/app-server-daemon/releases/0.160.0-*`); the first
  daemon-fired hook lock is from Oct 2 14:00. Codex entries have been
  invisible since.
- The sidebar's Missing-hooks notice cannot detect this failure mode: hooks
  are installed and "succeed" — they just write to the wrong pane.

### Zero-code workaround (verified live)

`codex --no-daemon` (public flag, `codex-rs/tui/src/cli.rs`) runs the session
in-process; hooks then inherit the correct `TMUX_PANE` and the sidebar works
completely (`@pane_agent`, status, wait_reason, prompt all verified).
Trade-offs: no agents overview, no `--remote`, thread not shared with the
desktop app, session dies with the TUI.

### Upstream status

- hiroppy/tmux-agent-sidebar#71 "Codex pane not detected when hooks run from
  app-server without TMUX_PANE" — open since 2026-05-27, no maintainer
  response, no linked PR. Related: #114 (SessionStart matcher).
- Upstream development is renovate-only lately.
- The 0.162.0→0.162.1 changelog confirms the daemon owns feature settings
  ("A running shared background server owns its feature settings").

---

## 2. Reference solution: danhper's pane-to-thread mapping

Fork: `danhper/tmux-agent-sidebar`, commit `06a4738` "Support Codex daemon
clients with pane-to-thread mapping" (2026-10-09, 1,282 additions and
47 deletions across 18 files, including lockfile changes). It was one
implementation located in this research snapshot. **Cannot be
cherry-picked** — it builds on upstream code that our fork has heavily
diverged from. It is a research comparison; the implemented design differs.

### Design: two channels, authoritative identity

Module header states the rule: *"Never infer a binding from cwd, recency, or
the daemon's inherited TMUX_PANE."*

1. **Pull channel (sidebar → daemon).** The daemon exposes a WebSocket
   JSON-RPC control socket at `~/.codex/app-server-control/
   app-server-control.sock`. A worker thread calls `thread/loaded/list` +
   `thread/read` (read-only, paginated) and gets each thread's id, cwd, name,
   preview, and status (`active`/`idle`/`systemError`, plus
   `waitingOnApproval` / `waitingOnUserInput` flags → waiting reason).
   Sidebar renders daemon truth even when hooks are absent.
   New files: `src/codex_daemon.rs` (+290), `src/codex_daemon/tests.rs`.

2. **Push channel (hook routing).** For `codex` hook events, `cmd_hook` no
   longer exits when `$TMUX_PANE` is empty/stale. It scans all panes
   (`pane_id`, `@pane_session_id`, `pane_title`, `pane_pid`) and routes the
   event to every pane whose process tree contains codex, whose registered
   session ID matches, and whose terminal title carries the event's thread id. One thread attached to
   several clients writes all panes; only the first pane gets the Stop
   protocol response. `--no-daemon` in-process hooks keep the direct path
   (excluded unless the hook process is a descendant of the pane's tree).

3. **Identity carrier: the terminal title.** Codex TUI's
   `tui.terminal_title` is configured to include `thread-id`, surfaced by
   tmux as `pane_title`. A `setup` subcommand rewrites `~/.codex/config.toml`
   atomically (toml_edit, create_new temp file, concurrent-modification
   check) to append `thread-id` while preserving other title items. Codex
   0.161 truncates UUIDs to 29 chars + "..."; matching accepts full UUIDs or
   unique truncated prefixes.

### Fit with our constraints

- Targets current Codex; no backward-compat concerns — matches our decision
  to fix against the latest version only.
- Adds a dependency (tungstenite) — repo default is dependency-free; needs
  justification in the decision record.
- `thread/loaded/list` / `thread/read` are experimental app-server APIs;
  version drift risk lives behind Codex upgrades. Sync checks should include
  them (`.agents/skills/sync-upstream-features/`).
- Per AGENTS.md this changes core data flow → a decision record in
  `docs/decisions/` is required before implementation.

### Smaller alternative (rejected by danhper, kept for the record)

Hook-side cwd fallback: when `$TMUX_PANE` is stale, match panes by
`pane_current_path == payload.cwd` + codex in process tree. One-function
change, but ambiguous when several panes share a cwd and races on pane reuse.
This fallback was rejected in the implemented design because it cannot
establish an unambiguous pane identity.

---

## 3. Verification commands (for re-checking after Codex upgrades)

```bash
# Which process executes codex hooks, with what pane env:
ps eww <daemon-pid> | tr ' ' '\n' | grep TMUX
# Daemon socket present?
ls ~/.codex/app-server-control/app-server-control.sock
# TUI thread hosted by daemon? session_loop logs carry the writer's pid:
sqlite3 ~/.codex/logs_2.sqlite \
  "select distinct process_uuid from logs where feedback_log_body like '%<thread-id>%';"
```

---

## 4. Fork ecosystem survey — menu for future work

89 forks; ~25 have commits ahead of upstream. Grouped by theme, with an
assessment for this fork. Details from each fork's ahead-of-upstream commits
as of 2026-10-11.

### Directly relevant to our workflow

| Fork | Work | Assessment |
|---|---|---|
| danhper | Codex daemon mapping (sec. 2); persisted Claude/Codex session **titles** in rows; hide idle `claude rc` / remote-control pre-spawned sessions | Daemon fix completed in commit `7533e59`; session titles and rc-hiding are worth evaluating next |
| Lesords | Hook hardening: ignore foreign-session events while pane owner lives; keep registration on session-end while process lives; PreToolUse activity; dialog→waiting | Adjacent to daemon work; review when touching hook.rs |
| tbando | Show **model name + reasoning effort** for all agents; org/repo repo labels; Antigravity adapter | Model/effort row is high-value for multi-agent monitoring |
| anhdle14 | Drop `#{q:...}` from hot list-panes (tmux 3.2a server leak, upstream #85); auto build-from-source in launcher | The tmux leak fix may apply to us — check our query path |

### UI/UX ideas

| Fork | Work |
|---|---|
| brandtg (48 commits, most engineering-heavy) | Narrow-viewport popup sidebar; compact view toggle; breathing "done-unseen" indicator; collapsible Activity/Git bottom panel; non-agent tmux windows in list; finished-task flash for plain panes; render perf via worker threads |
| NanoBoom | Group by tmux session instead of repo; open existing worktree with agent (`o`); self-close when last neighbour pane exits |
| alexciarlillo | Show all sessions option; sidebar width min/max clamps + live resize |
| b1rd0cean | Compact two-row agent list ordered by tmux window |
| satotoru | Show tmux window name instead of hardcoded agent label |
| laat | Sort repo groups by recent activity (`@sidebar_sort activity`) |

### Platform/distribution

| Fork | Work |
|---|---|
| comicchang (36) | Platform-specific binary names for multi-device sync; cross-window width sync; Pi adapter; glibc 2.17 Linux builds via zigbuild |
| afxnoize | Fix agent detection for Nix-wrapped binaries |
| sabrehagen | Prefer live process when session files share an id (upstream PR #167, open) |
| kotsmile | Project-level `ty.toml` config; custom VCS for the Git tab |

### Different species (reference only)

- **irhtml/herdmux**: renamed fork with an agent-orchestration CLI
  (list/spawn/prompt/wait/read agents), resurrect-based session resume,
  worktree env-file copying. Interesting if we ever want the sidebar to
  *drive* agents, not just observe them.
- **kellerkind84**: Devin/Copilot/Hermes adapters; summary/digest CLI.
- **maxronner**: smelt agent integration via a Lua plugin bridge.

### New-agent adapters seen across forks (demand signal)

Cursor (danhper, SkippyZA #146), Antigravity (tbando, lucasmacori #163),
Pi/OMP (several), Grok (gbarany #148, 73 review rounds, unmerged), Devin,
Copilot. All stalled upstream — more evidence that adapter PRs are not
getting merged there.

---

## 5. Suggested order of work

1. Codex daemon fix — completed in commit `7533e59`; see the implemented design.
2. Re-check Lesords' hook hardening while hook.rs is open.
3. Evaluate danhper's session titles + `claude rc` hiding.
4. tbando's model/effort display; anhdle14's tmux leak check.
5. Everything else in section 4 on demand.
