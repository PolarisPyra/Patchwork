// Profiles the workspace under a heavy load: 300 tasks across three projects and one long thread
// of 150 turns with Markdown, code, reasoning, and tool calls, 400 changed files, a 5000-line diff
// and file, and 5000 project files. Times startup, the long thread (open, switch, stream, type,
// scroll), the Changes view, the canvas diff and Code tabs, the archive, the command palette,
// settings, and the @ file picker. Prints each scenario's main-thread time, layout and style counts,
// frame rate, and its hottest functions by self time from a CPU profile.
// Runs with GPU compositing off, as the software-rendered webview does.
// Usage: node checks/profile-heavy.mjs   (QA_URL=http://localhost:4173/ for a vite preview build)
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const URL = process.env.QA_URL ?? "http://localhost:1420/";
const TOP = Number(process.env.QA_TOP ?? 12);

// Runs after the stub: swaps in the heavy data and lets the check emit agent events.
const hookScript = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const callbacks = {};
  let next = 1;
  internals.transformCallback = (cb) => { const id = next++; callbacks[id] = cb; return id; };
  window.__qaEmit = (threadId, event) => callbacks[window.__qaAgentHandler]({ payload: { threadId, event } });
  const now = Date.now();
  const projects = ["qa-project", "qa-other", "qa-third"];
  const base = { model: null, accessMode: "full-access", effort: null, sessionId: null, goal: null, archivedAt: null, parent: null, planMode: false, slashCommands: [], contextPending: false };
  const msg = (id, role, text, createdAt, extra = {}) => ({ id, role, text, parts: [], createdAt, finishedAt: createdAt + 30000, status: "complete", tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, cutOff: false, usage: null, ...extra });
  const body = (i) => "## Step " + i + "\\n\\nI looked at **the renderer** and found that \`useThreads\` re-reads the list after each change. Here is what I changed:\\n\\n- Moved the folder lookup out of the loop\\n- Kept the order stable\\n- Added a [link](https://example.com) to the docs\\n\\n\`\`\`ts\\nexport function sum(xs: number[]): number {\\n  let total = 0;\\n  for (const x of xs) total += x;\\n  return total;\\n}\\n\`\`\`\\n\\nThat keeps the sidebar quiet while a reply streams.";
  const messages = [];
  for (let i = 0; i < 150; i++) {
    const t = now - (150 - i) * 120000;
    messages.push(msg("u" + i, "user", "Please look at part " + i + " of the renderer and tidy it up.", t));
    const tools = Array.from({ length: 4 }, (_, k) => ({ id: "t" + i + "-" + k, name: k % 2 ? "Read" : "Bash", summary: k % 2 ? "src/file" + k + ".ts" : "rg -n useThreads apps/web/src", status: "done", subagent: null, parent: null, startedAt: t + k * 1000, endedAt: t + k * 1000 + 500, output: "line one\\nline two\\nline three" }));
    const parts = [{ type: "thinking", text: "Reading the request and the files involved.", startedAt: t, endedAt: t + 4000 }, ...tools.map((x) => ({ type: "tool", id: x.id })), { type: "text", text: body(i) }];
    messages.push(msg("a" + i, "assistant", body(i), t + 1000, { tools, parts, files: ["src/a" + i + ".ts"], usage: { input: 1000, cacheRead: 20000, cacheWrite: 100, output: 800, context: 40000 } }));
  }
  const threads = [{ ...base, id: "qa-big", projectId: "qa-project", title: "Long thread", agent: "claude", createdAt: now - 1000, messages, folderId: null }];
  for (let i = 0; i < 300; i++) {
    const t = now - (i + 2) * 60000;
    threads.push({ ...base, id: "qa-t" + i, projectId: projects[i % 3], title: "Task number " + i + " with a longer title", agent: ["claude", "codex", "antigravity"][i % 3], createdAt: t, folderId: i % 7 === 0 ? "qa-folder" : null, archivedAt: i % 11 === 5 ? t : null, messages: [msg("m" + i, "user", "Do task " + i, t), msg("r" + i, "assistant", "Done with task " + i, t + 1000, { parts: [{ type: "text", text: "Done with task " + i }] })] });
  }
  internals.invoke = async (cmd, args) => {
    if (cmd === "plugin:event|listen" && args && args.event === "agent-event") window.__qaAgentHandler = args.handler;
    if (cmd === "list_threads") return threads;
    if (cmd === "get_thread") return threads.find((t) => t.id === args.id);
    if (cmd === "git_changes") return Array.from({ length: 400 }, (_, i) => ({ path: "apps/web/src/dir" + (i % 20) + "/file" + i + ".ts", status: i % 9 ? "modified" : "added", additions: i % 50, deletions: i % 7, staged: false }));
    if (cmd === "git_file_diff") return "--- a/x.ts\\n+++ b/x.ts\\n" + Array.from({ length: 100 }, (_, h) => "@@ -" + (h * 50 + 1) + ",50 +" + (h * 50 + 1) + ",50 @@\\n" + Array.from({ length: 50 }, (_, k) => (k % 5 === 0 ? "-" : k % 5 === 1 ? "+" : " ") + "  const value" + k + " = compute(" + h + ", " + k + "); // line").join("\\n")).join("\\n");
    if (cmd === "read_file") return Array.from({ length: 5000 }, (_, k) => "export function f" + k + "(a: number): number { return a * " + k + "; }").join("\\n");
    if (cmd === "list_files") return Array.from({ length: 5000 }, (_, i) => "apps/web/src/dir" + (i % 50) + "/module" + i + ".tsx");
    if (cmd === "send_message") return msg("sent" + Date.now(), "user", args.text, Date.now());
    return invoke(cmd, args);
  };
})();`;

const browser = await chromium.launch({ args: ["--disable-gpu", "--disable-gpu-compositing"] });
const context = await browser.newContext({ viewport: { width: 1400, height: 900 } });
const page = await context.newPage();
await page.addInitScript({ content: readFileSync(join(here, "../lib/tauri-stub.js"), "utf8") });
await page.addInitScript({ content: hookScript });
// QA_NO_FOCUS=1 turns focus() into a no-op, to see what focusing costs on its own.
// QA_INIT_JS runs extra script before the app, e.g. to flip an experiment switch.
if (process.env.QA_INIT_JS) await page.addInitScript({ content: process.env.QA_INIT_JS });
if (process.env.QA_NO_FOCUS)
  await page.addInitScript({ content: "HTMLElement.prototype.focus = function () {};" });
const session = await context.newCDPSession(page);
await session.send("Performance.enable");
await session.send("Profiler.enable");
await session.send("Profiler.setSamplingInterval", { interval: 200 });
const metrics = async () =>
  Object.fromEntries((await session.send("Performance.getMetrics")).metrics.map((m) => [m.name, m.value]));

// Self time per function, summed over the profile, as "name (file:line)".
function hottest(profile) {
  const self = new Map();
  const dt = new Map(profile.nodes.map((n) => [n.id, 0]));
  for (let i = 0; i < profile.samples.length; i++)
    dt.set(profile.samples[i], dt.get(profile.samples[i]) + (profile.timeDeltas[i] ?? 0));
  for (const n of profile.nodes) {
    const f = n.callFrame;
    const file = f.url.split("/").pop()?.split("?")[0] || "";
    const key = `${f.functionName || "(anon)"} ${file ? `(${file}:${f.lineNumber + 1}:${f.columnNumber + 1})` : ""}`;
    self.set(key, (self.get(key) ?? 0) + dt.get(n.id) / 1000);
  }
  const top = [...self]
    .filter(([k]) => !/^\((idle|program|garbage collector|root)\)/.test(k))
    .sort((a, b) => b[1] - a[1])
    .slice(0, TOP);
  // QA_CALLERS=name,name prints the call chains that lead to those functions.
  const callers = new Map();
  const parent = new Map();
  for (const n of profile.nodes) for (const c of n.children ?? []) parent.set(c, n);
  const names = (process.env.QA_CALLERS ?? "").split(",").filter(Boolean);
  for (const n of profile.nodes) {
    if (!names.includes(n.callFrame.functionName)) continue;
    const chain = [];
    for (let p = parent.get(n.id); p && chain.length < 6; p = parent.get(p.id)) {
      const f = p.callFrame;
      chain.push(`${f.functionName || "(anon)"}:${f.lineNumber + 1}`);
    }
    const key = `${n.callFrame.functionName} <- ${chain.join(" <- ")}`;
    callers.set(key, (callers.get(key) ?? 0) + dt.get(n.id) / 1000);
  }
  return { top, callers: [...callers].sort((a, b) => b[1] - a[1]).slice(0, 6) };
}

// Frame intervals from requestAnimationFrame while a scenario runs, to show whether it drops frames.
const startFrames = () => {
  const run = (window.__frameRun = (window.__frameRun ?? 0) + 1);
  const frames = (window.__frames = []);
  let last = null;
  const tick = (t) => {
    if (window.__frameRun !== run) return;
    if (last !== null) frames.push(t - last);
    last = t;
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
};
const stopFrames = () => {
  window.__frameRun = (window.__frameRun ?? 0) + 1;
  return window.__frames ?? [];
};

async function scenario(label, run) {
  await page.evaluate(startFrames).catch(() => undefined);
  await session.send("Profiler.start");
  const before = await metrics();
  const start = Date.now();
  await run();
  const seconds = (Date.now() - start) / 1000;
  const after = await metrics();
  const { profile } = await session.send("Profiler.stop");
  const ms = (name) => Math.round((after[name] - before[name]) * 1000);
  console.log(
    `\n== ${label}: ${seconds.toFixed(2)} s wall, busy ${ms("TaskDuration")} ms, script ${ms("ScriptDuration")} ms, ` +
      `style ${ms("RecalcStyleDuration")} ms, layout ${ms("LayoutDuration")} ms, ` +
      `${after.LayoutCount - before.LayoutCount} layouts, ${after.RecalcStyleCount - before.RecalcStyleCount} style recalcs`,
  );
  const frames = await page.evaluate(stopFrames).catch(() => []);
  if (frames.length > 1) {
    const sorted = [...frames].sort((a, b) => a - b);
    const p95 = sorted[Math.floor(sorted.length * 0.95)];
    console.log(
      `  frames: ${(frames.length / seconds).toFixed(0)} fps, p95 ${p95.toFixed(1)} ms, worst ${sorted.at(-1).toFixed(1)} ms`,
    );
  }
  const { top, callers } = hottest(profile);
  for (const [name, t] of top) console.log(`  ${t.toFixed(1).padStart(7)} ms  ${name}`);
  for (const [chain, t] of callers) console.log(`  ${t.toFixed(1).padStart(7)} ms  ${chain}`);
}

await scenario("startup", async () => {
  await page.goto(URL, { waitUntil: "networkidle", timeout: 30000 });
  await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 30000 });
  await page.locator("aside", { hasText: "Long thread" }).waitFor();
});
await page.waitForTimeout(500);

await scenario("open long thread", async () => {
  await page.locator("aside div.group.cursor-pointer", { hasText: "Long thread" }).click();
  await page.getByText("Step 149").first().waitFor({ timeout: 15000 });
  await page.waitForTimeout(300);
});

// QA_SHOT=<file.png> saves the opened long thread, to check its rendering.
if (process.env.QA_SHOT) await page.screenshot({ path: process.env.QA_SHOT });

await scenario("switch to a short task and back", async () => {
  for (let i = 0; i < 3; i++) {
    await page.locator("aside div.group.cursor-pointer", { hasText: "Task number 3 with" }).first().click();
    await page.waitForTimeout(150);
    await page.locator("aside div.group.cursor-pointer", { hasText: "Long thread" }).click();
    await page.getByText("Step 149").first().waitFor();
    await page.waitForTimeout(150);
  }
});

await scenario("stream 100 text deltas at 20/s", async () => {
  const now = Date.now();
  await page.evaluate((now) => {
    window.__qaEmit("qa-big", {
      type: "messageStarted",
      message: { id: "live", role: "assistant", text: "", parts: [], createdAt: now, finishedAt: null, status: "streaming", tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, cutOff: false, usage: null },
    });
  }, now);
  for (let i = 0; i < 100; i++) {
    await page.evaluate((i) => {
      window.__qaEmit("qa-big", { type: "textDelta", messageId: "live", text: (i % 10 === 0 ? "\n\n- item " : "") + "some streamed **words** here " });
    }, i);
    await page.waitForTimeout(50);
  }
});

await scenario("stream 60 deltas at 20/s into a 20 KB reply", async () => {
  await page.evaluate(() => {
    let text = "";
    for (let i = 0; text.length < 20000; i++)
      text += `### Part ${i}\n\nSome **bold** text, \`code\`, and a [link](https://example.com).\n\n- one\n- two\n\n\`\`\`ts\nconst x = ${i};\n\`\`\`\n\n`;
    window.__qaEmit("qa-big", { type: "textDelta", messageId: "live", text });
  });
  await page.waitForTimeout(300);
  for (let i = 0; i < 60; i++) {
    await page.evaluate(() => window.__qaEmit("qa-big", { type: "textDelta", messageId: "live", text: "more words " }));
    await page.waitForTimeout(50);
  }
});

