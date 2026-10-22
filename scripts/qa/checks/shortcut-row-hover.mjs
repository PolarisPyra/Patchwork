// Opens Settings > Shortcuts and reports how far the key chips on every row, editable and fixed,
// sit from the row's end. Hovers one row for the highlight box. Saves /tmp/shortcut-row-hover.png. Usage: node checks/shortcut-row-hover.mjs

import { openWorkspace } from "../lib/open.mjs";

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.keyboard.press("Control+,");
await page.getByRole("button", { name: "Shortcuts" }).click();
const button = page.getByRole("button", { name: "Change shortcut for Search" });
await button.hover();
await page.waitForTimeout(200);
const box = await button.evaluate((el) => {
  const b = el.getBoundingClientRect();
  const chips = [...el.querySelectorAll("kbd")].map((k) => k.getBoundingClientRect());
  const first = chips[0];
  const last = chips.at(-1);
  return {
    box: { w: b.width, h: b.height },
    padLeft: first.left - b.left,
    padRight: b.right - last.right,
    padTop: first.top - b.top,
    padBottom: b.bottom - first.bottom,
  };
});
// Space between the last key chip and the end of the row, for every row; all should match.
const gapToRowEnd = await page.evaluate(() => [
  ...new Set(
    [...document.querySelectorAll("[role=dialog] section > div")].flatMap((row) => {
      const chips = row.querySelectorAll("kbd");
      if (!chips.length) return [];
      return [Math.round(row.getBoundingClientRect().right - chips[chips.length - 1].getBoundingClientRect().right)];
    }),
  ),
]);
console.log(JSON.stringify({ ...box, gapToRowEnd }));
await page.screenshot({
  path: "/tmp/shortcut-row-hover.png",
  clip: { x: 460, y: 280, width: 460, height: 90 },
});
await browser.close();
