// Opens a long chat whose last reply is still streaming, then sends text deltas while the view
// sits at the bottom. Reports whether the "Jump to latest message" button shows and how far the
// list is from the bottom. Usage: node checks/stream-follow.mjs

import { openWorkspace } from "../lib/open.mjs";

// Runs after the stub. Captures the agent-event listener so the check can send events to it, and
// gives the idle chat a long history whose last reply is streaming.
const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const callbacks = {};
  let next = 1;
  internals.transformCallback = (cb) => {
    const id = next++;
    callbacks[id] = cb;
    return id;
  };
  window.__qaEmit = (event) => callbacks[window.__qaAgentHandler]({ payload: { threadId: "qa-idle", event } });
  const now = Date.now();
  const messages = Array.from({ length: 40 }, (_, i) => ({
    id: "long" + i, role: i % 2 ? "assistant" : "user", text: "Paragraph " + i + ". " + "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(6),
    parts: [], createdAt: now - (40 - i) * 60000, finishedAt: i === 39 ? null : now - (40 - i) * 60000 + 1000,
    status: i === 39 ? "streaming" : "complete",
    tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null,
  }));
  internals.invoke = async (cmd, args) => {
    if (cmd === "plugin:event|listen" && args && args.event === "agent-event") window.__qaAgentHandler = args.handler;
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages } : t));
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(800);

const button = page.locator('button[aria-label="Jump to latest message"]');
// Distance from the bottom of the message list, in pixels.
const gap = () =>
  page.evaluate(() => {
    const list = document.querySelector('[class*="overflow-anchor"]');
    return list ? { gap: list.scrollHeight - list.scrollTop - list.clientHeight, height: list.scrollHeight } : null;
  });

const report = [];
async function record(label) {
  report.push({ label, buttonVisible: await button.isVisible(), ...(await gap()) });
}

await page.waitForTimeout(500);
await record("streaming starts at bottom");
for (let i = 1; i <= 60; i++) {
  await page.evaluate(() =>
    window.__qaEmit({ type: "textDelta", messageId: "long39", text: "Streamed words keep arriving at the end of the reply. ".repeat(3) }),
  );
  await page.waitForTimeout(100);
  if (i % 15 === 0) await record(`after ${i} deltas`);
}
await record("streaming settled");
await browser.close();
console.log(JSON.stringify(report, null, 2));
