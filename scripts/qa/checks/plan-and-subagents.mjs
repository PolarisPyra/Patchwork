// Opens a plan-mode chat whose replies show a steer cutting a reply short, two subagents (one
// still running, with its own thread), and a proposed plan, then opens the running subagent's
// thread from its card. Screenshots each step.
// Usage: node checks/plan-and-subagents.mjs [out-prefix]   (default /tmp/plan-and-subagents)

import { openWorkspace } from "../lib/open.mjs";

const prefix = process.argv[2] ?? "/tmp/plan-and-subagents";

const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const now = Date.now();
  const base = { plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null, cutOff: false };
  const user = (id, text, at, extra = {}) => ({ ...base, id, role: "user", text, status: "complete", createdAt: at, finishedAt: at, tools: [], parts: [], ...extra });
  const agent = (id, description, status, at, extra = {}) => ({
    id, name: "Agent", summary: description, status, parent: null, startedAt: at, endedAt: status === "running" ? null : at + 42000,
    subagent: { kind: "Explore", description, model: null, prompt: "Find where steering is handled." }, ...extra,
  });
  const t0 = now - 300000;
  const cut = {
    ...base, id: "a1", role: "assistant", status: "complete", createdAt: t0 + 1000, finishedAt: t0 + 20000, cutOff: true,
    text: "Starting on the runner", tools: [], parts: [{ type: "text", text: "Starting on the runner" }],
  };
  const plan = "# Keep subagents alive through a steer\\n\\n## Summary\\nBackground foreground subagents before a steer.\\n\\n## Key changes\\n- Send background_tasks for each running subagent\\n- Wait up to 5 s for Claude to confirm\\n\\n## Test plan\\n- Steer while a subagent runs and check it finishes";
  const reply = {
    ...base, id: "a2", role: "assistant", status: "complete", createdAt: t0 + 30000, finishedAt: t0 + 120000,
    text: "Here is the plan.",
    tools: [
      agent("s1", "Map the steer path", "running", t0 + 40000, { childThreadId: "qa-child" }),
      agent("s2", "Check the CLI protocol", "done", t0 + 41000, { output: "background_tasks is a stdin control request." }),
      { id: "c1", name: "Read", summary: "apps/server/src/runner.rs", status: "running", subagent: null, parent: "s1", startedAt: now - 2000, endedAt: null },
    ],
    parts: [{ type: "tool", id: "s1" }, { type: "tool", id: "s2" }, { type: "text", text: "Here is the plan." }, { type: "plan", text: plan }],
  };
  const messages = [user("u1", "Make steering keep subagents alive", t0), cut, user("u2", "Plan it first", t0 + 25000, { steer: true }), reply];
  const child = {
    id: "qa-child", projectId: "qa-project", title: "Map the steer path", agent: "claude", model: null, accessMode: "full-access", effort: null,
    sessionId: null, createdAt: t0 + 40000, goal: null, folderId: null, archivedAt: null, planMode: false,
    parent: { threadId: "qa-idle", toolId: "s1" },
    messages: [
      user("cu1", "Find where steering is handled.", t0 + 40000),
      { ...base, id: "ca1", role: "assistant", status: "streaming", createdAt: t0 + 40000, finishedAt: null, text: "Steering starts in Runner::steer.",
        tools: [{ id: "c1", name: "Read", summary: "apps/server/src/runner.rs", status: "running", subagent: null, parent: null, startedAt: now - 2000, endedAt: null }],
        parts: [{ type: "text", text: "Steering starts in Runner::steer." }, { type: "tool", id: "c1" }] },
    ],
  };
  internals.invoke = async (cmd, args) => {
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return [...r.map((t) => (t.id === "qa-idle" ? { ...t, messages, planMode: true } : t)), child];
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 900 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
const childListed = await page.locator("aside", { hasText: "Map the steer path" }).count();
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(1000);
await page.screenshot({ path: `${prefix}-chat.png` });

await page.getByRole("button", { name: "2 subagents" }).click();
await page.waitForTimeout(300);
await page.screenshot({ path: `${prefix}-subagents-open.png` });

await page.getByRole("button", { name: "Open Map the steer path" }).click();
await page.waitForTimeout(800);
await page.screenshot({ path: `${prefix}-child.png` });

await browser.close();
console.log(JSON.stringify({ subagentThreadInSidebar: childListed > 0, screenshots: ["chat", "subagents-open", "child"].map((s) => `${prefix}-${s}.png`) }, null, 2));
