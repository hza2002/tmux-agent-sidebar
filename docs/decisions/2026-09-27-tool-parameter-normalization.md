# Decision: Agent tool-name and argument normalisation

## Context

The Activity block was built for Claude Code and inherited from upstream
unchanged: `src/cli/label.rs` and `src/tool_name.rs` had no fork edits, and the
label strategy table reads exactly one Claude-style field per tool. Three
adapters were added on top of that table — Codex, Kimi, OpenCode — and only
OpenCode normalised anything (a single `filePath` → `file_path` rewrite).

A full audit of all four agents' tool vocabularies against real payloads found
the table was reading names and keys that three of the four agents never send.
Counted from the live activity logs on this machine:

| Pane | Agent | Entries | Blank labels | Blank tools |
| --- | --- | --- | --- | --- |
| `%253` | codex | 134 | 35 | `webrun` 20/20, `apply_patch` 13/13, `request_user_input_async` 2/2 |
| `%266` | kimi | 76 | 63 | `Read` 61/61, `TodoList` 2/2 |
| `%232` / `%268` | claude | 131 / 58 | 0 | — |

The counts are a snapshot of the live logs at audit time; the log keeps 200
lines, so older rows rotate out as an agent keeps working.

Verified causes, each from the agent's own binary, schema, or recorded payload:

- **Kimi** spells the file argument `path` (`Read`/`Write`/`Edit`) and names
  several tools differently: `FetchURL` → `WebFetch`, `ReadMediaFile` → `Read`,
  `TodoList` → `TodoWrite`, `AgentSwarm` → `Agent` (a fan-out tool, so the row
  loses that distinction — one call observed against 43 `Agent` calls), plus
  `WaitFor`, `TaskList`, and the goal tools, which stay unmapped and take the
  fallback. Its response key is `tool_output`, not `tool_response`.
- **Codex** rewrites `exec_command` → `Bash` and `cmd` → `command` itself, then
  reports every other tool under its own snake_case name with untyped
  `tool_input` (`apply_patch`, `webrun`, `request_user_input_async`,
  `view_image`). Because the installed `hooks.json` has an empty matcher, all of
  them reach the sidebar; the previous comment claiming `PostToolUse` fired for
  Bash only was wrong.
- **OpenCode** passes its lowercase ids and raw `args` through the JS bridge
  untouched; `question`, `patch`/`apply_patch`, `list`, `plan_exit` and `skill`'s
  `name` argument had no mapping.
- **Claude Code** itself had two dead rows: `ExitWorktree` was mapped to `name`,
  a field the tool does not have (it reports `action`), and `TodoWrite` is the
  one canonical tool with no strategy row at all.
- **`label_agent`** preferred `response.content[].text`, which no longer exists
  in Claude's Agent response (0 of 160 sampled calls), so the branch was dead
  there and every row fell back to `description`. Kimi never reached it at all —
  its response key is `tool_output`, which the adapter does not read — and that
  is also why mapping that key is not worth doing: the value is a 2000-character
  blob.
- Anything unmapped — every MCP tool, and any tool a future agent ships — wrote
  an empty label, which the renderer shows as a bare tool name truncated to 12
  columns (`mcp__plugin…`).

## Chosen Seam

