// Prints the composer's height and its last line's height as a draft gets a "$" menu, a skill chip,
// a "/" command and an @ file mention typed into it, so a chip or menu that grows the box shows up.
// Saves a screenshot of the composer at the end. Usage: node checks/composer-chip-height.mjs [out.png]

import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2] ?? "/tmp/composer-chip-height.png";
const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
const editor = page.locator(".ProseMirror").first();
await editor.waitFor({ timeout: 15000 });
const measure = async (label) => {
  await page.waitForTimeout(300);
  const sizes = await editor.evaluate((el) => {
    const box = el.closest("div.relative.flex.flex-col");
    const lines = [...el.querySelectorAll("p")].map((p) => Math.round(p.getBoundingClientRect().height));
    return { box: Math.round(box.getBoundingClientRect().height), editor: Math.round(el.getBoundingClientRect().height), lines };
  });
  console.log(`${label.padEnd(22)} box ${sizes.box}  editor ${sizes.editor}  lines ${sizes.lines.join(",")}`);
};

await editor.click();
await page.keyboard.type("first line");
await page.keyboard.press("Shift+Enter");
await page.keyboard.type("second line");
await page.keyboard.press("Shift+Enter");
await page.keyboard.type("third line");
await page.keyboard.press("Shift+Enter");
await page.keyboard.type("fourth line");
await measure("plain text");
await page.keyboard.type(" using $re");
await measure('"$" menu open');
await page.keyboard.press("Enter");
await measure("skill chip");
await page.keyboard.type("then /");
await measure('"/" typed');
await page.keyboard.press("Escape");
await page.keyboard.type("x and @");
await page.waitForTimeout(300);
await page.keyboard.press("Enter");
await measure("file mention");
await editor.screenshot({ path: out });
await page.keyboard.press("Control+A");
await page.keyboard.press("Delete");
await browser.close();
console.log(`saved ${out}`);
