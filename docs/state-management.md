# State Management Architecture

## State Scope & Update Frequency

Every piece of state belongs to one of three scopes: **Global** (stored in tmux
variables), **Per-pane** (keyed by tmux pane ID), or **Local** (owned by the
singleton sidebar process). The table below shows where each field lives, how
often it updates, and what triggers the update.

### Global State (synced via tmux global variables)

Stored in `GlobalState`. Written to tmux on change, with the cursor save
debounced briefly so selection changes do not block redraw/input handling;
reloaded on SIGUSR1.

| Field | Tmux Variable | Update Trigger | Description |
|-------|--------------|----------------|-------------|
| `status_filter` | `@sidebar_filter` | User input (left/right key) | Active status filter (All/Running/Background/Waiting/Idle/Error) |
| `selected_pane_row` | `@sidebar_cursor` | User input (j/k key); tmux write flushed after a short debounce | Cursor position in agent list |
| `repo_filter` | `@sidebar_repo_filter` | User input (repo popup) | Repository filter (All or specific repo) |
| sidebar lifecycle intent | `@agent_sidebar_enabled` | Explicit open/close | Durable policy used by event-driven lifecycle reconciliation; live pane and empty-slot topology remain derived from tmux pane inventory |

Each field has a corresponding `last_saved_*` to prevent sync conflicts — only overwrites tmux if the local write succeeded.

### Per-pane State (keyed by pane ID)

Written by `cli/hook.rs` on agent events, read by `query_sessions()` every **1 second**.

Each pane's runtime data is split into two buckets:

| Source | Update Trigger | Description |
|--------|----------------|-------------|
| tmux pane options | Event-driven + cleanup on agent exit | Agent type, status, cwd, permission mode, prompt, subagents, worktree, etc. |
| `PaneRuntimeState` in `AppState` | Refresh cycle + cleanup on agent exit | `ports`, `command`, `task_progress`, `task_dismissed_total`, `inactive_since` |

Pane options written to tmux:

| Tmux Option | Update Trigger | Description |
|-------------|----------------|-------------|
| `@pane_agent` | SessionStart | Agent type ("claude" / "codex" / "kimi" / "opencode") |
| `@pane_status` | Every event | Status ("running" / "background" / "waiting" / "idle" / "error") |
| `@pane_status_changed_at` | Every status transition | Unix epoch milliseconds used to order repositories newest-first inside a workflow tier |
| `@pane_cwd` | SessionStart, CwdChanged | Working directory |
| `@pane_permission_mode` | SessionStart, hook event | Permission mode |
| `@pane_prompt` | UserPromptSubmit, Stop | Latest prompt or response text |
| `@pane_prompt_source` | UserPromptSubmit, Stop | "user" or "response" |
| `@pane_started_at` | UserPromptSubmit | Unix epoch when agent started |
| `@pane_attention` | SessionStart, UserPromptSubmit, Interrupt, StopFailure, PermissionResult, response-review transitions (clear); Stop, TaskCompleted, actionable Notification, PermissionRequest/Denied, TeammateIdle (set) | "notification" or "clear". Any status write of `running`/`idle` also unsets it |
| `@pane_wait_reason` | Stop, StopFailure, PermissionRequest, PermissionDenied, TeammateIdle, focus review transition (set); UserPromptSubmit, PermissionResult, Interrupt (clear) | Reason for waiting/error, including internal `response_ready` / `response_reviewing` lifecycle markers |
| `@pane_bg_cmd` | ActivityLog (bg Bash), Refresh sweep (clear), SessionEnd (clear) | Latest sanitized command of a Bash tool started with `run_in_background`. It persists across turns and remains visible while a completed response is awaiting review. After review, the pane returns to `background` while this marker is live. The refresh loop runs a `ps`-based liveness sweep at most every 5 seconds (the first sweep is immediate) and clears the marker when no process matches the stored command. Only the most recent background Bash is tracked. |
| `@pane_subagents` | SubagentStart/Stop | Comma-separated active subagent list |
| `@pane_worktree_name` | SessionStart | Worktree name (if applicable) |
| `@pane_worktree_branch` | SessionStart | Worktree branch (if applicable) |
| `@pane_session_id` | SessionStart, UserPromptSubmit, Notification, Stop, StopFailure, PermissionDenied, CwdChanged | Agent-reported session id (skipped when subagents are active) |
| `@pane_role` | Sidebar lifecycle only | `sidebar` marks the live TUI pane only with `@agent_sidebar_owner=tmux-agent-sidebar` or a matching pane/`@sidebar_pid`; `sidebar-slot` marks a strictly validated processless layout placeholder. Both valid roles are excluded from agent state. |
| `@pane_turn_id` | UserPromptSubmit (set when the hook reports a turn id), SessionStart / teardown (clear) | Upstream turn id currently allowed to mutate the pane's workflow state; lifecycle hooks reject events from another turn |
| `@pane_completed_turn_id` | Stop, StopFailure, Interrupt (completion stamp), UserPromptSubmit / SessionStart / teardown (clear) | Last turn whose completion transition was applied, so late PostToolUse or permission events from a finished turn are ignored |
| `@pane_notification_run_id` | SessionStart, UserPromptSubmit | Unix epoch milliseconds regenerated per run; scopes desktop-notification fingerprints so a restart does not dedupe against the previous run |
| `@pane_os_notify_task_completed` / `@pane_os_notify_task_failed` / `@pane_os_notify_permission_required` | Each fired desktop notification of that kind | `timestamp\|fingerprint` stamp that suppresses duplicate notifications inside the cooldown |
| `@pane_pending_worktree_remove` | WorktreeRemove while subagents are active (set); SubagentStop draining to empty, SessionStart (clear) | Defers worktree teardown until the last subagent exits the shared pane |