- **Normalisation stays at the adapter boundary**, as
  [`architecture-map.md`](../maintainers/architecture-map.md) requires. Tool
  *names* are fully canonicalised there. Argument *keys* are canonicalised
  wherever a single top-level rename suffices (`alias_keys`); the extractors
  still read a few per-agent key spellings directly (`patchText`, the nested
  `action.queries`, a question's `title`, todo item keys) and the shared
  fallback key list carries `filePath`/`cmd`, because those shapes are nested or
  unconfirmed and a rename cannot reach them. The boundary claim is exact for
  names and "best effort, documented here" for keys.
- **Two shared helpers, three local tables.** `src/adapter/mod.rs` gains
  `canonical_tool_name(aliases, raw)` and `alias_keys(input, pairs)`
  (the `copy_keys` helper OpenCode already had, generalised). Each adapter owns
  a `TOOL_ALIASES` table and a `tool_arg_aliases(canonical_name)` function.
  Aliasing runs before key rewriting, so `ReadMediaFile` → `Read` inherits the
  `path` → `file_path` rewrite for free. Keys are added alongside the originals;
  a destination that already holds a non-empty string wins, an empty or null one
  does not block the alias. `tool_arg_aliases` keys off
  `CanonicalTool::from_name`, so renaming a canonical spelling cannot silently
  disable an alias. A fourth helper, `tool_input_value`, parses a `tool_input`
  that arrives as a stringified JSON object — Claude sent both shapes already
  and the other agents' shapes are not fully observable — while leaving raw text
  (a patch document, `true`, `ls -la`) untouched. Claude's own
  `parse_json_field` now delegates to it: it used to drop a non-JSON
  `tool_input` to `Null`, which silently discarded a payload the extractors can
  read (found by running a bare patch document through the release binary).
- **The vocabulary grows only where a label or state is worth having.**
  `CanonicalTool` gains `Patch` (a whole patch document instead of an edit pair),
  `EnterPlanMode`, and `ExitPlanMode`. The plan-mode badge in
  `src/cli/hook/activity.rs` now matches those constants instead of raw string
  literals. Only Claude panes *display* the badge (`tmux::query` reads
  `@pane_permission_mode` for Claude alone), so for OpenCode's `plan_exit` the
  observable effect is the Activity row (a canonical name with the command
  colour and no label, instead of a gray row printing `{}`); the option write
  keeps the value consistent for anything else that reads it.
- **`Patch` reads the patch document, and only its own dialect.**
  `*** Update File:` / `*** Add File:` / `*** Delete File:` headers name the
  files; the row shows the first basename and `+N` for the rest. Three payload
  shapes are accepted (the document as the argument, under `patchText`/`patch`,
  or a `changes[]` array of `{path}`) because that is what the agents actually
  send, plus a last-resort scan of any string value that carries the markers, so
  an unanticipated key spelling still yields a filename instead of a bare row.
  Files are counted by full path (`src/a/mod.rs` and `src/b/mod.rs` are two),
  collection stops at 64 files with a `+` marking the cap, and only the `***`
  dialect is parsed — guessing at other diff formats would put a wrong filename
  on the row.
- **`TodoWrite` shows count + active item** (`2 tasks · Adding a test`). The task
  *progress* band keeps reading `TaskCreate`/`TaskUpdate` labels; those two
  formats are unchanged on purpose, and the strategy table's comment records it.
- **A tool with no strategy row falls back**: first scalar value under a fixed
  candidate key list (`description`, `query`, `path`, `command`, `url`, …),
  else the payload's own key list (`{app,x,y}`). A *mapped* tool owns its label
  including an empty one, so an empty `Read` does not print `{file_path}`, and a
  new `LabelStrategy::Empty` marks the tools that are deliberately blank
  (plan-mode transitions, which carry a document, not an identifier). Every
  fallback branch is capped at 160 characters — the key list included, since key
  names are payload content. The key list is the self-documenting half, and it
  fires only when none of the candidate names is present: a payload carrying
  `description`, `id`, or `text` shows that value instead, so the shape hint is
  for genuinely unfamiliar payloads rather than every unmapped tool.
- **`label_agent` leads with `description`.** It is short, present in all four
  adapters, and stable; the response text stays as the fallback for a call whose
  description is missing. This reverses the 2026-09-18 intent (show what came
  back) because the payload behind that intent no longer exists for Claude and
  is a blob for Kimi.
- **Codex's `webrun` reads both query shapes** (`query`, and the model-facing
  `action.queries[0]`), because its hook spelling is not observable — see the
  open items.
- **`label_ask_user_question` accepts `question`, `title`, or `header`** as the
  prompt field, since Codex's `request_user_input_async` uses `title`.
