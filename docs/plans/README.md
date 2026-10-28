# Plans and Historical Research

Plans preserve discussion and implementation evidence. They are not the current
runtime contract. Read [architecture](../maintainers/architecture-map.md),
[verification](../maintainers/verification.md), [state management](../state-management.md)
and [Usage metering](../usage-metering.md) for maintained behavior.

## October 2026 closeout

The implemented work shipped in `v0.16.0-hza.1`. Earlier "uncommitted", "draft"
and "no restart" statements describe the stage at which they were written.

| Topic | Accepted contract |
| --- | --- |
| Claude background work | [Decision](../decisions/2026-10-26-claude-background-work.md) |
| Workflow timers and idle folds | [Decision](../decisions/2026-10-26-workflow-timers-and-idle-folds.md) |
| Sidebar width restoration | [Decision](../decisions/2026-10-26-sidebar-width-restoration.md) |
| Usage pricing and display | [Decision](../decisions/2026-10-26-usage-metering.md), [current reference](../usage-metering.md) |
| Durable usage history | [Decision](../decisions/2026-10-26-durable-usage-history.md), [implementation and verification](2026-10-26-usage-history-implementation-plan.md) |

A web visualization, cloud backup management, automatic project rename matching
and previous-cycle comparisons remain outside this release. Research alternatives
are not a backlog commitment. Future changes should update maintained references
and their narrow decision records rather than rewriting historical discussion.

## Research references

These documents preserve investigation history. Maintained decisions supersede
their proposals; unselected alternatives are not backlog commitments.

| Topic | Document |
| --- | --- |
| Codex session account attribution across resumed sessions | [Research and options](2026-10-27-codex-account-attribution-research.md); [implemented runtime subset](../decisions/2026-10-28-codex-runtime-attribution.md) |
