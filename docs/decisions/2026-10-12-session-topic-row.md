# Decision: Show native session titles in the existing preview row

## Context

The 35-column sidebar cannot fit a title beside the agent, permission badge,
branch, and ports. Prompt and response openings often carry little identifying
information. The existing preview row and selection expansion can carry a
stable session topic without adding rows or changing workflow state.

## Chosen Seam

Keep agent names in the status row. Prefer the native session title in the
preview row, with one line normally and at most three on selection. Fall back
to the existing prompt/response preview when no title is available.

Extend the existing background session-name scanner for Claude persisted titles,
Codex's session index, and Kimi's indexed session state. Claude transcripts are
read incrementally and only for sessions present in its live-session registry.
Manual Claude names win over generated titles; derived registry names are a
fallback. All file readers remain in the session leaf and polling worker.

OpenCode uses its existing plugin bridge and a normalized `SessionTitle` event.
The handler writes the existing `@pane_name` option only for the registered
session under the existing pane lock. The bulk query reads it, and refresh
preserves this hook-owned title instead of replacing it from the file cache.
Session changes and teardown clear the option. Title events never change status,
attention, prompts, or response-review ownership.

## Alternatives Rejected

- Replace the agent label with a title: hides identity and competes with ports.
- Generate summaries in the sidebar: adds model access, cost, and guessed state.
- Read OpenCode's database: requires another storage contract and database access
  when the installed bridge already receives title changes.
- Show only titles: new or unnamed sessions would lose the useful fallback.

## Upstream Compatibility

Use the upstream session scanner, adapter/event/handler pipeline, existing pane
option, bulk query, and row renderer. No dependency, new setting, command, or
daemon polling is added. The event is plugin metadata, not a hook declaration;
Claude/Codex/Kimi registrations remain unchanged.

## Conflict and Removal Strategy

Preserve upstream module ownership during merges and reapply the title readers
and preview preference at these seams. Remove the OpenCode event and title
readers together to revert the feature; the original prompt data and response
review transitions are retained, so rollback requires no data migration.
