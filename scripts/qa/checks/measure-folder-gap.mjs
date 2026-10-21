// Clicks a project folder header and samples the top of the row below it each frame, so a jump
// at the click or a bounce during the expand/collapse animation shows up in the samples.
// Usage: node checks/measure-folder-gap.mjs

import { openWorkspace } from "../lib/open.mjs";

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.waitForTimeout(800);

const run = () =>
  page.evaluate(
    () =>
      new Promise((resolve) => {
        const rows = () => [...document.querySelectorAll("aside div.cursor-pointer")];
        const header = rows().find((el) => el.querySelector("span.truncate")?.textContent.trim() === "Old");
        const below = rows().find((el) => el.textContent.trim().startsWith("Fix the stripe"));
        const tops = [below.getBoundingClientRect().top];
        const headerTop = header.getBoundingClientRect().top;
        header.click();
        const start = performance.now();
        const frame = () => {
          tops.push(below.getBoundingClientRect().top);
          if (performance.now() - start < 600) requestAnimationFrame(frame);
          else resolve({ headerTop, tops: tops.map((t) => Math.round(t * 10) / 10) });
        };
        requestAnimationFrame(frame);
      }),
  );

const first = await run();
const second = await run();
await browser.close();
for (const [name, r] of [["collapse", first], ["expand", second]]) {
  const t = r.tops;
  const jumpAtClick = Math.round((t[1] - t[0]) * 10) / 10;
  console.log(`${name}: headerTop=${r.headerTop} rowTop before=${t[0]} after=${t[t.length - 1]} firstFrameDelta=${jumpAtClick}`);
  console.log(`  samples: ${t.join(" ")}`);
}