- **MCP names are shortened for display, not for storage.** The log keeps the
  full `mcp__<server>__<tool>` name (it is the tool's identity); the row keeps
  the longest `__`-aligned suffix that fits the 12-cell column. A short server
  stays visible (`mcp__evil__Read` reads as `evil__Read`, so a scoped tool can
  never be mistaken for the native `Read`), and only when the server is too long
  does the tool name stand alone (`mcp__plugin-kimi-cu_mac__click` → `click`,
  instead of `mcp__plugin…`). A server whose name is long enough to hide behind
  still renders as a bare tool name; the Network colour and the stored log line
  are what separate it then.
- **The task band's contract is tested, not just documented.**
  `activity::parse_task_progress` parses `TaskCreate` / `TaskUpdate` labels back
  out of the log, so `test_task_progress_parses_extractor_labels` feeds the
  parser the extractor's real output; a format change on either side fails there.
- **The colour classifier matches the parsed vocabulary.**
  `CanonicalTool::from_name` (with `ALL`) replaces the classifier's string
  literals, and `TaskList`, `TeamDelete`, `CronList`, `RemoteTrigger` become
  canonical variants — they are real Claude Code tools (each name verified in
  the installed 2.1.278 bundle), so the literals were not dead branches but
  vocabulary the enum had never absorbed. `Patch` and `TodoWrite` moved from
  gray to the edit class. The match has no catch-all arm, so once a variant is
  in `ALL` a new one fails the build until its colour is decided.
- **The new guards cover what the compiler cannot.** `ALL` stays
  hand-maintained: a variant missing from it renders gray with a fallback label,
  and no test can see the omission, so `every_canonical_tool_has_a_strategy_row`
  pins the strategy side and each adapter's
  `every_alias_target_is_in_the_canonical_vocabulary` fails if an alias points
  at a variant `from_name` cannot parse. The decision not to generate the enum
  from a macro keeps this file mergeable against upstream.

## Measured Effect

Every mapping is covered end to end — adapter parse → normalised event → label,
with the response the production hook passes — in the adapters' own tests, and
the release binary was run through the production hook path
(`hook <agent> activity-log` with real payload shapes):

```text
# Kimi payloads
15:20|Read|STATUS.md                  # tool_input.path → file_path
15:20|WebFetch|example.com/docs        # FetchURL, url unchanged
15:20|TodoWrite|9 tasks · Injection-revert experiments   # TodoList, real item shape
# Codex payloads
15:20|Patch|spec.md                   # apply_patch, patch document as the arg
15:20|WebSearch|site:clash-verge-rev tun  # webrun, search_query[0].q
15:20|AskUserQuestion|Which DB?       # request_user_input_async, title
# OpenCode payloads
15:20|AskUserQuestion|Ship it?        # question
15:20|Patch|new.md                    # apply_patch, patchText
# Claude Code payloads
15:20|ExitWorktree|keep               # action, not name
15:20|ExitPlanMode|                   # deliberately blank
15:20|mcp__plugin-kimi-cu_mac__click|{app,x,y}   # no mapping: key list
```

## Alternatives Rejected

- **Three copies of the OpenCode normaliser.** Copying `copy_keys` and the
  matching logic into each adapter would have been the smaller diff per file and
  the larger one overall; the helpers are shared instead.
- **A macro-generated enum.** Deriving `CanonicalTool`, `as_str`, and `ALL` from
  one list would make the drift hole in finding three impossible, but it
  restructures a file that was byte-identical to upstream until this change, and
  the failure mode it removes is a gray row, not a wrong one. The two new guard
  tests cover the reachable half instead.
- **Mapping `todoread`.** OpenCode's read-only todo tool carries no arguments;
  mapping it to `TodoWrite` would invent a label it cannot have. It falls to the
  fallback like any other unmapped tool.
- **Stripping control characters in `sanitize_activity_label`.** The diff widens
  which payload fields reach a label, but the sidebar's own terminal is
  protected (ratatui drops graphemes containing control characters), and
  stripping escapes would break the documented contract that a shell command
  reaches the block exactly as the agent ran it. Left as an open item for the
  log file and clipboard surfaces.
- **Renaming at the OpenCode JS bridge.** The bridge would need a second copy of
  the mapping, and a bridge edit needs a reinstall; the Rust adapter is the
  single boundary for all four agents.
- **Preferring the subagent's response text.** Reversed here: Claude no longer
  sends one, Kimi's is a 2000-character blob, and OpenCode's title is the only
  short form — one rule that works for one agent is not a rule.
- **Always running the fallback.** It printed `{questions}` for an empty question
  array and a plan document's shape for `ExitPlanMode`; mapped tools own their
  labels instead.
- **Parsing every diff dialect** (`+++ b/…` and friends) for `Patch`: no agent
  sends one, and a wrong filename on the row is worse than a blank label.
- **A canonical variant for the whole long tail** (`ScheduleWakeup`,
  `SendFeedback`, `view_image`, goal tools): the fallback already gives those
  rows a readable label without a per-tool decision.

## Upstream Compatibility

- `src/cli/label.rs` and `src/tool_name.rs` diverge from upstream for the first
  time; the divergence is additive (new variants, new strategies, one corrected
  key) plus the fallback and the extractor tolerances.
- `src/adapter/opencode.rs` **replaces** upstream's `normalize_tool_name` /
  `normalize_tool_input` / `copy_keys` with the shared helpers and the adapter's
  own tables. A conflict resolution that takes upstream's hunk restores the old
  normaliser and leaves `TOOL_ALIASES` unused — names then pass through raw,
  rows go blank, and upstream's suite does not notice. Take this file's version.
- `src/adapter/` is a skeleton touchpoint. The change is confined to the
  boundary the architecture map already designates for normalisation: two
  helpers in `mod.rs`, one alias table and one key-rewrite function per adapter,
  and no new event kind or handler.
- No `HOOK_REGISTRATIONS` change, so no agent config re-paste and no Claude Code
  plugin update is needed; the Rust release build is resolved by `hook.sh` on
  every fire.
- `src/cli/mod.rs` widens `mod label` to `pub(crate) mod label` so adapter tests
  can assert parse → label in one test rather than two half-tests.

## Conflict and Removal Strategy

- Removal: delete `canonical_tool_name` / `alias_keys` / `tool_input_value` and
  the three adapters' tables, restore the raw pass-through in each tool-carrying
  arm, drop `Patch` / `EnterPlanMode` / `ExitPlanMode` / `TaskList` /
  `CronList` / `RemoteTrigger` / `TeamDelete` and their rows, restore
  `ExitWorktree` → `name`, restore `parse_json_field`'s string-dropping body,
  and revert the response-first `label_agent`; delete the
  fallback, `LabelStrategy::Empty`, and the extractor tolerances, restore the
  classifier's string-literal match and `is_command_tool`'s literals, revert the
  renderer to the raw tool name (and the two snapshots in
  `tests/bottom_tests.rs`), drop the new guards in `src/activity.rs`,
  `src/cli/label.rs`, `src/adapter/mod.rs`, and the adapter test modules, and
  revert `pub(crate) mod label` in `src/cli/mod.rs`. The upstream text of
  `label.rs`, `activity.rs`, and `tool_name.rs` is otherwise intact, so an
  upstream merge conflicts in the strategy table and the classifier arm, not in
  their structure.
- Merge risk concentrates in `src/cli/label.rs` (fork-shaped) and the two new
  functions in `src/adapter/mod.rs`; take upstream's table and reapply the
  additions on top.

## Open Items

- **Codex's hook argument spellings for `apply_patch`, `webrun`, and
  `request_user_input_async` are not observable** from this machine (Codex
  rewrites arguments before the hook fires, and no payload dump exists). What
  each tool's *own* arguments look like is known for two of them, from Codex's
  log database: `apply_patch` carries the patch document, and `webrun` carries
  `search_query: [{q, …}]` (26 of 94 logged `web__run` calls; none carry the
  `action`/`queries` spelling guessed earlier). The extractors accept every
  evidenced shape (including a scan of any string value that carries patch
  markers), so an unanticipated spelling yields a value rather than a blank. A
  spelling that yields *nothing* is the residual risk: the fallback cannot fire
  for a mapped tool, so that row would be blank until the spelling is learned.
- **Codex MCP calls produced no activity entry** in the audited window; whether
  Codex fires `PostToolUse` for MCP tools is unconfirmed. The docs say the log
  shows "whatever the agent calls" rather than claiming MCP coverage.
- **`PermissionRequest` normalisation has no consumer.** `src/cli/hook.rs`
  destructures the event with `..`, so the tool name and input it now
  canonicalises are unobservable today; they are normalised anyway so the event
  contract stays uniform, and its test is a contract test rather than behaviour
  coverage.
- **Kimi's `tool_output` response key stays unmapped.** `label_agent` prefers
  `description`, and Kimi has no `TaskCreate`, so nothing reads the response for
  that adapter; mapping the key would add a code path with no consumer.
- **Control characters and escapes** in a label reach the log file and the
  clipboard (not the sidebar's terminal, which filters them). Stripping them
  would conflict with the "a command arrives exactly as it ran" contract, so it
  is left as a known, documented residual.
- **OpenCode's vocabulary** comes from upstream source (no local install), so
  version-specific ids (`patch` vs `apply_patch`, `list`) are mapped for both
  generations rather than one; `todoread` and `execute` are deliberately not
  mapped.
- **Background-command detection** (`@pane_bg_cmd`) still keys off `Bash`
  plus `run_in_background`. That is correct for Claude and Kimi; Codex's exec
  payload carries no such flag and OpenCode's `background` lives on its task
  tool, so those two panes cannot report a background command.
