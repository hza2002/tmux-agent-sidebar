# Codex PermissionRequest hook integration

## Context

Codex 0.153.4 exposes a `PermissionRequest` hook, but the sidebar only
handled completion and tool activity events. A Codex pane waiting for approval
therefore remained `working` and could not raise the sidebar's attention state.

## Chosen Seam

Add a dedicated `AgentEvent::PermissionRequest` variant and register the
Codex trigger through its existing adapter table. The hook dispatcher routes
the event to the existing notification/attention handler with the
`permission` wait reason, preserving one state-writing path.
The handler fences stale events against the pane's session, active turn, and
completed-turn markers before changing visible state.

## Alternatives Rejected

Mapping the trigger to the generic `Notification` event would generate the
wrong CLI event argument from the registration table and lose Codex-specific
turn fencing. Parsing raw JSON in the core handler would violate the adapter
boundary.

## Upstream Compatibility

The change uses the established adapter, normalized event, and handler seams;
Claude and OpenCode behavior is unchanged. Setup output automatically gains
the Codex registration.

## Conflict and Removal Strategy

During upstream merges, preserve the upstream adapter/event layout and
reapply this registration as a focused follow-up. If Codex removes this hook,
deleting the variant, registration, dispatch branch, tests, and this record
cleanly restores the prior behavior.
