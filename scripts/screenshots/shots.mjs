// Renders the real app with canned data and saves the four README screenshots.
//
// It drives the built app, not the dev server: `pnpm --filter patchwork-web build` first, then
// `vite preview` on 1420. A dev build frames the window in hazard stripes, which do not belong in a
// screenshot of the app.
//
// Each shot is a fresh page, so one state cannot leak into the next. The viewport is the app's own
// default window size, 1600x960 (see apps/desktop/src-tauri/tauri.conf.json), at 2x for a crisp
// image on a high-density display. `stub.js` answers the backend, so no real project or chat is in
// the image.
//
// Usage, from the repo root:
//   pnpm --filter patchwork-web build
//   pnpm --filter patchwork-web exec vite preview --port 1420 --strictPort &
//   node scripts/screenshots/shots.mjs
//
// It needs Playwright, which the project does not depend on. `PW` names an install that has it.
const pw = await import(process.env.PW ?? "playwright");
const { chromium } = pw.default ?? pw;
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const stub = readFileSync(join(here, "stub.js"), "utf8");
const out = join(here, "..", "..", "docs", "screenshots");
const url = process.env.SHOTS_URL ?? "http://localhost:1420/";

/** Waits until the app has painted its first frame and the fonts have loaded. */
async function settle(page) {
  await page.waitForLoadState("networkidle");
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(900);
}

async function shot(browser, { name, mode = "goal", act }) {
  const page = await browser.newPage({
    viewport: { width: 1600, height: 960 },
    deviceScaleFactor: 2,
  });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e).slice(0, 200)));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text().slice(0, 200));
  });
  await page.addInitScript({ content: `window.__SHOT_MODE__ = ${JSON.stringify(mode)};` });
  await page.addInitScript({ content: stub });
  await page.goto(url, { waitUntil: "networkidle" });
  await settle(page);
  if (act) await act(page);
  await settle(page);
  await page.screenshot({ path: join(out, `${name}.png`) });
  console.log(`${name}.png  errors=${errors.length ? errors.slice(0, 3) : "none"}`);
  await page.close();
}

const browser = await chromium.launch();

// A turn still running, with each step in its work rows.
await shot(browser, { name: "grid-1" });

// An empty task, ready for a prompt.
await shot(browser, { name: "grid-2", mode: "new" });

// Settings, on General: the theme, app icons and chat backgrounds.
await shot(browser, {
  name: "grid-3",
  act: async (page) => {
    await page.locator('button[aria-label="Settings"]').first().click();
    await page.getByRole("button", { name: "General", exact: true }).first().click();
    await page.waitForTimeout(400);
  },
});

// A finished turn, with the subscription usage open over it.
await shot(browser, {
  name: "grid-4",
  act: async (page) => {
    await page.locator('[data-thread-id="t1"]').first().click();
    await page.waitForTimeout(600);
    await page.getByRole("button", { name: "Usage" }).first().click();
    await page.waitForTimeout(700);
  },
});

await browser.close();
