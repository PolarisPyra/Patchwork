// Prints, for each chat and task row in the sidebar, its right edge and the right edge of the
// nearest ancestor that clips it, so a selection border cut off on the right shows as clipped.
// Usage: node checks/thread-row-clip.mjs [out.png]

import { openWorkspace } from "../lib/open.mjs";

// The stub's threads are all tasks, so a chat without a project is added and opened, so its row
// has the selection border.
const init = `
  const invoke = window.__TAURI_INTERNALS__.invoke;
  let qaChat = null;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args, ...rest) => {
    if (cmd === "get_last_thread") return "qa-chat";
    if (cmd === "get_thread" && args?.id === "qa-chat") return qaChat;
    const out = await invoke(cmd, args, ...rest);
    if (cmd !== "list_threads") return out;
    qaChat = { ...out[0], id: "qa-chat", projectId: null, title: "A chat without a project" };
    return [...out, qaChat];
  };
`;
const { browser, page } = await openWorkspace({ width: 1200, height: 800, init });
await page.locator('[data-thread-id="qa-chat"]').waitFor({ timeout: 15000 });
await page.waitForTimeout(800);
const box = await page.locator('[data-thread-id="qa-chat"]').boundingBox();
await page.screenshot({
  path: process.argv[2] ?? "/tmp/thread-row-clip.png",
  clip: { x: box.x - 20, y: box.y - 30, width: box.width + 40, height: box.height + 60 },
});

const rows = await page.evaluate(() =>
  [...document.querySelectorAll("aside [data-thread-id]")].map((row) => {
    const right = row.getBoundingClientRect().right;
    let clip = row.parentElement;
    while (clip && getComputedStyle(clip).overflowX === "visible") clip = clip.parentElement;
    const clipRight = clip?.getBoundingClientRect().right ?? Infinity;
    return {
      text: row.textContent.trim().slice(0, 30),
      right: Math.round(right * 10) / 10,
      clipRight: Math.round(clipRight * 10) / 10,
      clipped: right > clipRight + 0.5,
    };
  }),
);
await browser.close();
for (const r of rows) console.log(JSON.stringify(r));
