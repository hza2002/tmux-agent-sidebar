# Evaluation Briefing: Codex Daemon-Mode Hook Routing Fix

Date: 2026-10-11
Status: historical neutral briefing, superseded by the [implemented design](2026-10-11-codex-daemon-routing-design.md)
and [deployment guide](../codex-daemon-routing.md). Implementation completed in
commit `7533e59`. The candidate descriptions below preserve the evaluated
alternatives; corrections are incorporated where independently verified.

Audience: a senior engineer/agent evaluating HOW to fix Codex pane tracking in
this fork. This briefing intentionally contains **no recommendation**. It
records verified facts, repro commands, the option space, and two detailed
candidate designs with their respective strengths and weaknesses. Evaluate
independently; contradicting prior analyses is welcome and expected.

Related reading (same repo):
- `docs/maintainers/codex-daemon-and-fork-survey.md` — full research record
- `docs/maintainers/architecture-map.md`, `docs/state-management.md`
- `AGENTS.md` — fork constraints (binding)

---

## 1. Verified facts (reproduced live on the owner's machine, 2026-10-10/11)

F1. Interactive TUI sessions on the live 0.162.1 build are hosted by a
    shared `codex app-server --managed-daemon` process; the exact first
    affected release is unproven (0.160.0 is a local observation only).
    Hook commands from
    `~/.codex/hooks.json` are **executed by the daemon with the daemon's
    environment**, not by the TUI process in the user's tmux pane.
    Evidence: instrumented `hook.sh` logged
    `ppid=<daemon-pid> args="codex user-prompt-submit" TMUX_PANE=%6`
    while the user's codex TUI lived in a different pane.
F2. The sidebar resolves the hook's target pane solely from `$TMUX_PANE`
    (`src/cli/mod.rs` `tmux_pane()`). In daemon mode this value is whatever
    pane the daemon was started from — stale or dead. Writes to it fail
    silently (`run_tmux` discards errors).
F3. Hook payloads in daemon mode carry `cwd`, `session_id`, `turn_id`,
    `transcript_path`, `model`, `permission_mode` (per-event field lists are
    in `codex-rs/hooks/schema/generated/*.schema.json`). No tty/pane field.
    Per-event `session_id` presence in daemon mode has NOT been re-verified
    per event (open item V1 below).
F4. `codex --no-daemon` runs sessions in-process; hooks then fire with the
    correct pane env and the sidebar works fully (verified live: all
    `@pane_*` options written).
