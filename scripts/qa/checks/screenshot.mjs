// Saves a screenshot of the whole workspace, or of one element when a selector is given.
// With --spinners, adds a second working chat outside the folder, then a moment after load
// collapses and expands the folder and opens its working chat, so those spinners start later than
// the other's, and prints where in its lap each spinner's arc is: in step, they all match.
// Usage: node checks/screenshot.mjs <out.png> [selector] [--spinners]
import { openWorkspace } from "../lib/open.mjs";

const args = process.argv.slice(2);
const spinners = args.includes("--spinners");
const [out, selector] = args.filter((a) => !a.startsWith("--"));
if (!out) throw new Error("Usage: node checks/screenshot.mjs <out.png> [selector] [--spinners]");

// A copy of the working chat at the top level, whose spinner shows from the start.
const second = `(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const result = await invoke(cmd, args);
    if (cmd !== "list_threads") return result;
    const working = result.find((t) => t.id === "qa-working");
    return [...result, { ...working, id: "qa-working-2", title: "Second working chat", folderId: null }];
  };
})();`;

const { browser, page } = await openWorkspace(spinners ? { init: second } : {});
if (spinners) {
  const folder = page.locator("aside").getByText("Old", { exact: true });
  await page.waitForTimeout(537);
  await folder.click();
  await page.waitForTimeout(311);
  await folder.click();
  await page.waitForTimeout(173);
  await page.locator("aside div.group.cursor-pointer", { hasText: "Refactor the diff view" }).click();
  await page.waitForTimeout(500);
  const phases = await page.evaluate(() =>
    // A hidden spinner has no animation running.
    [...document.querySelectorAll(".spinner-arc")].flatMap((arc) =>
      arc.getAnimations().map((animation) => Math.round(Number(animation.currentTime) % 800)),
    ),
  );
  console.log(`spinner phases (ms into an 800 ms lap): ${phases.join(", ")}`);
}
if (selector) await page.locator(selector).first().screenshot({ path: out });
else await page.screenshot({ path: out });
await browser.close();
console.log(`saved ${out}`);
