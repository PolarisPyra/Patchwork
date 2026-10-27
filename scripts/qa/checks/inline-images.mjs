// Pastes an image into the middle of a draft and prints the editor's HTML, the Markdown it sends,
// and the Markdown again after the draft is reloaded from it, then screenshots the composer.
// Usage: node checks/inline-images.mjs [out.png]

import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2] ?? "/tmp/inline-images.png";
const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
const editor = page.locator(".ProseMirror").first();
await editor.waitFor({ timeout: 15000 });
await editor.click();
await page.keyboard.type("look at ");
await editor.evaluate((el) => {
  const dt = new DataTransfer();
  dt.items.add(new File([new Uint8Array([137, 80, 78, 71])], "shot.png", { type: "image/png" }));
  el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
});
await page.waitForTimeout(300);
await page.keyboard.type("and fix it");
const state = await editor.evaluate((el) => {
  const markdown = el.editor.getMarkdown();
  el.editor.commands.setContent(markdown, { contentType: "markdown" });
  return { html: el.innerHTML, markdown, reloaded: el.editor.getMarkdown() };
});
console.log(`-- html\n${state.html}\n-- markdown sent\n${JSON.stringify(state.markdown)}`);
console.log(`-- markdown after reload\n${JSON.stringify(state.reloaded)}`);
const chips = await page.getByText("IMAGE", { exact: true }).count();
console.log(`-- image chips above the draft: ${chips}`);
// The chip should be centered on the text beside it, as a $skill chip is.
const line = await page.evaluate(() => {
  const p = document.querySelector(".ProseMirror p");
  const chip = document.querySelector(".ProseMirror span[title]");
  const range = document.createRange();
  range.setStart(p.firstChild, 0);
  range.setEnd(p.firstChild, 4);
  const text = range.getBoundingClientRect();
  const c = chip.getBoundingClientRect();
  const center = (r) => r.top + r.height / 2;
  return {
    line: p.getBoundingClientRect().height,
    chip: c.height,
    offset: (center(c) - center(text)).toFixed(2),
  };
});
console.log(
  `-- line ${line.line}px, chip ${line.chip}px, chip center minus text center ${line.offset}px`,
);
await page.locator("form, .composer-editor").first().screenshot({ path: out });
console.log(`screenshot: ${out}`);
await browser.close();