In-memory per-pane runtime state. Every field lives inside
`PaneRuntimeState` so the whole record is dropped together when its
pane disappears (`prune_pane_states_to_current_panes`).

| Field | Update Frequency | Description |
|-------|-----------------|-------------|
| `pane_states.map[...].ports` | Every 10s (port scan) | Listening localhost ports detected from the pane process tree |
| `pane_states.map[...].command` | Every 10s (port scan) | Best-effort commandline for the pane process tree, with tmux command fallback in the UI |
| `pane_states.map[...].task_progress` | Every 1s (refresh cycle) | Parsed from activity log — task list per pane |
| `pane_states.map[...].task_dismissed_total` | On task completion | Tracks dismissed completed-task counts |
| `pane_states.map[...].inactive_since` | On status change | Debounce timestamp (3s grace before hiding tasks) |
| `pane_states.map[...].tab_pref` | On user tab switch | Remembered bottom tab choice per pane (cleared on relaunch) |
| `pane_states.map[...].task_progress_log_mtime` | Every 1s (refresh cycle) | mtime of the task-progress log last parsed; skips re-parsing when unchanged |

Per-pane file-based state:

| File | Update Trigger | Read Frequency | Description |
|------|---------------|----------------|-------------|
| `/tmp/tmux-agent-activity_{pane_id}.log` | Each ActivityLog event | Every 1s | Tool usage log (`HH:MM\|tool\|label`), max 200 lines. Readers split each line on its first two `\|`, so a label keeps its own pipes — only newlines are replaced — and a shell command arrives at the Activity block exactly as the agent ran it. The tool name and label are canonical before the write: adapters map each agent's own tool names and argument keys, the label strategy table extracts one field per tool, and a tool with no strategy row falls back to its first describing argument or its key list |

### Local State (single sidebar process only)

