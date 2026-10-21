// Opens a long chat, then reports whether the "Jump to latest message" button shows while the
// chat is at the bottom, before and after the window is resized. Usage: node checks/jump-button.mjs

import { openWorkspace } from "../lib/open.mjs";

// Runs after the stub, so it wraps the stub's invoke and gives the idle chat a long history.
const script = `(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const now = Date.now();
  const messages = Array.from({ length: 40 }, (_, i) => ({
    id: "long" + i, role: i % 2 ? "assistant" : "user", text: "Paragraph " + i + ". " + "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(6),
    parts: [], createdAt: now - (40 - i) * 60000, finishedAt: now - (40 - i) * 60000 + 1000, status: "complete",
    tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null,
  }));
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
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
const report = [];
const gapState = () =>
  page.evaluate(() => {
    const list = document.querySelector('[class*="overflow-anchor"]');
    return { gap: list.scrollHeight - list.scrollTop - list.clientHeight, viewport: list.clientHeight };
  });
async function record(label) {
  await page.waitForTimeout(600);
  report.push({ label, visible: await button.isVisible(), height: await page.evaluate(() => innerHeight) });
}

await record("opened at bottom");
for (const height of [700, 650, 720, 800]) {
  await page.setViewportSize({ width: 1200, height });
  await record(`resized to ${height}`);
}
// Scrolling up well away from the end should show the button.
await page.mouse.move(600, 400);
await page.mouse.wheel(0, -4000);
await record("scrolled up");

// Back at the bottom, then a diagonal drag of the bottom-right corner: width and height change
// together, in small steps, with no pause between them.
await page.evaluate(() => {
  const list = document.querySelector('[class*="overflow-anchor"]');
  list.scrollTop = list.scrollHeight;
});
await page.waitForTimeout(800);
report.push({ label: "back at bottom before drag", visible: await button.isVisible() });
for (let i = 1; i <= 40; i++) {
  await page.setViewportSize({ width: 1200 - i * 12, height: 800 - i * 10 });
  await page.waitForTimeout(30);
  report.push({ label: `diagonal step ${i}`, visible: await button.isVisible(), ...(await gapState()) });
}
for (let i = 40; i >= 0; i--) {
  await page.setViewportSize({ width: 1200 - i * 12, height: 800 - i * 10 });
  await page.waitForTimeout(30);
  report.push({ label: `diagonal back ${i}`, visible: await button.isVisible(), ...(await gapState()) });
}
await browser.close();
console.log(JSON.stringify(report, null, 2));
