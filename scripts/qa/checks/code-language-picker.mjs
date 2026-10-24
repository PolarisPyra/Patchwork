// Pastes code into the composer, opens the code block's language picker, and prints where the
// picker sits against the window and whether its filter field and last visible row can be seen
// (not cut off by the composer), then saves a screenshot.
// Usage: node checks/code-language-picker.mjs [out.png]

import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2] ?? "/tmp/code-language-picker.png";
const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
const editor = page.locator(".ProseMirror").first();
await editor.waitFor({ timeout: 15000 });
await editor.evaluate((el) => {
  const dt = new DataTransfer();
  dt.setData("text/plain", "fn main() {\n    tauri_build::build()\n}");
  el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
});
await page.waitForTimeout(500);
await page.locator('.ProseMirror button[aria-haspopup="listbox"]').click();
const filter = page.getByLabel("Filter languages");
await filter.waitFor({ timeout: 5000 });
const report = await filter.evaluate((input) => {
  const popup = input.parentElement;
  const rect = popup.getBoundingClientRect();
  // A point is visible when the topmost element there is inside the picker.
  const visible = (x, y) => popup.contains(document.elementFromPoint(x, y));
  const field = input.getBoundingClientRect();
  const rows = [...popup.querySelectorAll("button")];
  const last = rows[Math.min(rows.length, 6) - 1].getBoundingClientRect();
  return {
    popup: { top: Math.round(rect.top), bottom: Math.round(rect.bottom), height: Math.round(rect.height) },
    window: innerHeight,
    fieldVisible: visible(field.left + 10, field.top + field.height / 2),
    sixthRowVisible: visible(last.left + 10, last.top + last.height / 2),
  };
});
console.log(JSON.stringify(report));
await page.screenshot({ path: out });
console.log(`saved ${out}`);
await browser.close();
