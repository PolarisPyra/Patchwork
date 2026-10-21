// Opens Settings and reports the background of the active nav row (Appearance) before and while
// it is hovered, so the hover highlight on the active row can be checked.
// Usage: node checks/settings-nav-hover.mjs [screenshot.png]

import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2] ?? "/tmp/settings-nav-hover.png";
const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.locator('button[aria-label="Settings"]').click();
const row = page.getByRole("button", { name: "Appearance", exact: true });
await row.waitFor({ timeout: 10000 });
const background = () => row.evaluate((el) => getComputedStyle(el).backgroundColor);
// Waits out the dialog's opening transition, with the pointer off the row.
await page.mouse.move(0, 0);
await page.waitForTimeout(800);
const idle = await background();
await row.screenshot({ path: out.replace(/\.png$/, "-idle.png") });
const classes = await row.getAttribute("class");
await row.hover();
await page.waitForTimeout(200);
const hovered = await background();
await page.screenshot({ path: out });
await browser.close();
console.log(JSON.stringify({ classes, idle, hovered, highlighted: idle !== hovered, screenshot: out }, null, 2));
