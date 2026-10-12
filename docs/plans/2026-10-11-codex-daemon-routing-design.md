# Codex daemon hook routing design

Date: 2026-10-11
Status: Implemented and verified; local deployment completed on 2026-10-12.
Repository baseline: `95d3b52393e8ff0c3b5f5266fdb7d7e63246c574`.
Verified Codex build: `0.162.1`; source tag `rust-v0.162.1`, commit
`092d3acd6bec3e3a14bdc7e7a2810ab628ab759d` (peeled commit;
`7a078f6bb4ce0dc2a81408b334ce52979d92c603` is the annotated tag object).

Related documents:

- [Neutral evaluation briefing](2026-10-11-codex-daemon-routing-evaluation.md)
- [Decision record](../decisions/2026-10-11-codex-daemon-routing.md)
- [Architecture map](../maintainers/architecture-map.md)
- [Fork strategy](../maintainers/fork-strategy.md)
- [State management](../state-management.md)
- [Verification](../maintainers/verification.md)

## 1. Verdict and decision points

Choose **I: client-title routing, hook-owned lifecycle transitions, and an
ID-only daemon query to resolve truncated titles**. This is a hardened version
of B, with a narrow part of A's transport capability. It has no status poller,
thread metadata cache, or pull-created pane registration.

The selected pane is a client-side fact. Hook lifecycle events are server-side
facts. A hook targets every supported TUI pane whose observed title resolves
to the event's full `session_id`. A daemon inventory helps resolve the shortened
identity; it does not supply the pane binding. Existing handlers retain status,
prompt, activity, session, and turn semantics. Refresh retains the local response
review workflow.

| Decision | Resolution | Reason |
|---|---|---|
| D1 | I, as specified here | Preserves this fork's state flow while rejecting detected title collisions; routing has no pull bootstrap dependency. |
| D2 | No SessionStart retry | Titles preceded SessionStart in the isolated new-session observations. A retry cannot recover a missing client-selection event, and a delayed SessionStart can reset newer turn state. |
| D3 | Prerequisite guidance and notice in the same implementation | Installed hooks can still be unusable without titles or explicit endpoints. Silent omission would make this fix difficult to operate. |
| D4 | Minimal-feature `tungstenite` over a Unix stream | The installed proxy is a byte relay, not a newline JSON to WebSocket adapter. Hand-written framing adds avoidable protocol ownership. |
| D5 | Defend server affinity through explicit command configuration | Never let daemon-inherited `TMUX` choose the Codex routing server. Pin the intended server independently of the daemon environment. |

This is the best supported local design found for the current interface and
fork constraints, not a claim of an established industry standard. The durable
upstream solution is a per-client, full-ID displayed-thread signal. Current
titles are shortened and updated asynchronously. No local design based on that
signal can promise atomic selection-to-write correctness in every race. If that
stronger guarantee is mandatory, a supported upstream signal is a prerequisite;
adding more daemon polling does not establish it.

## 2. Evidence and corrections to the briefing

### Direct verification of V1-V5

The live checks used a dedicated tmux socket, a temporary Codex home with
trusted diagnostic hooks, and two TUI clients sharing that temporary daemon.
The logger recorded event names, field names, IDs, parent PID, inherited tmux
identity, and pane titles. It did not record prompt or tool contents. Temporary
credentials were removed, the temporary daemon was stopped, and only the named
isolated tmux session was cleaned up.

| Item | Observation | Scope |
|---|---|---|
| V1 | SessionStart, UserPromptSubmit, PostToolUse, PermissionRequest, and Stop all carried full `session_id`. All except SessionStart carried the observed turn ID. | All five currently registered Codex events, one exercised daemon turn with a permission request. |
| V2 | Both new clients exposed their thread title before their SessionStart hook. Source queues pending SessionStart until turn processing. | Two new sessions; not proof for every resume, switch, remote client, or scheduling race. |
| V3 | tmux exposed Codex OSC title changes, including with `allow-passthrough=all` and `set-titles=off`, matching the owner's queried options. | Isolated writes and live title readback; not a reproduction of every owner tmux option. |
| V4 | The proxy produced no JSON response within an eight-second observation. A direct Unix WebSocket upgrade returned HTTP 101 and answered initialize and `thread/loaded/list`. | Installed 0.162.1 and matching proxy source. |
| V5 | Each client's title contained the first 29 characters of its hook `session_id`, followed by `...`. A progress spinner sometimes followed that token. | Installed 0.162.1, two clients, corroborated by title source. |

