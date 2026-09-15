---
title: Kimi Code setup
description: Register the Kimi Code hooks in ~/.kimi-code/config.toml.
---

Kimi Code registers hooks as `[[hooks]]` entries in `~/.kimi-code/config.toml`. No feature flag is required.

## Steps

1. Print the paste-ready snippet:

   ```sh
   tmux-agent-sidebar setup kimi
   ```

2. Append the printed block to `~/.kimi-code/config.toml`. Each entry looks like:

   ```toml
   [[hooks]]
   event = "SessionStart"
   matcher = "startup|resume"
   command = "bash ~/.tmux/plugins/tmux-agent-sidebar/hook.sh kimi session-start"
   ```

3. Restart Kimi Code so it reloads the hook configuration.

Kimi hooks fire with exit-code semantics: exit 0 allows the action, exit 2 blocks it. The sidebar's hook script always stays silent on stdout because Kimi appends hook stdout to the model context.
