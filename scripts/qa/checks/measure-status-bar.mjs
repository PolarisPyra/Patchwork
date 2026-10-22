// Measures the status bar's items: each item's box, where its text sits, and the gaps between the
// text of neighbouring items. Saves /tmp/status-bar.png. Usage: node checks/measure-status-bar.mjs

import { openWorkspace } from "../lib/open.mjs";

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(500);
const report = await page.evaluate(() => {
  const bar = document.querySelector("footer");
  const barBox = bar.getBoundingClientRect();
  const items = [...bar.children].filter((el) => el.textContent.trim() && !el.className.includes("flex-1"));
  const rows = items.map((el) => {
    const box = el.getBoundingClientRect();
    // The text's own box: the range around the item's text nodes.
    const range = document.createRange();
    range.selectNodeContents(el);
    const text = range.getBoundingClientRect();
    const style = getComputedStyle(el);
    return {
      text: el.textContent.trim().slice(0, 24),
      tag: el.tagName,
      boxTop: +(box.top - barBox.top).toFixed(2),
      boxH: +box.height.toFixed(2),
      textTop: +(text.top - barBox.top).toFixed(2),
      textH: +text.height.toFixed(2),
      textLeft: +text.left.toFixed(1),
      textRight: +text.right.toFixed(1),
      padX: style.paddingLeft,
      lineHeight: style.lineHeight,
    };
  });
  const gaps = rows.slice(1).map((r, i) => +(r.textLeft - rows[i].textRight).toFixed(1));
  return { barH: barBox.height, rows, textGaps: gaps };
});
console.log(JSON.stringify(report, null, 1));
await page.screenshot({ path: "/tmp/status-bar.png", clip: { x: 600, y: 760, width: 600, height: 40 } });
await browser.close();
