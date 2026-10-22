// Measures how smooth the workspace stays while work runs: frame intervals from requestAnimationFrame,
// and the main thread's busy share from CDP metrics. A 120 Hz display gives each frame 8.3 ms, so
// the busy share is the headroom that matters. Headless Chromium runs at 60 Hz, so its frame
// intervals sit near 16.7 ms even when idle. Runs with GPU compositing off, as the software-rendered
// webview does. Usage: node checks/frame-rate.mjs
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
// QA_URL points the check at another build, such as `vite preview` on 4173.
const URL = process.env.QA_URL ?? "http://localhost:1420/";
const DELTAS_PER_SECOND = 20;
const SAMPLE_MS = 5000;

// Runs after the stub. Lets the check emit agent events into qa-idle, and optionally gives qa-idle
// an active goal so its sidebar row draws the goal ring.
const hookScript = (withGoal) => `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const callbacks = {};
  let next = 1;
  internals.transformCallback = (cb) => { const id = next++; callbacks[id] = cb; return id; };
  window.__qaEmit = (event) => callbacks[window.__qaAgentHandler]({ payload: { threadId: "qa-idle", event } });
  const now = Date.now();
  const reply = { id: "live", role: "assistant", text: "", parts: [{ type: "thinking", text: "Reading the request. ", startedAt: now - 5000, endedAt: null }], createdAt: now - 5000, finishedAt: null, status: "streaming", tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null };
  const goal = { objective: "Ship the fix", status: "active", startedAt: now - 60000, checks: null, tokensUsed: null, tokenBudget: null, timeUsedSeconds: null };
  internals.invoke = async (cmd, args) => {
    if (cmd === "plugin:event|listen" && args && args.event === "agent-event") window.__qaAgentHandler = args.handler;
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages: [...t.messages, reply], goal: ${withGoal} ? goal : null } : t));
  };
})();`;

const browser = await chromium.launch({ args: ["--disable-gpu", "--disable-gpu-compositing"] });

// Records frame intervals in the page until stopFrames is called.
const startFrames = () => {
  window.__intervals = [];
  window.__rafStop = false;
  let last = null;
  const tick = (t) => {
    if (last !== null) window.__intervals.push(t - last);
    last = t;
    if (!window.__rafStop) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
};
const stopFrames = () => {
  window.__rafStop = true;
  return new Promise((resolve) => setTimeout(() => resolve(window.__intervals), 50));
};
const emit = () =>
  window.__qaEmit({ type: "thinking", messageId: "live", text: "Considering how the pieces fit together before deciding. " });

async function openPage(withGoal) {
  const context = await browser.newContext({ viewport: { width: 1200, height: 800 } });
  const page = await context.newPage();
  await page.addInitScript({ content: readFileSync(join(here, "../lib/tauri-stub.js"), "utf8") });
  await page.addInitScript({ content: hookScript(withGoal) });
  await page.goto(URL, { waitUntil: "networkidle", timeout: 15000 });
  await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
  await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
  await page.waitForTimeout(600);
  const session = await context.newCDPSession(page);
  await session.send("Performance.enable");
  const metrics = async () =>
    Object.fromEntries((await session.send("Performance.getMetrics")).metrics.map((m) => [m.name, m.value]));
  return { context, page, metrics };
}

async function sample(label, { page, metrics }, { deltas, animations = true }) {
  if (!animations) await page.addStyleTag({ content: "*, *::before, *::after { animation: none !important; }" });
  await page.evaluate(startFrames);
  const before = await metrics();
  const start = Date.now();
  let sent = 0;
  while (Date.now() - start < SAMPLE_MS) {
    if (deltas) {
      await page.evaluate(emit);
      sent++;
    }
    await page.waitForTimeout(1000 / DELTAS_PER_SECOND);
  }
  const intervals = await page.evaluate(stopFrames);
  const after = await metrics();
  const seconds = (Date.now() - start) / 1000;
  const share = (name) => (((after[name] - before[name]) / seconds) * 100).toFixed(1).padStart(5);
  const sorted = [...intervals].sort((a, b) => a - b);
  const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? 0;
  const fps = (intervals.length / seconds).toFixed(0);
  console.log(
    `${label.padEnd(36)} ${String(fps).padStart(4)} fps  p95 ${p95.toFixed(1).padStart(5)} ms  ` +
      `busy ${share("TaskDuration")}%  script ${share("ScriptDuration")}%  ` +
      `style+layout ${(Number(share("RecalcStyleDuration")) + Number(share("LayoutDuration"))).toFixed(1).padStart(5)}%` +
      (deltas ? `  (${sent} deltas)` : ""),
  );
}

for (const withGoal of [false, true]) {
  const { context, ...run } = await openPage(withGoal);
  const tag = withGoal ? "goal row" : "no goal";
  await sample(`spinner only, ${tag}`, run, { deltas: false });
  await sample(`spinner only, animations off, ${tag}`, run, { deltas: false, animations: false });
  await sample(`streaming deltas, ${tag}`, run, { deltas: true });
  await sample(`streaming, animations off, ${tag}`, run, { deltas: true, animations: false });
  await context.close();
}
await browser.close();