await scenario("type 40 characters in the composer", async () => {
  await page.locator(".ProseMirror").first().click();
  await page.keyboard.type("Please refactor the sidebar rendering now", { delay: 30 });
});

await scenario("scroll the long thread to the top and back", async () => {
  const box = await page.locator("main").first().boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let i = 0; i < 40; i++) {
    await page.mouse.wheel(0, -1500);
    await page.waitForTimeout(25);
  }
  for (let i = 0; i < 40; i++) {
    await page.mouse.wheel(0, 1500);
    await page.waitForTimeout(25);
  }
});

await scenario("open the Changes view with 400 files", async () => {
  await page.locator('button[aria-label="Changes"]').click();
  await page.waitForFunction(() => document.querySelector('aside [title$="file399.ts"]'), null, { timeout: 15000 });
  await page.waitForTimeout(300);
});

await scenario("open a 5000-line diff in the canvas", async () => {
  await page.locator('aside [title$="/file1.ts"]').first().click();
  await page.waitForFunction(() => document.body.textContent?.includes("const value49"), null, { timeout: 15000 });
  await page.waitForTimeout(500);
});

await scenario("scroll the diff", async () => {
  const box = await page.locator("main").last().boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let i = 0; i < 40; i++) {
    await page.mouse.wheel(0, 1500);
    await page.waitForTimeout(25);
  }
});