| Field | Update Frequency | Description |
|-------|-----------------|-------------|
| `repo_groups` | Every 1s | Panes grouped by git repo root and ordered by workflow tier: urgent, unread completion, active work, reviewed/parked. Each tier is newest-first. |
| `focus_state.focused_pane_id` | Every 1s, plus immediately on user-initiated pane jumps | Currently focused agent pane |
| `focus_state.sidebar_focused` | Every 1s | Whether sidebar pane itself has focus |
| `focus_state.focus` | On user input | UI focus: `Filter` / `Panes` / `ActivityLog`; input also triggers an immediate redraw so focus changes appear without waiting for the next poll tick |
| `focus_state.pending_g` | On input | Local Vim `gg` prefix with a one-second lifetime, bound to its starting focus; consumed or cancelled by subsequent input, never persisted |
| `focus_state.prev_focused_pane_id` | Every 1s | Previous focused pane ID (for detecting focus changes) |
| `now` | Every 1s | Current Unix epoch |
| `scrolls.panes` | On user input / render | Agent list scroll position |
| `scrolls.git` | On user input / render | Git status scroll position |
| `activity.scroll` | On user input / render | Activity log scroll position |
| `activity.entries` | Every 1s | Focused pane's activity entries (max 50) |
| `activity.selected` | On user input / render | Cursor into `activity.entries`: the entry the block highlights and the one `y` copies (`0` = newest = top row). Re-anchored by `replace_entries` on every refresh, and scrolled into view each frame |
| `activity.max_entries` | Once at startup | Max activity log entries to display |
| `activity.log_cache` | Every 1s | `(focused_pane_id, mtime)` of the last-rendered activity log; skips re-reads when unchanged |
| `git` | Every 2s (bg thread) | Branch, diff stats, ahead/behind, PR number |
| `bottom_tab` | On user input / auto-switch | Current bottom panel tab |
| `theme` | Once at startup | Color theme from tmux `@sidebar_color_*` variables |
| `flash` | On spawn / remove / copy feedback | Transient one-line status banner (`message`, expiry) cleared once its deadline passes |
| `popup` | On user input / render | `PopupState` enum: `None` / `Repo { selected, query, area }` / `Notices { area }` / `SpawnInput { input, target_repo, target_repo_root, agent_idx, mode_idx, field, anchor_y, error, area }` / `RemoveConfirm { pane_id, branch, error, area }`. Enforces "at most one popup open" via the type system |
| `layout` | Every frame (render) | `FrameLayout` sub-struct bundling the ephemeral fields the UI rewrites every frame for click hit-testing: `pane_row_targets`, `line_to_row`, `header_width`, `repo_button_col`, `repo_spawn_targets`, `spawn_remove_targets`, `hyperlink_overlays`, `quota_block_rows`, `band_blocks` |
| `notices` | Once at startup / on copy | `NoticesState` sub-struct: `button_col`, `hook_check_agents`, `missing_hook_groups`, `claude_plugin_status`, `claude_settings_has_residual_hooks`, `claude_plugin_notice`, `copy_targets`, `copied_at` |
| `timers` | Refresh cycle / on user input | `RefreshTimers` sub-struct gating periodic work: `last_filter_click` (debounce), `last_port_refresh`, `port_scan_initialized`, `last_bg_shell_sweep` |
| `pending_osc52_copy` | On copy / frame flush | OSC 52 clipboard payload queued for terminal forwarding — set by both the notices copy and the Activity command copy |
| `pending_clipboard_copy` | On copy / frame flush | Payload for the clipboard sinks that need a process or a platform call (`arboard`, `tmux set-buffer`). Queued by input and flushed after the next frame so a key press never blocks the render path on a clipboard round trip |
| `pet_state` | Every 200ms (animation) | `Idle` / `WalkRight` / `Working` / `WalkLeft`. Working is driven by any `PaneStatus::is_active()` pane (running, background, or waiting), so a background-only sidebar keeps the pet at its desk |
| `pet_x` / `pet_frame` | Every 200ms (animation) | Pet X position and sprite frame |
| `pet_bob_timer`, `pet_working_frame_tick`, `pet_walk_*`, `pet_idle_*`, `pet_working_paper_*` | Every 200ms (animation) | Animation clocks and LCG seeds the pet state machine advances: the idle bob/blink/wave schedule, the walk-bounce pacing, and the working paper shuffle. Each motion keeps its own counter and seed so they do not fire in lockstep |
| `pet_enabled` | Once at startup | Whether the pet is drawn and ticked (from `@sidebar_pet`, default `off`). The pet renders in the agents-panel filler band above the tab band and the quota block; it no longer requires the bottom panel to be visible. It is funded last, so it is the first thing to lose its rows when vertical space runs out |
| `icons` | Once at startup | `StatusIcons` theme (overridable via tmux options) |
| `tmux_pane` | Once at startup | This sidebar's own tmux pane ID |
| `bottom_panel_height` | Once at startup | Bottom-panel height in lines from `@sidebar_bottom_height` (`0` hides the panel) |
| `pane_states.seen` | Every 1s | Set of pane IDs that have been seen as agents (bundled with `pane_states.map` under the `PaneRuntimeMap` wrapper) |
| `version_notice` | Test/debug fixtures only | Optional version-notice rendering state; production never performs a remote update check |
| `sessions.names` | Every 10s (background thread) | `session_id → session name` map; scanned by `session_poll_loop` in `app/workers.rs` so the TUI thread never blocks on filesystem I/O |
| `sessions.dirty` | On session map refresh / application tick | Marks the session map as changed so the per-pane session label walk only runs when needed |
| `quota.codex` / `quota.kimi` | Every 5 min, plus each window reset (background thread, backs off to 30 min while failing) | Last good Codex (ChatGPT) and Kimi Code quota snapshot per subscription: the 5-hour and weekly windows with remaining percentage and reset time, plus a fetch-failed flag that dims the row and replaces the countdowns with an age marker. `quota_poll_loop` (`src/quota/mod.rs`, spawned from `app/workers.rs`) fetches both subscriptions independently and reports per-subscription results, so one failure never masks the other's success. Missing credentials/binaries leave the subscription absent forever. Reset stamps are Unix seconds — the same clock as `AppState::now`. A successful fetch pulls the next attempt forward to the earliest `resets_at` (floored at 60s), so a rolling window is picked up about a minute after it rolls instead of up to a full interval later. The cadence deliberately never reacts to agent activity — a fetch spawns a subprocess, hits the network, and shares the user's own OAuth credentials, so the click on the block is the manual escape hatch |
| `quota_enabled` | Once at startup | Whether the quota block renders (from `@sidebar_quota`, default `on`). The renderer reserves rows only from the space the agent list does not use, collapsing full (header + one row per subscription: both windows with percentages and reset countdowns) → compact (same rows without the header and countdowns) → hidden. Percentages are colored on a configurable battery scale (`@sidebar_color_quota_*`) and the subscription name uses its agent identity color |
| `band_enabled` | Once at startup | Whether the agents panel hosts the bottom tabs in its idle rows while the bottom panel is hidden (from `@sidebar_band`, default `on`). The band stacks **both** tabs as separate blocks (Activity above Git) directly above the quota block, sizes each to its own content, and compresses the pair row by row into whatever rows the list leaves free — a block only disappears when even its title bar plus one content row does not fit. `Left`/`Right` (or `h`/`l`) move the keyboard focus between the blocks while the band has it. The border has three levels: accent for the block the keys are driving, `text_muted` for the block they would land on while the keyboard is still in the agent list, `border_inactive` for the rest. `@sidebar_bottom_height > 0` takes precedence: the tabs stay in the bottom panel and the band stays hidden |
| `quota_force_refresh` | On a click on the quota block | Shared `AtomicBool` the renderer sets and `quota_poll_loop` consumes to refetch immediately; the input path never performs the fetch itself |
| `usage` | Every 30s + on any agent status change (background thread) | DeepSeek spend row state (`src/usage.rs`): the selected window (`Today` / `SevenDays` / `ThirtyDays`, cycled by clicking the row), the last snapshot for that window, a single-value window mailbox, and the force flag the pane-status trigger sets. `usage::poll_loop` (spawned from `app/workers.rs`) parses `$CODEX_HOME/sessions/**` and `~/.claude/projects/**`, caches per file by `(mtime, size)`, and never blocks the TUI thread. A result whose window no longer matches the selection is dropped, so a stale scan can never be painted under the wrong label |

