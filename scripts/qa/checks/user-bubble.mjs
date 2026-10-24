// Shows a user message with Shift+Enter line breaks and a pasted terminal block in the chat, then
// prints each bubble line's top offset (lines sit one line-height apart when no break is doubled)
// and saves a screenshot. Usage: node checks/user-bubble.mjs [out.png]

import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2] ?? "/tmp/user-bubble.png";
const text = [
  "first line  ",
  "second line  ",
  "third line",
  "",
  "```",
  "~/project main > ./start.sh",
  "{",
  '  "region": {',
  '    "size": 49152',
  "  }",
  "}",
  "```",
].join("\n");

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
// Puts the user message ahead of the stub thread's reply.
await page.addInitScript((text) => {
  const ipc = window.__TAURI_INTERNALS__;
  const invoke = ipc.invoke;
  ipc.invoke = async (cmd, args) => {
    const result = await invoke(cmd, args);
    if (cmd !== "get_thread" || !result) return result;
    const reply = result.messages[0];
    const user = { ...reply, id: "qa-user", role: "user", text, status: "complete", createdAt: reply.createdAt - 1000 };
    return { ...result, messages: [user, ...result.messages] };
  };
}, text);
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
const bubble = page.locator("div.rounded-\\[12px\\]", { hasText: "first line" }).first();
await bubble.waitFor({ timeout: 15000 });
const tops = await bubble.locator("p").first().evaluate((p) => {
  const range = document.createRange();
  range.selectNodeContents(p);
  return [...new Set([...range.getClientRects()].map((r) => Math.round(r.top)))];
});
console.log("paragraph line tops:", tops.join(", "));
console.log("line-height:", await bubble.evaluate((el) => getComputedStyle(el).lineHeight));
await bubble.screenshot({ path: out });
console.log(`saved ${out}`);
await browser.close();