For client B, hooks retained client A's `TMUX_PANE`, while the title identified
B's thread in B's pane. The daemon was still a descendant of A's TUI. This is a
direct counterexample to treating hook ancestry as proof of pane ownership.

A further read-only transport check used five fresh connections to the active
daemon, each performing WebSocket upgrade, initialize, and one loaded-list
request. With five loaded IDs, elapsed times were 0.66-0.87 ms, median 0.72 ms.
This establishes local feasibility, not a latency guarantee under load. The
query did not read transcripts, subscribe to threads, or mutate thread state.

A separate isolated turn then performed that same query from inside each of
the five hooks, while the daemon awaited the hook response. SessionStart,
UserPromptSubmit, PermissionRequest, PostToolUse, and Stop all returned a null
cursor and included the event's full session ID. Their measured query times
were 1.53, 1.17, 0.98, 0.96, and 1.18 ms respectively. Hook PPID identified
the temporary managed daemon throughout. The turn ran only `/usr/bin/true`;
the work directory remained empty. This verifies basic request reentrancy on
0.162.1, while leaving load and concurrency stress to implementation acceptance.
The initial diagnostic attempt used an unresolved overlength socket path and
failed before handshake; the successful run used the effective socket path.

### Corrections

1. **F1's historical boundary is too strong.**
   [Codex issue 48500](https://github.com/openai/codex/issues/48500)
   reports the same behavior on 0.157.1. This evaluation independently confirms
   0.162.1, not the first affected release. Do not advertise 0.160.0 as a proven
   introduction point.
2. **C4's unconditional self-healing claim is false.** `on_stop` checks session
   and turn identity before `set_agent_meta`. Permission and activity handlers
   reject conflicting nonempty context, but accept empty context: permission
   can seed partial agent metadata, and activity can append a log without a
   registered turn. Neither establishes a complete session/turn lifecycle.
   The daemon router therefore requires an established matching session and
   turn, a `title-v1` marker, and the selected client PID for Stop,
   PermissionRequest, and PostToolUse; only lifecycle starts
   may register a pane. A late-attached pane can miss
   an active turn until the next accepted lifecycle start.
3. **Candidate A's binding predicate is AND, not OR.** At danhper commit
   `06a47388dbdaf86c63c14f640535866cb32919bc`, `hook_binding_matches` requires
   both `registered_id == hook_id` and a matching title prefix. Its push path
   therefore depends on pull-created full-ID registration.
4. **The cited patch size is inaccurate.** GitHub reports 1,282 additions and
   47 deletions, across 18 files, for that commit. Some changes are lockfile
   churn; this is not a measure of routing logic alone.
5. **Candidate B's collision description is incorrect.** The 29-character
   prefix contains 25 hexadecimal digits, including UUIDv7 timestamp, version,
   and variant structure. The quoted `2^-116` is unsupported. Two different
   thread identities sharing a prefix would produce incorrect cross-thread
   writes; that is not benign fan-out.
6. **H's dependency-free reuse assumption fails on this build.** The proxy
   copies bytes between stdio and the Unix stream. It does not perform the
   WebSocket handshake or frame conversion required by the control socket.
7. **The ID token need not end the title.** Codex appends progress after the
   identity. A parser requiring a final UUID token would drop valid hooks.

The fact that a feature request is closed is not evidence that it shipped.
Issue 48880's inspected timeline contains a closure without a linked implementing
commit. The installed interface remains the deciding evidence.

## 3. What other implementations establish

| Primary source | Relevant behavior | Consequence here |
|---|---|---|
| Codex maintainer replies on issues 48500 and 44774 | Hooks run in the server; threads can have zero or multiple clients. Steering also makes turn-origin context insufficient for current terminal presentation. | Model binding as a set of currently displaying clients, rather than a single launching pane. |
| danhper's sidebar commit | Combines title binding with loaded-thread metadata and periodic status reads. | Reuse its demonstrated title/Unix WebSocket approach, but preserve this fork's lifecycle ownership and remove the registration dependency. |
| workmux at `0b741a9` | Supports explicit backend/instance/pane targets and validates persistent session bindings with process and server identity. Its fallback resolver first accepts the current pane from environment. | Explicit instance identity is useful. Its environment/ancestry fallback cannot establish daemon-client selection in this fork. Do not infer support from a README description of cwd matching. |
| OpenCode merged PR 42957 | TUI route activation submits terminal variables; the server stores one environment map per session. | Client-side context publication is an available architectural pattern. A single map replaced on each update does not itself represent several clients attached to the same session. |

The transferable rule is to keep terminal presentation identity at the client
boundary and declare the supported topology. None of these sources establishes
that daemon-loaded thread IDs reveal which thread a given pane displays.

## 4. Routing and lifecycle contract

### Supported topology

Support local Codex TUIs using an explicitly configured control socket and tmux
server. Several panes may display one thread; route to all qualifying panes.
Several threads may share cwd; cwd never participates in binding. A headless or
desktop-only thread with no matching pane receives no pane writes.

Remote daemon clients, multiple unresolved Codex homes, and panes with competing
terminal-title writers are not silently inferred. Missing or conflicting
identity means no write. These deployments need explicit endpoints and a usable
client selection signal before they are supported.

### Parse and select

1. Parse the Codex payload before any `TMUX_PANE` early exit. Obtain session and
   turn context through normalized `AgentEvent`, not raw JSON in handlers.
   A missing or malformed full session UUID is unroutable.
2. Scan the explicitly pinned tmux server once for pane ID, pane PID, title,
   current command, and existing session/turn metadata. Escape title separators
   through tmux format quoting; never concatenate raw titles into commands.
3. Take one process snapshot. A candidate must contain an interactive Codex
   client outside daemon/exec subtrees. Bare `tree_has_agent` is insufficient:
   an `app-server`, `exec`, or a tool-launched Codex process can match its basename.
   Keep this qualification in the Codex leaf and test installed argv shapes.
4. Accept exactly one identity token from the configured title identity segment:
   a full canonical UUID, or the installed 29-character UUID prefix plus `...`.
   Validate positions of hex digits and hyphens. Permit a progress suffix.
   Reject shorter prefixes, multiple identity tokens, and malformed tokens.
5. A full title UUID must equal the event UUID. A truncated title must resolve
   uniquely to that UUID in the complete loaded-ID snapshot below. Require the
   event ID to be present. An old `@pane_session_id` cannot override a different
   observed title.
6. Deduplicate pane IDs and process targets in stable pane-ID order. Multiple
   panes matching one full ID are legitimate fan-out. Multiple full IDs matching
   one prefix are ambiguity and must be rejected.

Configure `[tui] terminal_title = ["thread-id"]` for the simplest parseable title.
Existing title preferences may be retained with `thread-id` as the final
configured item; the progress suffix still remains valid. Other title shapes
must pass the same single-token rule rather than receiving heuristic matching.

### Bounded ID lookup

Use one fresh Unix WebSocket connection per hook invocation that has truncated
candidate titles. Perform the existing initialize handshake, then request
`thread/loaded/list` with empty params. Do not request thread status or history.
Resolve the configured socket path at connection time. Codex can publish a long
control-socket path as a symlink to a shorter socket under its daemon directory;
its Unix client canonicalizes before retrying an overlength address. A plain
UnixStream connection to the unexpanded long path is insufficient. Keep setup
independent of socket existence and fail closed if runtime resolution fails.
Use `tungstenite = "=0.29.0"` with default features disabled and `handshake`
enabled, matching the inspected working reference. Review its dependency graph
when implementing; the design adds neither a TLS stack nor an async runtime.

On pinned 0.162.1 source, omitted `limit` defaults to the number of loaded
catalog entries and returns all exposed IDs from one collected vector.
`ThreadManagerState::list_thread_ids` excludes `SessionSource::Internal`
threads; this is the API's complete catalog, not every internal manager entry.
An event absent from that catalog cannot use truncated-title routing.
This removes the need
for paginated enumeration and a long-lived cache. Require `nextCursor == null`.
If a future build paginates this request, reject the snapshot and surface a
compatibility diagnostic rather than treating a partial page as complete.

Use a 250 ms transport wall-clock budget for the ID lookup, a 1 MiB message bound,
and at most 16,384
validated IDs. These are proposed operational limits, not measured guarantees.
Bound connect, handshake, reads, and writes within that lookup deadline.
Synchronous filesystem canonicalization and OS spawn/reap do not provide hard
real-time deadlines; arbitrary filesystem/kernel stalls remain outside this
application-level bound.
Give added routing work a one-second aggregate budget, including bounded ps/tmux
reads and 50 ms per-pane lock attempts. Stop attempting remaining targets when
that budget is exhausted. This bounds the added router work; existing handler
execution and generic tmux helpers do not currently have an end-to-end deadline.
Do not claim that this change alone bounds the entire hook invocation.
Match JSON-RPC response IDs, handle control frames
through the WebSocket library, bound unsolicited messages, and reject RPC
errors, missing results, malformed IDs, and incomplete frames. Close after use.
Socket failures and ambiguity cause no writes to truncated candidates. Exact
full-ID candidates can still route without this lookup.

The snapshot resolves a prefix among loaded threads at observation time.
It cannot detect an unloaded thread whose stale title happens to have the same
prefix, nor freeze concurrent selection changes. It is a useful ambiguity check,
not a replacement for a full-ID client binding. No probability calculation is
used to justify writing both colliding threads.

### Serialize and revalidate

Acquire one pane lock at a time. Key it by canonical tmux socket identity and
pane ID, not the entire `TMUX` value: server PID/client-index suffixes must not
split locks for the same server. Preserve the existing private-directory,
ownership, mode, and `O_NOFOLLOW` protections. Release before the next pane.
Use bounded nonblocking lock attempts so a stuck sibling hook cannot stall
model work indefinitely.

After taking the lock, reread title, pane PID, and stored session/turn context.
Confirm the same pane process and binding; skip if anything changed. Avoid
holding the lock during the daemon query. Process exit or pane reuse skips dispatch. Read/lock/transport failures use
stderr diagnostics. Existing generic option setters discard command failures;
this implementation does not claim to report every partial write, and it only
records provenance after rereading the established session.

A same-session SessionStart must not clear a turn already established by
UserPromptSubmit. Keep the generic handler intact; apply this Codex dispatch
guard before it. UserPromptSubmit remains the event that establishes a new
turn. Before dispatching daemon-routed Stop or permission/activity events,
require an established matching session and a nonempty matching turn, plus a `title-v1` routing marker and
the selected client PID. Old unmarked metadata and a previous client's metadata
must wait for an accepted lifecycle start. This
additional router guard prevents permission from seeding partial metadata or
activity from writing an unregistered log. Existing handler guards still
reject stale and duplicate completion events. Legacy dispatch remains unchanged.

Title observation and tmux option updates have no atomic transaction with the
Codex TUI. Revalidation narrows the race; the refresh rejection below prevents
a mismatched projection from entering the review lifecycle. This is eventual
safety at the sampled binding boundary, not a linearizable write guarantee.

### Hook protocol and failures

Detach Stop's protocol response from the event before fan-out. Call handlers
with `response = None`, then emit the saved response once from the hook entry
point. Do this even with zero matches, rejected events, a socket outage, or
lock failure. Routing diagnostics go to stderr. A sidebar routing outage is
observational and must not block the Codex turn; Codex routing failures exit
successfully after the protocol response. Other agents keep their existing
exit behavior.

Activity filenames now include the canonical tmux server hash used by locks.
All readers, writers, and cleanup use that same namespace. Existing unscoped
logs are left untouched and are not imported into an unidentified server.

Pane-local activity and existing notification policy apply independently to
each accepted pane. Stop stdout deduplication does not imply desktop notification
deduplication. Two displaying panes may receive two existing pane-scoped
notifications; changing that policy is outside this fix.

## 5. Stale state, startup, and switching

Add title identity to the existing bulk query. Only panes marked
`@pane_codex_routing=title-v1` participate in the new title check; keep legacy
process cleanup for unmarked panes. Record the chosen TUI PID in
`@pane_codex_client_pid` and require that same unique client. Before exposing
a marked Codex pane to review transitions, check that its title still matches its stored full
session ID and that a qualifying TUI remains alive. Hide a mismatched projection
immediately. Reuse the existing stale-agent cleanup path, with the same pane
lock and a fresh comparison before clearing durable fields and activity.
Do not clear a new binding installed after the query snapshot.

Run cleanup in the query/refresh ownership seam, not in UI rendering. Keep the
parser's new binding check pure; return cleanup candidates to orchestration.
The current query already has stale-process cleanup side effects, so this
integration must fit that path rather than introduce another tmux query loop.
Cleanup must clear session, turn, completed turn, prompt, wait state, attention,
subagents, location metadata, and activity through the owning existing helpers.

On A:X -> Y -> X, a detected mismatch invalidates X's old pane projection.
Returning to X must not resurrect a parked response as a freshly completed
turn. If switching away and back occurs entirely between observations, the
sidebar cannot detect that interval; it must not claim otherwise.

An idle startup before the first hook, a switch without a prompt, or attachment
mid-turn can leave a pane without trustworthy lifecycle metadata. Display no
fabricated running/ready state. The next accepted SessionStart or
UserPromptSubmit can establish it. A late Stop must not bypass guards to make
the pane visible. This deliberate coverage limit is materially different from
A's status reconciliation and must appear in the rollout notes.

A future requirement to show every selected idle thread or recover in-flight
turns needs client binding plus explicit lifecycle reconciliation. That is a
separate feature with rules for turn provenance and response review state,
not a reason to overwrite review states with daemon status in this fix.

## 6. Configuration, notices, and server scope

Keep `setup codex` a pure generator. Extend its explicit inputs with
`--tmux-socket <absolute-path>` and `--codex-control-socket <absolute-path>`.
Print the title snippet and generated hook commands; do not edit user TOML,
hook trust records, or hooks.json. Endpoints must come from explicit inputs,
not discovery using the daemon's environment. Reject relative paths and tmux
socket paths containing commas, which conflict with `TMUX` encoding.

Generated commands pin `TMUX` to the selected socket with a stable suffix and
set a dedicated `SIDEBAR_CODEX_CONTROL_SOCKET` variable for the router. Use the
existing shell-quoting helper for every literal. A separate isolated check
verified that `TMUX=<socket>,0,0` directs the installed tmux client to that
socket. Scope these variables to the hook subprocess; do not mutate Rust's
global process environment. Require the explicit control-socket override for
daemon routing instead of guessing a different Codex home.

The generator needs endpoint inputs but still does not read user configuration.
Hook drift detection must compare the configured socket with the sidebar's
server and understand the generated command prefix without evaluating shell
code. Keep registration trigger/matcher ownership in `HOOK_REGISTRATIONS`.

For several tmux servers sharing one daemon, merge a separate generated command
for each server into each hook registration. Each invocation scans only its
configured server and emits at most one response for that invocation. Keep
these entries distinct when merging; do not replace server A's entries with B's.
No global socket-directory scan or active-server fallback is permitted.

Include prerequisite feedback in the existing notices surface. Check hook
command endpoint configuration at startup for explicit scoped deployments;
otherwise show daemon prerequisites when a qualified daemon-mode client is
observed. Intentional `--no-daemon` clients do not require titles or endpoints. Derive an unavailable-title notice
from the same bulk pane/process observations, including live daemon-mode Codex clients
that lack agent options; do not infer TOML validity with substring matching.
Allow a two-second startup grace and dismiss the notice when a usable title
appears. Say that the thread title is unavailable and offer the setup snippet;
do not assert which config file setting caused the runtime observation.
If endpoint configuration is absent, show the notice and perform no daemon
routing. Transient socket failures remain stderr diagnostics, not a new polling
status subsystem.

Bare ancestry fallback is excluded. Legacy hooks remain operational only when
the hook's nearest Codex ancestor is the pane's unique qualified interactive
client outside daemon and exec subtrees. Repeat this proof after locking; a
changed session/turn/completed snapshot cancels dispatch. An accepted legacy
lifecycle start clears prior title-routing provenance. This narrowly proven
path preserves the explicit `--no-daemon` workaround without trusting the
shared daemon's inherited pane. Unknown argument forms fail closed.

## 7. Integration outline

The change spans more than five files and several hotspots. Implement under the
linked decision record as a focused bug-fix change, without upstream cleanup.

| Location | Planned change |
|---|---|
| `src/cli/hook.rs` | Codex-specific parse-before-routing dispatch, response extraction/emission, per-pane revalidation and SessionStart guard. Preserve the other-agent path. |
| `src/cli/hook/codex_routing.rs` and `codex_routing/transport.rs` | Endpoint validation, pure target selection, bounded ID-only WebSocket client, routing and protocol dispatch. |
| `src/adapter/codex/identity.rs` | Shared pure title grammar, qualified client discovery, and proven legacy ancestry; no daemon status authority. |
| `src/pane_lock.rs` and `src/activity.rs` | Preserve lock protections; normalize socket identity; allow stale cleanup to use the same lock; bounded acquisition and shared server-scoped activity filenames. |
| Normalized event access in routing leaf | Existing event variants provide session context; no event schema change or raw-payload path. |
| `src/process.rs` | Expose only any process data needed by the leaf; keep Codex policy out of generic process matching. |
| `src/tmux/query.rs` and query result/types | Add escaped title identity to the one bulk query and return stale-binding cleanup candidates. Keep field indexes synchronized. |
| `src/state/refresh.rs` | Filter invalid bindings before review transitions; locked, revalidated cleanup through the existing ownership seam. |
| `src/cli/setup/mod.rs` | Explicit endpoint inputs, generated environment prefix and title guidance, drift comparison for multiple scoped registrations. |
| `src/state/notices.rs` and existing notice renderer | Endpoint and observed-title prerequisites, with existing popup/copy interaction. |
| `Cargo.toml`, `Cargo.lock` | One justified WebSocket dependency, pinned using repository convention; disable unneeded TLS features. |
| State/setup/user documentation | Endpoint scope, title prerequisite, lifecycle coverage limits, setup re-paste requirement. |

Do not change `hook.sh` for routing policy. It already forwards arguments and
inherits command-scoped variables. Do not share the quota app-server subprocess
with the router: it has different transport and timeout requirements.

Implement in three reviewable parts: routing/protocol tests and leaf integration;
stale projection/lock integration; setup/notices and isolated acceptance checks.
All parts are required for the proposed fix to be operable.

## 8. Regression and acceptance plan

### Deterministic unit and fixture coverage

- Full UUID; installed truncated UUID; spinner suffix; final configured title
  segment; invalid hyphens; too-short prefix; multiple tokens; delimiter escaping.
- Two same-cwd threads route independently. An existing session option cannot
  override a different title. Several panes displaying one thread all receive it.
- Two distinct full IDs with the same title prefix cause zero writes. Missing
  event ID, non-null next cursor, duplicate/malformed IDs, and oversized results
  fail closed. Duplicate returned IDs are invalid snapshot data, not extra clients.
- A daemon launched below pane A with a hook for pane B never enables the
  inherited-pane fallback. Daemon-only and exec-only trees are rejected.
- Fake Unix WebSocket server verifies initialize and exactly one unpaginated
  loaded-list call. Exercise control frames, unrelated RPC messages, disconnect,
  invalid upgrade, RPC error, excluded internal IDs, frame limits, timeout,
  long-path symlink resolution, bounded child-process cleanup, and no stdout leakage.
- Rebind/exit/PID change between scan and lock causes no event writes. Cleanup
  rereads the binding and cannot erase a newer hook's metadata. Locks for the
  same socket with different `TMUX` suffixes coincide; different servers differ.
- Stop response appears once for zero, one, and multiple panes, including
  transport error, lock failure, and lifecycle rejection. Assert exit behavior.
- Delayed SessionStart cannot erase a newer turn. Missing SessionStart followed
  by UserPromptSubmit recovers; followed only by Stop/PermissionRequest/activity
  is rejected by the daemon router when lifecycle context is missing. Do not
  weaken stale completion tests or change generic legacy handler behavior.
- Focus still changes response_ready to response_reviewing, then leaving parks
  it. A daemon inventory lookup never changes those states. Switching invalidates
  prompt/activity/wait metadata before review logic can consume it.
- Setup quotes hostile literal paths, supports several explicit server commands,
  performs no config writes, and flags missing/wrong endpoints. Existing Claude,
  Kimi, and adapter drift tests remain valid.
- Notice startup grace, disappearance after title publication, and full inline
  Ratatui snapshots. No substring-only assertions for rendered frames.

Use `tmux::test_mock` and fake transport inputs for unit tests. Never connect
unit tests to the developer's live server or daemon.

### Isolated real-tmux and daemon acceptance

Use dedicated sockets and a temporary Codex home. Trust only temporary hooks
through the supported trust flow. Do not use the hook-trust bypass as evidence
of shared-daemon behavior: the earlier diagnostic run with that flag selected
embedded execution and was excluded from daemon evidence.

1. Two new clients, same cwd, different threads; daemon started by A. Verify
   B's five event types write only B despite inherited A identity.
2. Two panes attached to the same thread; verify fan-out and per-invocation
   Stop response. Attach a third pane mid-turn and verify the documented
   coverage limit rather than inventing lifecycle metadata.
3. Switch A:X -> Y -> X without prompts. Verify mismatch rejection and cleanup,
   then submit a fresh prompt and check recovery. Exercise a late X Stop while
   Y is visible.
4. Two isolated tmux servers with colliding pane numbers and one daemon.
   Configure one command per server; verify scope, including when the daemon
   was launched outside either target server.
5. Restart the temporary daemon, remove its socket, replace a pane process,
   suppress title identity, and test bounded failure with no inherited writes.
6. Exercise response review, permissions, activity, and duplicate/late Stop
   guards under the new router. Check that other agent adapters still work.
7. Run the ID lookup from inside every temporary hook while the daemon awaits
   that hook's response. Verify that request dispatch remains available and
   resolves the current event ID within the deadline. An idle-daemon latency
   probe does not establish this reentrancy property. The basic five-event
   check passed during design research; repeat it with concurrent clients and
   the Rust implementation. Treat failure as a release blocker for this design,
   not a reason to use unchecked prefix routing.

Multi-attach, resume/switch timing, and two-server daemon behavior are required
implementation acceptance cases. They have not been claimed as live-verified
by this design-only investigation.

After implementation, run targeted tests and `./scripts/verify.sh full`, then
inspect `./scripts/fork-delta.sh`. Update state documentation and setup guidance.
Re-paste the new `setup codex` output into the user's hook configuration and
merge the title setting; command changes may require fresh Codex hook trust.
Relaunch TUIs to verify the effective title configuration. Rust changes need
the release build; changing adapter/setup output alone does not update the
user's hooks. No Claude plugin update is needed if its staged files stay unchanged.

## Implementation verification

Independent subagent review approved the architecture conditionally and found
mandatory provenance gating and cross-server activity isolation. Both were
implemented. A second independent code review identified missing post-lock
legacy ancestry checks, stale marker migration, and lifecycle snapshot races;
these were fixed and covered before handoff. It also prompted unique-client
revalidation and legacy notice exemptions.
Final source readback corrected the empty-context permission/activity assumption
and added daemon-only session/turn prerequisites before handler dispatch, with
regression coverage for missing and conflicting context. A final independent
migration review additionally required the routing marker and selected client
PID: matching legacy session/turn metadata alone cannot authorize daemon
observations. Unit and isolated tmux tests cover missing markers and stale PIDs.
Negative session/turn fixtures carry valid routing provenance so they exercise
the intended guard independently. Missing and unknown markers, and missing,
wrong, or malformed client PIDs have separate cases. Temporarily removing the
router guard caused both negative unit tests to fail; restoring it passed all
eight routing unit tests.

`./scripts/verify.sh full` passed on the final production code: format check,
strict Clippy, isolated unit/integration suites, real-tmux lifecycle tests,
twelve Codex routing tests (nine scenarios plus guarded subprocess workers),
and release build. `./scripts/fork-delta.sh` and `git diff --check` were inspected.
No tracked capture PNGs were edited. The notices popup was captured from an
isolated server and its generated image inspected for clipping and control
alignment. Existing capture fixtures now obtain the scoped activity namespace
from the owning writer instead of assuming the old filename.

The built release binary also passed isolated live acceptance on Codex 0.162.1:

- Two same-cwd TUI clients shared a temporary daemon whose inherited pane was
  A. B's first lifecycle changed only B; A subsequently acquired its own full
  session and turn metadata.
- SessionStart, UserPromptSubmit, PermissionRequest, PostToolUse, and Stop paths
  were exercised. Permission waiting changed B to `waiting/permission`, while
  A retained its completed response. Only harmless `/usr/bin/true` was approved.
- Both clients completed replies. B resumed with a new client PID, and routing
  recorded that replacement PID without modifying A's turn.
- The temporary daemon was stopped and verified dead; the isolated session and
  credential home were removed. The temporary work directory remained empty.

This does not constitute an exhaustive live stress test. Simultaneous attachment
to one real thread, selection races, mid-turn attachment, and two real tmux
servers sharing one daemon remain live release-matrix cases; fan-out,
server separation, switching cleanup, timeout, collisions, and lock contention
have deterministic isolated coverage. The asynchronous-title and missing-hook
coverage limits above still apply. The initial isolated verification made no
user configuration changes. Subsequent local deployment persisted title settings
and updated hook trust through cc-switch common configuration, replaced the five
legacy sidebar commands, and revalidated the deployed daemon route. Provider
settings and unrelated live configuration were preserved.

## 9. Evidence sources and reproduction

Primary sources inspected on 2026-10-11:

- [Codex title identity and truncation](https://github.com/openai/codex/blob/092d3acd6bec3e3a14bdc7e7a2810ab628ab759d/codex-rs/tui/src/chatwidget/status_surfaces.rs)
- [Codex loaded-list implementation](https://github.com/openai/codex/blob/092d3acd6bec3e3a14bdc7e7a2810ab628ab759d/codex-rs/app-server/src/request_processors/thread_processor.rs),
  [catalog filtering](https://github.com/openai/codex/blob/092d3acd6bec3e3a14bdc7e7a2810ab628ab759d/codex-rs/core/src/thread_manager.rs)
- [Codex raw stdio-to-Unix relay](https://github.com/openai/codex/blob/092d3acd6bec3e3a14bdc7e7a2810ab628ab759d/codex-rs/stdio-to-uds/src/lib.rs)
- [Codex Unix client path resolution](https://github.com/openai/codex/blob/092d3acd6bec3e3a14bdc7e7a2810ab628ab759d/codex-rs/uds/src/lib.rs)
- [Codex pending SessionStart execution](https://github.com/openai/codex/blob/092d3acd6bec3e3a14bdc7e7a2810ab628ab759d/codex-rs/core/src/hook_runtime.rs)
- [Codex UUIDv7 generation](https://github.com/openai/codex/blob/092d3acd6bec3e3a14bdc7e7a2810ab628ab759d/codex-rs/protocol/src/thread_id.rs)
- [Maintainer explanation of hook/client separation](https://github.com/openai/codex/issues/48500#issuecomment-5878557125)
- [Maintainer explanation of steering and terminal context](https://github.com/openai/codex/issues/44774#issuecomment-5641955379)
- [Displayed-thread signal request](https://github.com/openai/codex/issues/48880)
- [Hook-scoped client environment proposal](https://github.com/openai/codex/issues/44902)
- [danhper's complete routing commit](https://github.com/danhper/tmux-agent-sidebar/commit/06a47388dbdaf86c63c14f640535866cb32919bc)
- [workmux exact status resolution](https://github.com/raine/workmux/blob/0b741a985c1b879b76c4b2520d3b74af0d24bb93/src/command/set_window_status.rs)
- [OpenCode terminal environment PR](https://github.com/anomalyco/opencode/pull/42957),
  [merged session environment map](https://github.com/anomalyco/opencode/blob/42fc297d30c3bdffb442b09cac512a479c9e3bee/packages/core/src/session/environment.ts),
  [merged TUI publication](https://github.com/anomalyco/opencode/blob/42fc297d30c3bdffb442b09cac512a479c9e3bee/packages/tui/src/app.tsx)

To repeat source inspection, fetch the pinned Codex tag and danhper commit,
then inspect the linked functions. To repeat V1-V5, use the isolated acceptance
procedure above and a temporary logger recording field names and IDs rather
than event contents. Confirm hook PPID is the managed daemon and confirm that
two clients share that PID before accepting the run as daemon evidence.

For the ID-only probe, connect to the explicitly selected Unix socket using a
WebSocket client, complete initialize/initialized, and send
`{"id":2,"method":"thread/loaded/list","params":{}}`. Check the matching
response and null `nextCursor`; do not invoke start/resume/subscribe or read
thread history. The temporary diagnostic frame implementation is not proposed
as product code. Reproduce transport behavior against the installed version
before adapting this design to a different Codex release.