### Activity block rendering

The Activity block keeps one row per entry, with one exception: the **cursor
entry** wraps as far as its label needs, so the command being read is shown in
full. That does not depend on focus — the cursor marks what is being read, and
reading the log is not something that only happens while the block owns the
keyboard. Every other entry stays on one row, clipped with an ellipsis, which is
also what `content_height` counts.

| Row | Content |
| --- | --- |
| Command entry | `HH:MM command…` — no tool column: the label *is* the command |
| File/agent entry | `HH:MM Tool label…` — the tool name is what separates a filename from a pattern |
| Continuation | Flush at the gutter and full width — the first row is the only one that pays for `HH:MM` |

Every row starts with a one-column gutter: `┃` in the accent color on the cursor
entry's rows, a blank for the rest. The guide line is the whole cursor — no
background behind the entry, because a background across wrapped, syntax-colored
text costs more legibility than it buys. `activity.scroll` is derived: the cursor
anchors the viewport every frame, and `scroll_bottom`'s Activity arm moves the
cursor rather than an offset, so `j`/`k`, `Ctrl-D`/`Ctrl-U`, `gg`/`G`, and the
wheel all move the selection.

Shell commands (Bash, PowerShell, Monitor labels, which the hook stores as the
command line) are parsed with the `tree-sitter-bash` grammar and its
`HIGHLIGHT_QUERY`; the grammar's highlight names map onto existing theme slots
(command word `text_active` + bold, options `activity_interaction`, strings
`activity_edit`, operators/keywords/comments `text_inactive`, numbers and
properties `activity_read`). Everything the grammar leaves unlabelled — a bare
word, a glob, a path — keeps the plain body color, and so does a non-command
label (a basename, a glob, a URL, a subagent paragraph).

