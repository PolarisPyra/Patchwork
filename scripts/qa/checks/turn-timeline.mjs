// Opens a chat with a pasted multi-line prompt, a finished reply, and a streaming one, both with ordered parts (reasoning,
// tool calls, text), and screenshots the turn timeline folded, unfolded, and with a group open.
// Usage: node checks/turn-timeline.mjs [out-prefix]   (default /tmp/turn-timeline)

import { openWorkspace } from "../lib/open.mjs";

const prefix = process.argv[2] ?? "/tmp/turn-timeline";

const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const now = Date.now();
  const tool = (id, name, summary, status, output, at) => ({
    id, name, summary, status, subagent: null, parent: null, startedAt: at, endedAt: status === "running" ? null : at + 2000,
    ...(output ? { output } : {}),
  });
  const base = { plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null };
  const t0 = now - 600000;
  const done = {
    ...base, id: "a1", role: "assistant", status: "complete", createdAt: t0 + 1000, finishedAt: t0 + 124000,
    text: "Looking at the runner first.\\n\\nThe leftover process came from a detached shell. Stopping the whole scope fixes it.",
    tools: [
      tool("c1", "Read", "apps/server/src/runner.rs", "done", "fn drive(...) {\\n  ...\\n}", t0 + 5000),
      tool("c2", "Grep", "signal_process_group", "done", "runner.rs:437\\nrunner.rs:1040", t0 + 8000),
      tool("c3", "Bash", "cargo clippy --workspace --all-targets", "done", "Finished dev profile in 1.2s", t0 + 20000),
      tool("c4", "Bash", "cargo test --workspace", "done", "test result: ok. 32 passed", t0 + 40000),
      tool("c5", "Edit", "apps/server/src/scope.rs", "done", null, t0 + 60000),
    ],
    parts: [
      { type: "thinking", text: "The user says chats hang. Check the memory slice and the runner's cleanup.", startedAt: t0 + 1000, endedAt: t0 + 4000 },
      { type: "tool", id: "c1" },
      { type: "tool", id: "c2" },
      { type: "text", text: "Looking at the runner first." },
      { type: "tool", id: "c5" },
      { type: "tool", id: "c3" },
      { type: "tool", id: "c4" },
      { type: "text", text: "\\n\\nThe leftover process came from a detached shell. Stopping the whole scope fixes it." },
    ],
  };
  const t1 = now - 30000;
  const live = {
    ...base, id: "a2", role: "assistant", status: "streaming", createdAt: t1, finishedAt: null,
    text: "Now checking the frontend.",
    plan: [{ text: "Map the timeline", status: "done" }, { text: "Rebuild it", status: "running" }, { text: "Verify", status: "pending" }],
    tools: [
      tool("d1", "Read", "apps/web/src/components/chat/Messages.tsx", "done", "...", t1 + 3000),
      tool("d2", "Bash", "pnpm typecheck", "running", null, t1 + 9000),
    ],
    parts: [
      { type: "thinking", text: "Next, the frontend reducer.", startedAt: t1, endedAt: t1 + 2000 },
      { type: "tool", id: "d1" },
      { type: "text", text: "Now checking the frontend." },
      { type: "tool", id: "d2" },
    ],
  };
  const user = (id, text, at) => ({ ...base, id, role: "user", text, status: "complete", createdAt: at, finishedAt: at, tools: [], parts: [] });
  const messages = [user("u1", "Do these steps in order:\\n\\n1. Read the file.\\n\\n2. Search the repo.\\n\\n3. Run \`git status\`.\\n\\nDon't edit any files.\\n\\n", t0), done, user("u2", "Now redo the timeline", t1 - 1000), live];
  internals.invoke = async (cmd, args) => {
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages } : t));
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 900 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(1000);
await page.screenshot({ path: `${prefix}-folded.png` });

await page.getByRole("button", { name: /Worked for/ }).click();
await page.waitForTimeout(400);
await page.screenshot({ path: `${prefix}-unfolded.png` });

await page.getByRole("button", { name: /Ran 2 commands|Changed 1 file/ }).first().click();
await page.waitForTimeout(400);
await page.screenshot({ path: `${prefix}-group-open.png` });

await browser.close();
console.log(`${prefix}-folded.png ${prefix}-unfolded.png ${prefix}-group-open.png`);
