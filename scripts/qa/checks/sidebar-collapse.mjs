// Times the sidebar tree opening and closing a project and a folder that hold many tasks:
// frames drawn, the longest gap between frames, and the layout, style and script work done while
// it animates. Runs with GPU compositing off, as the software-rendered webview does.
// Usage: node checks/sidebar-collapse.mjs   (TASKS=240 changes the number of tasks)

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const TASKS = Number(process.env.TASKS ?? 120);

// Runs after the stub: half the tasks sit in the project's folder, half at its top level.
const hookScript = `(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const now = Date.now();
  const base = { model: null, accessMode: "full-access", effort: null, sessionId: null, goal: null, archivedAt: null, parent: null, planMode: false, slashCommands: [], contextPending: false, messages: [], agent: "claude", projectId: "qa-project" };
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    const more = Array.from({ length: ${TASKS} }, (_, i) => ({ ...base, id: "qa-many" + i, title: "Task number " + i + " with a longer title", createdAt: now - (i + 2) * 60000, folderId: i % 2 ? "qa-folder" : null }));
    return [...r, ...more];
  };
})();`;

const browser = await chromium.launch({ args: ["--disable-gpu", "--disable-gpu-compositing"] });
const page = await browser.newPage({ viewport: { width: 1200, height: 800 } });
await page.addInitScript({ content: readFileSync(join(here, "../lib/tauri-stub.js"), "utf8") });
await page.addInitScript({ content: hookScript });
await page.goto("http://localhost:1420/", { waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor();
await page.waitForTimeout(500);
const cdp = await page.context().newCDPSession(page);
await cdp.send("Performance.enable");
const metrics = async () => Object.fromEntries((await cdp.send("Performance.getMetrics")).metrics.map((m) => [m.name, m.value]));

async function measure(label, click) {
  await page.evaluate(() => {
    const run = (window.__run = (window.__run ?? 0) + 1);
    window.__frames = [];
    let last = performance.now();
    const tick = (t) => { if (window.__run !== run || !window.__recording) return; window.__frames.push(t - last); last = t; requestAnimationFrame(tick); };
    window.__recording = true;
    requestAnimationFrame(tick);
  });
  const before = await metrics();
  await click();
  await page.waitForTimeout(450);
  const after = await metrics();
  const frames = await page.evaluate(() => { window.__recording = false; return window.__frames.slice(1); });
  const d = (k) => +(after[k] - before[k]).toFixed(3);
  console.log(label.padEnd(18), JSON.stringify({
    frames: frames.length,
    worstFrameMs: +Math.max(...frames).toFixed(1),
    framesOver25ms: frames.filter((f) => f > 25).length,
    layouts: d("LayoutCount"),
    layoutMs: +(d("LayoutDuration") * 1000).toFixed(0),
    styleRecalcs: d("RecalcStyleCount"),
    scriptMs: +(d("ScriptDuration") * 1000).toFixed(0),
    taskMs: +(d("TaskDuration") * 1000).toFixed(0),
  }));
}

const project = page.locator("aside").getByText("QA", { exact: true }).first();
const folder = page.locator("aside").getByText("Old", { exact: true }).first();
await measure("close project", () => project.click());
await measure("open project", () => project.click());
await measure("close folder", () => folder.click());
await measure("open folder", () => folder.click());
await browser.close();