`y` copies the **cursor** entry's label through
`AppState::request_activity_copy`: the input path only queues the payload, and
the frame loop writes it to the OS clipboard, the tmux paste buffer
(`tmux set-buffer`), and the terminal (OSC 52). It answers whenever the footer
owns the keyboard (`Focus::ActivityLog`) — not only while the movement keys
happen to drive the Activity block — so the binding has no invisible
precondition.

---

## Update Cycle Summary

```
┌─────────────────────────────────────────────────────────────┐
│  Every frame (redraw on demand)                             │
│  layout.* (rebuilt by ui::draw), pet animation              │
├─────────────────────────────────────────────────────────────┤
│  Every 1s (refresh cycle)                                   │
│  repo_groups, focus_state, activity.entries,                │
│  layout.pane_row_targets, task_progress                     │
├─────────────────────────────────────────────────────────────┤
│  Every 10s (port scan, background)                          │
│  pane_states.map[..].ports, agent liveness cleanup          │
├─────────────────────────────────────────────────────────────┤
│  Every 10s (session_names background thread)                │
│  sessions.names map populated by session_poll_loop          │
├─────────────────────────────────────────────────────────────┤
│  Every 5s (bg-shell liveness sweep)                         │
│  @pane_bg_cmd cleared when no process matches               │
├─────────────────────────────────────────────────────────────┤
│  Every 5 min + each window reset (backoff 30 min)           │
│  quota.codex / quota.kimi subscription snapshots            │
│  (countdown text itself is recomputed every 1s tick)        │
├─────────────────────────────────────────────────────────────┤
│  Once at startup                                            │
│  theme, icons, bottom_panel_height, quota_enabled,          │
│  pet_enabled, band_enabled, hook_check_agents,              │
│  notices.claude_plugin_*, global filter state,              │
│  notices.claude_settings_has_residual_hooks,                │
│  notices.claude_plugin_notice, notices.missing_hook_groups  │
├─────────────────────────────────────────────────────────────┤
│  Every 2s (git background thread)                           │
│  git (branch, diff, ahead/behind, PR)                       │
├─────────────────────────────────────────────────────────────┤
│  On SIGUSR1 (tmux focus change)                             │
│  GlobalState reloaded from tmux variables                   │
│  (also re-read after 2 inactive refresh ticks)              │
├─────────────────────────────────────────────────────────────┤
│  Event-driven (agent hooks)                                 │
│  @pane_* tmux options, activity log files                   │
├─────────────────────────────────────────────────────────────┤
│  On user input                                              │
│  focus_state.focus, scrolls.*, activity.scroll,             │
│  bottom_tab, GlobalState fields,                            │
│  popup (PopupState enum), timers.last_filter_click,         │
│  immediate selection / active-pane redraw                   │
├─────────────────────────────────────────────────────────────┤
│  Every frame (render)                                       │
│  layout.line_to_row, popup.area, notices.button_col,        │
│  notices.copy_targets, layout.hyperlink_overlays            │
└─────────────────────────────────────────────────────────────┘
```

