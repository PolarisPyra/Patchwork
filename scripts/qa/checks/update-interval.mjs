// Steps the update check interval in the Updates settings pane through every value, printing the
// shown value and which buttons are disabled at each, and screenshots the pane.
// Usage: node checks/update-interval.mjs <out.png>
import { openWorkspace } from "../lib/open.mjs";

const [out] = process.argv.slice(2);
if (!out) throw new Error("Usage: node checks/update-interval.mjs <out.png>");

const { browser, page } = await openWorkspace();
await page.locator('button[aria-label="Settings"]').first().click();
const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
await dialog.getByRole("button", { name: "Updates" }).click();
const stepper = dialog.getByRole("group", { name: "Check for updates every" });
const less = stepper.getByRole("button", { name: "Less often" });
const more = stepper.getByRole("button", { name: "More often" });
const show = async () => {
  const value = await stepper.innerText();
  console.log(`${value} (less ${await less.isDisabled() ? "off" : "on"}, more ${await more.isDisabled() ? "off" : "on"})`);
};
await show();
await dialog.screenshot({ path: out });
for (const button of [more, less]) {
  while (!(await button.isDisabled())) {
    await button.click();
    await show();
  }
}
await browser.close();
console.log(`saved ${out}`);
