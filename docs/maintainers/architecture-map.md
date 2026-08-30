# Architecture and Hotspot Map

## Stable Data Flow

```text
Agent hook JSON
  -> adapter::{claude,codex,opencode}
  -> AgentEvent / AgentEventKind
  -> cli::hook handlers
  -> tmux @pane_* options + activity logs
  -> tmux::query_sessions
  -> AppState::refresh
  -> group/filter/layout derivation
  -> ui::draw
```

Keep normalization at the adapter boundary. Handlers consume `AgentEvent`, not
raw JSON or agent-specific field names. The TUI reads tmux state through the
query layer; renderers consume state and only record ephemeral layout targets.

## Extension Seams

| Need | Preferred seam | Avoid |
|---|---|---|
| New agent event | Adapter `HOOK_REGISTRATIONS` + `AgentEvent` + one handler | Parsing agent JSON in handlers |
| New pane metadata | Constant in `tmux/options.rs`, query field, typed consumer | Ad hoc `tmux show` calls in UI |
| New workflow state | Existing `PaneStatus`/wait-reason lifecycle and state submodule | A parallel status model in rendering |
| New repository ordering/filter | `group.rs` and `state/filter.rs` | Sorting independently in row rendering |
| New interaction | `app/input.rs` -> `AppState` method -> layout target | Running tmux commands from renderer code |
| New visual preference | Theme/icon option and leaf renderer | Forking the top-level draw pipeline |
| New CLI behavior | Thin dispatch in `cli/mod.rs`, implementation in a focused module | Expanding `main.rs` orchestration |

## Hotspots

### `src/cli/toggle.rs`

Owns sidebar pane creation, focus, close, layout restoration, and restart.

Invariants:

- target panes are resolved explicitly;
- restart does not switch clients, sessions, windows, or active panes;
- saved layout/zoom state is restored or cleared coherently;
- executable paths passed through tmux are shell-quoted;
- tests use the `TmuxClient` fixture, never a live server.

Minimum checks: toggle unit tests, `cargo clippy`, and an isolated tmux smoke for
changes to live pane lifecycle.

### `src/state.rs` and `src/state/refresh.rs`

Own central state composition and periodic synchronization.

Invariants:

- global, per-pane, and local state retain their documented ownership;
- refresh derives state and prunes stale panes without rendering side effects;
- background work stays off the input/render path;
- cursor/filter persistence cannot overwrite newer external state;
- new fields are placed in an existing topical sub-structure where possible.

Minimum checks: state unit tests plus relevant `tests/state_tests.rs` coverage.
Update `docs/state-management.md` when scope or cadence changes.

### `src/tmux/query.rs`

Owns the single bulk tmux query and conversion into typed pane/session data.

Invariants:

- preserve one `list-panes -a` query for the main refresh path;
- keep field indexes and format construction synchronized;
- reject stale agent metadata conservatively;
- parsing remains independent of the rendering layer.

Minimum checks: query parser tests and any stale-process regression tests.

### `src/cli/hook.rs` and `src/cli/hook/handlers/`

Own event dispatch and durable pane-state transitions.

Invariants:

- serialize writes per pane;
- session and turn identifiers reject stale or duplicate completion events;
- teardown clears every owned pane option;
- adapter-specific payload details do not leak into handlers;
- notifications do not define workflow state independently.

Minimum checks: adapter drift tests, handler lifecycle regressions, and option
cleanup tests.

### `src/group.rs`, `src/state/filter.rs`, and `src/state/popup.rs`

Own repository identity, workflow priority, filtering, and repo selection.

Invariants:

- display names may change, stable repository identity may not;
- urgent, ready, working, and parked tiers remain explicit;
- status and repository filters compose rather than overwrite one another;
- popup selection stores the stable ID, not a disambiguated label.

Minimum checks: group ordering/identity tests, filter tests, and popup tests.

### `src/ui/` and snapshot tests

Own presentation only.

Invariants:

- fixed-format controls have stable widths;
- icons/colors come from the theme surface;
- click targets are generated from the same layout that was rendered;
- visual tests use complete inline snapshots;
- narrow layouts keep complete controls or intentionally omit them.

Minimum checks: relevant inline snapshots and styled color assertions. For
visual changes, inspect a real capture in addition to accepting snapshots.

## Cross-File Contracts

| Contract | Files that must move together |
|---|---|
| Hook registration | adapter table/parser, `hooks/hooks.json`, setup output, adapter/setup tests, `tests/plugin_hooks_tests.rs` |
| Pane option | `tmux/options.rs`, hook writer/cleanup, query parser, state docs |
| Status/icon | tmux type, state filter/counts, UI icon/theme, snapshots, tmux option docs |
| CLI command | `cli/mod.rs`, implementation, README/docs when public, command tests |
| Version | `Cargo.toml`, `Cargo.lock`, `.claude-plugin/plugin.json`, tag |

## Decision Records

Create `docs/decisions/YYYY-MM-DD-topic.md` only for changes that cross the soft
divergence budget in the fork strategy. Use this minimal structure:

```markdown
# Decision: Topic

## Context
## Chosen Seam
## Alternatives Rejected
## Upstream Compatibility
## Conflict and Removal Strategy
```
