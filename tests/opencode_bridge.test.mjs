import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { EventEmitter } from "node:events";
import * as path from "node:path";
import * as url from "node:url";
import { SourceTextModule, SyntheticModule } from "node:vm";

// Evaluate the real bridge with an inert child process sink. No hook or tmux
// command can escape this fixture, including under the developer's live TMUX.
const calls = [];
const module = new SourceTextModule(await readFile(
  new URL("../.opencode/plugins/tmux-agent-sidebar.js", import.meta.url), "utf8"), {
  initializeImportMeta(meta) { meta.url = import.meta.url; },
});
await module.link((specifier) => {
  const exports = specifier === "node:child_process" ? {
    spawn(_cmd, args) {
      const child = new EventEmitter();
      child.stdin = new EventEmitter();
      child.stdin.end = (body) => {
        calls.push({ event: args.at(-1), payload: JSON.parse(body) });
        queueMicrotask(() => child.emit("close", 0));
      };
      return child;
    },
  } : specifier === "node:fs" ? { existsSync: () => false }
    : specifier === "node:path" ? path : url;
  return new SyntheticModule(Object.keys(exports), function () {
    for (const [key, value] of Object.entries(exports)) this.setExport(key, value);
  });
});
await module.evaluate();
let failLookup = false;
const bridge = await module.namespace.TmuxAgentSidebar({
  directory: "/repo",
  client: { session: { async get({ path: { id } }) {
    assert.equal(id, "ses-1");
    if (failLookup) throw new Error("unavailable");
    return { data: { id, title: "Resumed topic" } };
  } } },
});
await bridge.event({ event: { type: "session.created", properties: {
  info: { id: "ses-1", title: "Initial topic" },
} } });
assert.deepEqual(calls.splice(0), [
  { event: "session-start", payload: { cwd: "/repo", session_id: "ses-1", source: "startup" } },
  { event: "session-title", payload: { session_id: "ses-1", title: "Initial topic" } },
]);
await bridge.event({ event: { type: "session.updated", properties: {
  info: { id: "ses-1", title: "Renamed topic" },
} } });
assert.deepEqual(calls.splice(0), [
  { event: "session-title", payload: { session_id: "ses-1", title: "Renamed topic" } },
]);
await bridge["chat.message"]({ sessionID: "ses-1" }, { parts: [{ type: "text", text: "Current prompt" }] });
assert.deepEqual(calls.splice(0), [
  { event: "user-prompt-submit", payload: { cwd: "/repo", session_id: "ses-1", prompt: "Current prompt" } },
  { event: "session-title", payload: { session_id: "ses-1", title: "Resumed topic" } },
]);
failLookup = true;
await bridge["chat.message"]({ sessionID: "ses-1" }, { parts: [{ type: "text", text: "Fallback" }] });
assert.deepEqual(calls.splice(0), [
  { event: "user-prompt-submit", payload: { cwd: "/repo", session_id: "ses-1", prompt: "Fallback" } },
]);
await bridge.event({ event: { type: "session.updated", properties: {
  info: { id: "ses-1", title: "" },
} } });
assert.deepEqual(calls.splice(0), [
  { event: "session-title", payload: { session_id: "ses-1", title: "" } },
]);
await bridge.event({ event: { type: "session.updated", properties: { info: { title: "Unbound" } } } });
assert.deepEqual(calls, []);
