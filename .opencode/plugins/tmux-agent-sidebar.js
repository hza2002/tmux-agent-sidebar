import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const resolveHookScript = () => {
  let dir = dirname(fileURLToPath(import.meta.url));
  for (let i = 0; i < 4; i += 1) {
    const candidate = resolve(dir, "hook.sh");
    if (existsSync(candidate)) {
      return candidate;
    }
    const parent = dirname(dir);
    if (parent === dir) {
      break;
    }
    dir = parent;
  }
  return null;
};

const HOOK_COMMAND = (() => {
  const hookScript = resolveHookScript();
  return hookScript
    ? { cmd: "bash", prefix: [hookScript, "opencode"] }
    : { cmd: "tmux-agent-sidebar", prefix: ["hook", "opencode"] };
})();

// Most callers remain fire-and-forget. Title publication follows the owning
// lifecycle write so a resumed session is registered before its metadata arrives.
const hook = (eventName, payload) => new Promise((done) => {
  try {
    const child = spawn(HOOK_COMMAND.cmd, [...HOOK_COMMAND.prefix, eventName], {
      stdio: ["pipe", "ignore", "ignore"],
    });
    child.on("error", done);
    child.on("close", done);
    child.stdin.on("error", () => {});
    child.stdin.end(JSON.stringify(payload));
  } catch {
    done();
  }
});

const pickFirstString = (value, keys) => {
  for (const key of keys) {
    const candidate = value?.[key];
    if (typeof candidate === "string" && candidate) {
      return candidate;
    }
  }
  return "";
};

const errorMessage = (err) => {
  if (!err) return "";
  if (typeof err === "string") return err;
  if (typeof err === "object") {
    return pickFirstString(err, ["message", "name"]) || JSON.stringify(err);
  }
  return String(err);
};

const extractPromptText = (parts) => {
  if (!Array.isArray(parts)) return "";
  const chunks = [];
  for (const part of parts) {
    if (!part || part.type !== "text") continue;
    if (part.synthetic || part.ignored) continue;
    if (typeof part.text === "string" && part.text) {
      chunks.push(part.text);
    }
  }
  return chunks.join("\n");
};

export const TmuxAgentSidebar = async ({ directory, client }) => {
  const cwd = typeof directory === "string" ? directory : "";

  const publishTitle = async (session_id, info) => {
    if (!session_id) return;
    try {
      const session = info ?? (await client?.session?.get({ path: { id: session_id } }))?.data;
      if (typeof session?.title === "string") {
        hook("session-title", { session_id, title: session.title });
      }
    } catch {
      // Missing title metadata leaves the prompt/response fallback available.
    }
  };

  return {
    "chat.message": async (input, output) => {
      const session_id =
        typeof input?.sessionID === "string" ? input.sessionID : "";
      const prompt = extractPromptText(output?.parts);
      await hook("user-prompt-submit", { cwd, session_id, prompt });
      await publishTitle(session_id);
    },

    event: async ({ event }) => {
      if (!event || !event.type) return;
      const props = event.properties ?? {};
      const session_id = pickFirstString(props, ["sessionID", "sessionId", "session_id"])
        || pickFirstString(props.info, ["id"]);

      switch (event.type) {
        case "session.created":
          await hook("session-start", { cwd, session_id, source: "startup" });
          await publishTitle(session_id, props.info);
          return;

        case "session.updated":
          await publishTitle(session_id, props.info);
          return;

        case "session.status": {
          // Status is a union: { type: "idle" | "busy" | "retry", ... }.
          // `busy` is a secondary status-transition signal — the real prompt
          // text is written via the `chat.message` hook, which fires with
          // the UserMessage parts. When both fire, the empty-prompt call
          // here is a no-op for @pane_prompt (handler guard) but still
          // advances status to "running" for cases where chat.message is
          // delayed or missing (e.g. retry → busy).
          const statusType = props.status?.type;
          if (statusType === "busy") {
            hook("user-prompt-submit", { cwd, session_id, prompt: "" });
          } else if (statusType === "idle") {
            hook("stop", { cwd, session_id, last_message: "" });
          }
          return;
        }

        case "session.idle":
          hook("stop", { cwd, session_id, last_message: "" });
          return;

        case "session.error":
          hook("stop-failure", {
            cwd,
            session_id,
            error: errorMessage(props.error) || "session.error",
          });
          return;

        case "permission.asked":
          hook("notification", { cwd, session_id, wait_reason: "permission" });
      }
    },

    // Dedicated hook for tool execution results. `event` bus does not carry
    // tool.execute.* — those are surfaced only through these trigger hooks.
    "tool.execute.after": async (input, output) => {
      hook("activity-log", {
        cwd,
        session_id: input?.sessionID ?? "",
        tool_name: input?.tool ?? "",
        tool_input: input?.args ?? {},
        tool_response: {
          title: output?.title ?? "",
          output: output?.output ?? "",
          metadata: output?.metadata ?? null,
        },
      });
    },
  };
};
