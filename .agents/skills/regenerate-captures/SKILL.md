---
name: regenerate-captures
description: Regenerate tmux-agent-sidebar website capture PNGs from scenario fixtures after visible UI or fixture changes. Also use for UI checks and showcase requests; never hand-edit generated images.
---

# Regenerate Website Captures

The source of truth is `fixtures/scenarios/`; generated images live under
`website/src/assets/captures/`, with the social image at
`website/public/og-image.png`.

## Outputs

| Scenario | Output |
|---|---|
| `hero` | `hero.png`, `og-image.png` |
| `agent-pane-focus` | `agent-pane-focus.png` |
| `activity-focus` | `activity-focus.png` |
| `git-focus` | `git-focus.png` |
| `worktree-spawn` | `worktree-spawn.png` |
| `pet-idle` | `pet-idle.png` |
| `pet-walking` | `pet-walking.png` |
| `pet-working` | `pet-working.png` |

Shared pane state is seeded by `fixtures/scenarios/common/_lib.sh`, so one seed
change may affect several outputs.

## Workflow

1. Identify the scenarios affected by the visible code or fixture change.
2. Run `cargo build --release`; scenarios invoke the release binary.
3. Use `scripts/build-assets.sh` when every capture is affected. For a targeted
   update, run only the relevant scenario and renderer in a private directory:

   ```bash
   capture_tmp="$(mktemp -d -t tas-capture.XXXXXX)"
   ./fixtures/scenarios/<name>/scenario.sh "$capture_tmp"
   (cd website && node ../scripts/render-frames.mjs "$capture_tmp")
   cp "$capture_tmp/<name>.png" website/src/assets/captures/<name>.png
   ```

4. When `hero` changes, regenerate `og-image.png` with
   `scripts/hero-compose.mjs` and copy it to `website/public/og-image.png`.
5. Open every changed PNG. Confirm the intended change, branch label, ordering,
   colors, crop edges, and terminal chrome before handoff.

Only remove the exact temporary directory created for this run after verifying
its `tas-capture.*` prefix. Do not restart or reconfigure the user's live tmux
server. Scenario tmux sockets and activity logs stay under the fixture-owned
temporary directory.

## Handoff

Keep fixture and generated-image changes together. Do not commit them unless the
current request authorizes a commit. For worktree binary copying and macOS
signing, follow `docs/maintainers/verification.md`.