---

## Data Flow

```
Agent hooks (hook.sh)
  → CLI `hook` subcommand (cli/hook.rs)
    → resolve_adapter() (event/adapter.rs) → adapter.parse() → AgentEvent
    → handle_event() writes @pane_* tmux options + /tmp activity log files
                        ↓
TUI main loop (app::run in app.rs; submodules app/{setup,workers,input,render})
  → startup plugin-state reads (cli/plugin_state.rs)
    → installed_plugins.json / ~/.claude/settings.json
    → initializes Claude notices state once
                        ↓
  → refresh() every 1s
    → query_sessions() (tmux.rs)     ← reads @pane_* via `tmux list-panes -a`
    → group_panes_by_repo() (group.rs)
    → rebuild_row_targets()          ← applies GlobalState filters
    → refresh_activity_data()        ← reads /tmp activity logs
    → refresh_task_progress()        ← updates PaneRuntimeState.task_progress
    → refresh_port_data()            ← updates PaneRuntimeState.ports
    → scan_session_process_snapshot() ← detects dead panes and clears stale tmux metadata
                        ↓
  → git_rx.try_recv()                ← receives GitData from background thread
  → notices popup render/copy state  ← derived from AppState plugin fields
                        ↓
  → ui::draw() renders frame         ← reads all AppState fields
```

---

## Key Types