F5. The daemon speaks WebSocket-framed JSON-RPC over
    `~/.codex/app-server-control/app-server-control.sock` (per
    openai/codex#23572). As of this writing the socket is **absent** on the
    owner's machine when no daemon-hosted session is active — the daemon
    starts on demand. `codex app-server proxy --sock <path>` (stdio↔socket
    bridge) exists but had reported bridging bugs (openai/codex#23572,
    #25846); current behavior unverified.
F6. Codex TUI can include `thread-id` in the terminal title via
    `[tui] terminal_title` in `~/.codex/config.toml`; tmux exposes it as
    `pane_title`. Codex 0.161 truncates UUIDs in titles to 29 chars + "...".
    The thread id in the title is the same UUID as the hook payload's
    `session_id` (per danhper's live verification; not yet re-verified here).
F7. Current live status on the owner's machine: the sidebar shows codex
    *right now* by coincidence — the running daemon was spawned by the codex
    TUI in pane %1, so its frozen `TMUX_PANE=%1` happens to be correct. This
    breaks as soon as a second codex TUI opens, the daemon is restarted by
    another client, or %1 dies.
F8. Codex hook trust state in `~/.codex/config.toml` is valid; hooks fire
    (F1). The failure is purely pane routing.
F9. The daemon inherits `$TMUX` from its spawner; routing queries therefore
    hit whatever tmux server that points at (default socket if unset).
    Multi-server setups can route against the wrong server. Both candidate
    designs share this limitation.

## 2. Constraints

C1. From `AGENTS.md`: preserve upstream module boundaries and data flow
    (hooks normalize events → handlers write tmux pane options → refresh loop
    reads → ui renders). Prefer config > existing seam > leaf module > small
    integration change. No parallel replacements of upstream subsystems.
    Default dependency-free; new dependencies need justification. This change
    touches a core entry point → a decision record in `docs/decisions/` is
    required before implementation.
C2. Owner's requirements: personal fork; target only current Codex behavior
    (no backward compatibility with pre-daemon Codex); correctness over
    cheapness.
C3. Tests must never touch the developer's live tmux server (use
    `TMUX_TMPDIR` isolation or the existing `tmux::test_mock`). Ratatui
    renders need inline insta snapshots.
C4. Fork-specific behavior that any design must not regress (absent
    upstream): response-review lifecycle `response_ready` →
    `response_reviewing` (`src/state/refresh.rs`), kimi adapter, quota/band
    blocks, activity log. Handlers `on_user_prompt_submit`, `on_stop`,
    `on_notification`, `on_permission_request` each call `set_agent_meta`
    (re-writes `@pane_agent`/cwd/session-id). A permission request may seed
    partial metadata, activity may log without a registered lifecycle, and
    Stop needs a matching session/turn; only lifecycle starts establish
    daemon routing provenance.
C5. `AgentEvent::Stop.response` is printed to stdout as the hook protocol
    response; with multi-pane fan-out it must be emitted exactly once.

## 3. Option space

| # | Design | New deps | Notes |
|---|---|---|---|
| A | Dual channel: WS pull (thread list/status) + title-routed push + config-writing setup (shipped by danhper, see §4) | tungstenite, toml_edit | Only known shipped fix |
| B | Push-only title routing (prior analysis, see §5) | none | Hooks remain sole pane-state writers |
| C | Pull-only via WS socket | tungstenite | Loses hook-only fidelity (prompt text, activity log, turn ids) |
| D | Session-keyed event journal; sidebar resolves session→pane each tick | none | Reverses hook→tmux-options data flow; re-implements handler pipeline in app state |
| E | cwd + process-tree fallback routing | none | Ambiguous on shared cwd; races on pane reuse |
| F | Rollout-file watching | none | No pane binding, no live waiting state, lags |
| G | `--no-daemon` opt-out as the "fix" | none | Verified working (F4); loses agents overview, --remote, desktop-app sharing |
| H | B first, add pull later via `codex app-server proxy` stdio bridge reusing `src/quota/codex.rs` machinery | none if proxy works | Gated on verifying the proxy on 0.162.x |

## 4. Candidate A — danhper's shipped implementation

Reference: `github.com/danhper/tmux-agent-sidebar`, commit `06a4738`
"Support Codex daemon clients with pane-to-thread mapping" (1,282 additions
and 47 deletions across 18 files, including lockfile changes). Fetch the full
patch before judging.

Design:
- Pull: worker thread connects the control socket (tungstenite over unix),
  `thread/loaded/list` + `thread/read` (read-only, paginated) → per-thread
  id/cwd/name/preview/status (`active|idle|systemError`, flags
  `waitingOnApproval`/`waitingOnUserInput`).
- Push: codex hook events scan all panes
  (`pane_id`, `@pane_session_id`, `pane_title`, `pane_pid`) and route to
  panes whose process tree contains codex AND that are bound to the event's
  thread — binding requires both an `@pane_session_id` match (persisted
  by the pull channel) and a title thread-id match. Fan-out to all matches; Stop response
  stripped for all but the first. `--no-daemon` panes excluded unless the
  hook process is their descendant.
- Setup: subcommand atomically rewrites `~/.codex/config.toml`
  (toml_edit, temp-file rename, concurrent-modification check) adding
  `thread-id` to `tui.terminal_title`.
- Explicit design rule (module header): never infer binding from cwd,
  recency, or the daemon's inherited `$TMUX_PANE`.

Strengths: daemon-truth status even when hooks don't fire (desktop-app-only
turns, daemon-only wait states); thread names/previews; support for 0.161
title truncation; multi-client attach handled.
Weaknesses (as applied to THIS fork): pull channel overrides hook-written
status at read time — conflicts with C4's response-review lifecycle; push
routing depends on pull-persisted `@pane_session_id` (channels not
separable); two new dependencies; query parser doubles as a tmux writer;
setup writes the user's config file (hook trust hashes also live there).

## 5. Candidate B — push-only title routing (prior analysis)

Source: an independent design pass by a planning subagent on 2026-10-11.
Treat it as analysis, not authority.

Design: for codex hook events, replace the `$TMUX_PANE` early-exit with a
pure router: `list-panes` + one process snapshot → route to panes where
(codex in process tree) AND (title's trailing UUID-shaped token matches the
payload `session_id`, full or 29-char-truncated prefix). Fan-out to all
matches; Stop response once; `--no-daemon`/`exec` preserved via an
ancestry fallback (hook pid ∈ inherited pane's process tree). Setup only
*prints* the `terminal_title` snippet (no config write, no toml_edit).
Optional: bounded SessionStart retry (3×400 ms) for the title race; a
startup notice when codex hooks are installed but `thread-id` is missing
from `terminal_title`.

Strengths: zero new dependencies; single state authority (hooks), upstream
data flow untouched; no bootstrap dependency between channels; stateless
session-switch safety (a late event from an old thread matches no title and
is dropped); small footprint (one leaf module + entry-point change).
Weaknesses: no daemon-truth status (daemon-only wait states invisible);
SessionStart may be dropped if the title isn't set yet (first paint delayed
until an accepted lifecycle start; C4 does not guarantee recovery from other
events); the pane appears only once a titled TUI client is attached (correct for desktop-only
turns, but nothing is displayed for them); truncated-prefix collision is
unsafe if resolved by dual-write; the quoted collision probability is not
supported for a structured UUIDv7 prefix. The implemented design requires
unique full-ID resolution instead. B shares F9's multi-server limitation with A.

## 6. Open verification items (run before finalizing any design)

V1–V5 results and corrections are recorded in the final design (see status above).

V1. Per-event `session_id` presence in daemon-mode payloads — especially
    `PostToolUse` and `PermissionRequest` (if PostToolUse lacks it, push-only
    activity routing is impossible and the pull channel's weight rises).
    Method: temporary logging hook in `~/.codex/hooks.json`, one live turn.
V2. Title timing: does the TUI set the thread-id title before SessionStart
    fires? Measure the race; decides whether B needs the bounded retry.
V3. `pane_title` propagation under the owner's tmux config (OSC passthrough).
V4. Whether `codex app-server proxy` works on 0.162.x (decides if H is
    dependency-free).
V5. F6 re-verification on the installed build (title thread-id == payload
    session_id).

## 7. Open decision points for the evaluator

D1. A vs B vs H vs something else entirely.
D2. If push-only: include the SessionStart retry? (≈10 lines, slight hook
    latency vs earlier first paint)
D3. Config-missing notice: same change or follow-up?
D4. If a pull channel is wanted: tungstenite vs stdio-proxy vs hand-rolled
    WS-over-unix (~150 lines, protocol re-implementation).
D5. How to handle F9 (daemon's `$TMUX` server affinity) — document-only or
    defend in code?

## 8. Expected deliverable

A verdict on D1 with reasoning grounded in the facts above (re-verify any
claim you rely on — several were gathered under time pressure), resolution
of D2–D5, and an implementation outline with integration points and a test
plan compatible with C3. If you find a design outside A–H, even better.
