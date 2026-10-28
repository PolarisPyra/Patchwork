// Opens Settings and reports the background of the active nav row (Appearance) before and while
// it is hovered, so the hover highlight on the active row can be checked.
// With --select=<label>, instead reports another row's box and background while hovered, just
// after it is clicked, and once the pointer leaves, so a highlight that changes size on selection
// shows. --height=<px> sets the window's height; a short one makes the nav scroll.
// Usage: node checks/settings-nav-hover.mjs [screenshot.png] [--select=<label>] [--height=<px>]

import { openWorkspace } from "../lib/open.mjs";

const args = process.argv.slice(2);
const option = (name) => args.find((a) => a.startsWith(`--${name}=`))?.slice(name.length + 3);
const out = args.find((a) => !a.startsWith("--")) ?? "/tmp/settings-nav-hover.png";
const select = option("select");
const { browser, page } = await openWorkspace({ width: 1200, height: Number(option("height") ?? 800) });
await page.locator('button[aria-label="Settings"]').click();
const row = page.getByRole("button", { name: select ?? "Appearance", exact: true });
await row.waitFor({ timeout: 10000 });
const background = () => row.evaluate((el) => getComputedStyle(el).backgroundColor);
// Waits out the dialog's opening transition, with the pointer off the row.
await page.mouse.move(0, 0);
await page.waitForTimeout(800);

if (select) {
  const state = () =>
    row.evaluate((el) => {
      const { x, width, height } = el.getBoundingClientRect();
      const nav = el.closest("nav");
      return {
        x,
        width,
        height,
        background: getComputedStyle(el).backgroundColor,
        navScrollbar: nav ? nav.offsetWidth - nav.clientWidth - 1 : null,
      };
    });
  await row.hover();
  await page.waitForTimeout(200);
  const hovered = await state();
  await row.click();
  await page.waitForTimeout(200);
  const selected = await state();
  await page.screenshot({ path: out });
  await page.mouse.move(0, 0);
  await page.waitForTimeout(200);
  const left = await state();
  await browser.close();
  console.log(JSON.stringify({ hovered, selected, left, screenshot: out }, null, 2));
  process.exit(0);
}

const idle = await background();
await row.screenshot({ path: out.replace(/\.png$/, "-idle.png") });
const classes = await row.getAttribute("class");
await row.hover();
await page.waitForTimeout(200);
const hovered = await background();
await page.screenshot({ path: out });
await browser.close();
console.log(JSON.stringify({ classes, idle, hovered, highlighted: idle !== hovered, screenshot: out }, null, 2));