```rust
enum Focus { Filter, Panes, ActivityLog }
enum StatusFilter { All, Running, Background, Waiting, Idle, Error }
enum RepoFilter { All, Repo(String) }
enum BottomTab { Activity, GitStatus }
enum PaneStatus { Running, Background, Waiting, Idle, Error, Unknown }
enum AgentType { Claude, Codex, Kimi, OpenCode, Unknown }
enum PermissionMode { Default, Plan, AcceptEdits, Auto, DontAsk, BypassPermissions, Defer }

/// At-most-one popup state. The enum encodes both which popup is open
/// and its per-popup data so the invariant is checked by the type system.
enum PopupState {
    None,
    Repo { selected: usize, query: String, area: Option<Rect> },
    Notices { area: Option<Rect> },
    /// Modal text input shown when the user spawns a new worktree.
    SpawnInput {
        input: String,
        target_repo: String,
        target_repo_root: String,
        agent_idx: usize,
        mode_idx: usize,
        field: SpawnField,
        anchor_y: Option<u16>,
        error: Option<String>,
        area: Option<Rect>,
    },
    /// Confirmation prompt shown when the user removes a sidebar-spawned pane.
    RemoveConfirm {
        pane_id: String,
        branch: String,
        error: Option<String>,
        area: Option<Rect>,
    },
}

struct ScrollState {
    offset: usize,
    total_lines: usize,
    visible_height: usize,
}

struct HyperlinkOverlay {
    x: u16,
    y: u16,
    text: String,
    url: String,
}

struct PaneRuntimeState {
    ports: Vec<u16>,
    command: Option<String>,
    task_progress: Option<TaskProgress>,
    task_dismissed_total: Option<usize>,
    inactive_since: Option<u64>,
    tab_pref: Option<BottomTab>,
    task_progress_log_mtime: Option<SystemTime>,
}

/// Wraps `PaneRuntimeState` per pane plus the set of pane IDs that
/// have been seen as agents. Methods delegate to the underlying
/// `HashMap`; `seen` is read/written alongside `map` during refresh.
struct PaneRuntimeMap {
    map: HashMap<String, PaneRuntimeState>,
    seen: HashSet<String>,
}

/// Focus-related fields grouped so UI code can pass them as a single
/// sub-struct rather than juggling five flat fields.
struct FocusState {
    sidebar_focused: bool,
    /// Pending normal-mode `g` prefix, scoped to the focus where it started.
    pending_g: Option<(Instant, Focus)>,
    focus: Focus,
    focused_pane_id: Option<String>,
    prev_focused_pane_id: Option<String>,
}

/// Non-activity scrolls (the agent list and the git bottom panel).
/// Activity's scroll lives inside `ActivityState` because it pairs
/// with the activity entries buffer.
struct ScrollStates {
    panes: ScrollState,
    git: ScrollState,
}

/// Activity-log snapshot for the focused pane plus cache metadata so
/// the polling tick can skip redundant file reads.
struct ActivityState {
    entries: Vec<ActivityEntry>,
    scroll: ScrollState,
    selected: usize,
    max_entries: usize,
    log_cache: Option<(String, SystemTime)>,
}

/// Session-name map scanned by a background thread so the TUI thread
/// never blocks on `~/.claude/sessions/*.json` reads.
struct SessionNamesState {
    names: HashMap<String, String>,
    dirty: bool,
}

/// Frame-scoped render output cached for click hit-testing. Rewritten
/// every frame by the UI layer; consumed by mouse/keyboard handlers
/// before the next render.
struct FrameLayout {
    pane_row_targets: Vec<RowTarget>,
    line_to_row: Vec<Option<usize>>,
    header_width: u16,
    repo_button_col: Option<u16>,
    repo_spawn_targets: Vec<RepoSpawnTarget>,
    spawn_remove_targets: Vec<SpawnRemoveTarget>,
    hyperlink_overlays: Vec<HyperlinkOverlay>,
    /// Screen rows the quota block covers, as an inclusive `(first, last)`
    /// pair; `None` when the block is hidden.
    quota_block_rows: Option<(u16, u16)>,
    /// Screen rects of the stacked band blocks, top-first, paired with the
    /// tab each one shows; empty when the band is hidden.
    band_blocks: Vec<(Rect, BottomTab)>,
}

/// Periodic-refresh bookkeeping. session_names refresh is intentionally
/// NOT here — it lives in a dedicated background thread so the TUI
/// thread never performs blocking filesystem I/O.
struct RefreshTimers {
    last_filter_click: Instant,
    last_port_refresh: Instant,
    port_scan_initialized: bool,
    last_bg_shell_sweep: Option<Instant>,
}

/// All fields for the header status indicator and its popup.
struct NoticesState {
    button_col: Option<u16>,
    missing_hook_groups: Vec<NoticesMissingHookGroup>,
    claude_plugin_status: ClaudePluginStatus,
    claude_settings_has_residual_hooks: bool,
    claude_plugin_notice: Option<ClaudePluginNotice>,
    copy_targets: Vec<NoticesCopyTarget>,
    copied_at: Option<(String, Instant)>,
}
```

---

## State Invariants

1. `selected_pane_row` is always < `layout.pane_row_targets.len()` — clamped in `rebuild_row_targets()`
2. `activity.entries` contains only the focused pane's entries — cleared on focus change
3. Tab preferences persist per pane in `PaneRuntimeState.tab_pref` and are restored on focus change. They vanish together with the rest of `PaneRuntimeState` when the pane is pruned, so a relaunched agent starts on the default tab
4. Git fetching respects the `git_tab_active` flag — stops when tab is hidden
5. Task progress has a 3-second debounce — prevents flicker when agent briefly pauses
6. Global state persists via tmux variables across sidebar pane moves and restarts
7. Scroll positions are independent per panel — agents, activity, git each have their own `ScrollState`
8. `layout.line_to_row` is rebuilt every frame — ensures accurate click routing
9. Pane runtime state is pruned when the pane disappears — prevents stale per-pane ports, task progress, and tab preferences from surviving after the agent is gone
10. At most one popup is open at a time — enforced structurally by the `PopupState` enum, not by parallel boolean flags
11. Hook-based cleanup wins when available; pid-based cleanup is a slower fallback that removes panes when the agent process is gone but the hook did not fire
