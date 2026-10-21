// Measures the selected row in the Tasks tree: its fill height and the space above and below its text.
// Usage: node checks/measure-tree-selection.mjs

import { openWorkspace } from "../lib/open.mjs";

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.waitForTimeout(800);

const result = await page.evaluate(() => {
  const rows = [...document.querySelectorAll("aside div.cursor-pointer")];
  const row = rows.find((el) => getComputedStyle(el).backgroundColor !== "rgba(0, 0, 0, 0)");
  if (!row) return null;
  const box = row.getBoundingClientRect();
  const text = row.querySelector("span.truncate, span.text-ellipsis")?.getBoundingClientRect();
  if (!text) return { height: box.height };
  return {
    text: row.textContent.trim(),
    height: Math.round(box.height * 10) / 10,
    above: Math.round((text.top - box.top) * 10) / 10,
    below: Math.round((box.bottom - text.bottom) * 10) / 10,
  };
});
await browser.close();
console.log(result ? JSON.stringify(result) : "no selected row found");