await scenario("open a 5000-line file in the Code tab", async () => {
  await page.locator('[role="tab"]', { hasText: "Code" }).click();
  await page.waitForFunction(() => document.body.textContent?.includes("function f4999"), null, { timeout: 15000 });
  await page.waitForTimeout(500);
});

await scenario("scroll the file", async () => {
  const box = await page.locator("main").last().boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let i = 0; i < 40; i++) {
    await page.mouse.wheel(0, 1500);
    await page.waitForTimeout(25);
  }
});

await scenario("open the archive view", async () => {
  await page.locator('button[aria-label="Archived"]').click();
  await page.waitForFunction(() => document.querySelector("aside")?.textContent?.includes("Task number 5 with"), null, { timeout: 15000 });
  await page.waitForTimeout(300);
});

await scenario("command palette: open and type", async () => {
  await page.locator('button[aria-label="Tasks"]').click();
  await page.keyboard.press("Escape");
  await page.locator(".ProseMirror").first().click();
  await page.keyboard.press("Control+k");
  await page.waitForTimeout(200);
  await page.keyboard.type("task number 12", { delay: 30 });
  await page.waitForTimeout(300);
  await page.keyboard.press("Escape");
});

await scenario("settings: open and close", async () => {
  await page.keyboard.press("Control+,");
  await page.waitForTimeout(600);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
});

await scenario("@ file picker over 5000 files", async () => {
  await page.locator(".ProseMirror").first().click();
  await page.keyboard.type(" @mod", { delay: 30 });
  await page.waitForTimeout(300);
  await page.keyboard.type("ule42", { delay: 30 });
  await page.waitForTimeout(300);
  await page.keyboard.press("Escape");
});

await browser.close();
