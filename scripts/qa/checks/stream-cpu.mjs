// Measures the renderer's CPU while a reply streams reasoning, to see what the live view costs:
// idle, streaming with the UI's animations, streaming with animations off, and a live row with
// animations but no deltas. Chromium runs without GPU compositing so paint cost shows up as CPU,
// as it does in the software-rendered webview. Usage: node checks/stream-cpu.mjs
import { readFileSync } from "node:fs";
import { chromium } from "playwright";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const URL = "http://localhost:1420/";
const DELTAS_PER_SECOND = 20;
const SAMPLE_MS = 6000;

// Runs after the stub. Captures the agent-event listener and gives qa-idle a reply that is
// streaming a reasoning block.
const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const callbacks = {};
  let next = 1;
  internals.transformCallback = (cb) => { const id = next++; callbacks[id] = cb; return id; };
  window.__qaEmit = (event) => callbacks[window.__qaAgentHandler]({ payload: { threadId: "qa-idle", event } });
  const now = Date.now();
  const reply = { id: "live", role: "assistant", text: "", parts: [{ type: "thinking", text: "Reading the request. ", startedAt: now - 5000, endedAt: null }], createdAt: now - 5000, finishedAt: null, status: "streaming", tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null };
  internals.invoke = async (cmd, args) => {
    if (cmd === "plugin:event|listen" && args && args.event === "agent-event") window.__qaAgentHandler = args.handler;
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages: [...t.messages, reply] } : t));
  };
})();`;

const browser = await chromium.launch({ args: ["--disable-gpu", "--disable-gpu-compositing"] });
const context = await browser.newContext({ viewport: { width: 1200, height: 800 } });
const page = await context.newPage();
await page.addInitScript({ content: readFileSync(join(here, "../lib/tauri-stub.js"), "utf8") });
await page.addInitScript({ content: script });
await page.goto(URL, { waitUntil: "networkidle", timeout: 15000 });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });

// The renderer process of this page, found through the browser's own process list.
const session = await browser.newBrowserCDPSession();
const { processInfo } = await session.send("SystemInfo.getProcessInfo");
const renderer = processInfo.filter((p) => p.type === "renderer").sort((a, b) => b.cpuTime - a.cpuTime)[0];
const cpuOf = () => {
  const stat = readFileSync(`/proc/${renderer.id}/stat`, "utf8").split(") ")[1].split(" ");
  return (Number(stat[11]) + Number(stat[12])) / 100; // utime + stime, seconds
};
async function sample(label, { deltas, animations }) {
  if (!animations) await page.addStyleTag({ content: "*, *::before, *::after { animation: none !important; }" });
  const before = cpuOf();
  const start = Date.now();
  let sent = 0;
  while (Date.now() - start < SAMPLE_MS) {
    if (deltas) {
      await page.evaluate(() => window.__qaEmit({ type: "thinking", messageId: "live", text: "Considering how the pieces fit together before deciding. " }));
      sent++;
    }
    await page.waitForTimeout(1000 / DELTAS_PER_SECOND);
  }
  const used = cpuOf() - before;
  console.log(`${label.padEnd(44)} ${((used / (SAMPLE_MS / 1000)) * 100).toFixed(1).padStart(6)}% of one core${deltas ? `  (${sent} deltas)` : ""}`);
}

await page.locator("aside div.group.cursor-pointer", { hasText: "Refactor the diff view" }).click();
await page.waitForTimeout(600);
await sample("plain streaming task selected (stub, no deltas)", { deltas: false, animations: true });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(600);
await sample("thinking live, animations on, no deltas", { deltas: false, animations: true });
await sample("thinking live, animations on, deltas", { deltas: true, animations: true });
await sample("thinking live, animations off, deltas", { deltas: true, animations: false });
await sample("thinking live, animations off, no deltas", { deltas: false, animations: false });
await browser.close();
