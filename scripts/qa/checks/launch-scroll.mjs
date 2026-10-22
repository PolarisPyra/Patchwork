// Opens the workspace on a long chat, as a launch does, and records the message list's scroll
// position and height every frame for the first two seconds, to see whether the list moves after
// its first paint. Rows logged as hidden are not seen by the user. Usage: node checks/launch-scroll.mjs
import { readFileSync } from "node:fs";
import { chromium } from "playwright";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const URL = "http://localhost:1420/";

// Runs after the stub: a 30-message history on qa-idle, which is also the last open thread, so
// the workspace opens on it as it would after a launch.
const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const now = Date.now();
  const para = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(8);
  const code = "\\n\\n\`\`\`ts\\nexport function example(n: number) {\\n  return n * 2;\\n}\\n\`\`\`\\n\\n";
  const messages = Array.from({ length: 30 }, (_, i) => ({
    id: "long" + i, role: i % 2 ? "assistant" : "user",
    text: i % 2 ? "## Step " + i + "\\n\\n" + para + code + "- one\\n- two\\n- three" : "Question " + i + ". " + para,
    parts: [], createdAt: now - (30 - i) * 60000, finishedAt: now - (30 - i) * 60000 + 1000, status: "complete",
    tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null,
  }));
  internals.invoke = async (cmd, args) => {
    if (cmd === "get_last_thread") return "qa-idle";
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages } : t));
  };
})();`;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 800 } });
await page.addInitScript({ content: readFileSync(join(here, "../lib/tauri-stub.js"), "utf8") });
await page.addInitScript({ content: script });
// Samples from inside the page, so nothing is missed while the check waits on navigation.
await page.addInitScript({
  content: `window.__qaSamples = [];
    const start = performance.now();
    const tick = () => {
      const list = document.querySelector('[class*="overflow-anchor"]');
      if (list) {
        const first = [...list.querySelectorAll("h2, div")].filter((el) => el.children.length === 0 && el.textContent.startsWith("## Step 29")).concat([...list.querySelectorAll("h2")]).find((el) => el.textContent.includes("Step 29"));
        window.__qaSamples.push({ ...{ t: Math.round(performance.now() - start), top: Math.round(list.scrollTop), height: list.scrollHeight, view: list.clientHeight, gap: Math.round(list.scrollHeight - list.scrollTop - list.clientHeight) }, anchor: first ? Math.round(first.getBoundingClientRect().top) : null, shown: getComputedStyle(list).visibility !== "hidden" });
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);`,
});
await page.goto(URL, { waitUntil: "commit" });
await page.waitForTimeout(2500);
const samples = await page.evaluate(() => window.__qaSamples);
const composer = await page.evaluate(() => {
  const boxes = [...document.querySelectorAll("main div.rounded-\\[8px\\]")];
  const box = boxes.at(-1);
  return box ? { height: box.offsetHeight, placeholder: box.className.includes("h-[106px]") } : null;
});
console.log("composer box:", JSON.stringify(composer));
// Only moments where something changed, so the trace stays short.
let last = null;
for (const s of samples) {
  if (last && s.top === last.top && s.height === last.height && s.view === last.view && s.anchor === last.anchor && s.shown === last.shown) continue;
  console.log(`${String(s.t).padStart(5)} ms  scrollTop ${String(s.top).padStart(6)}  height ${String(s.height).padStart(6)}  viewport ${s.view}  gap ${String(s.gap).padStart(4)}  last heading at ${s.anchor}  ${s.shown ? "shown" : "hidden"}`);
  last = s;
}
await browser.close();
