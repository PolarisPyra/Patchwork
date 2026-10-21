// Screenshots the Notifications settings pane and prints its toggles and their states.
// Usage: node checks/notifications-pane.mjs <out.png>
import { openWorkspace } from "../lib/open.mjs";

const [out] = process.argv.slice(2);
if (!out) throw new Error("Usage: node checks/notifications-pane.mjs <out.png>");

const { browser, page } = await openWorkspace();
await page.locator('button[aria-label="Settings"]').first().click();
const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
await dialog.getByRole("button", { name: "Notifications" }).click();
for (const name of ["Goal completed", "Task completed"]) {
  const toggle = dialog.getByRole("switch", { name });
  console.log(`${name}: ${await toggle.getAttribute("aria-checked")}`);
  await toggle.click();
  console.log(`${name} after click: ${await toggle.getAttribute("aria-checked")}`);
}
await dialog.screenshot({ path: out });
await browser.close();
console.log(`saved ${out}`);
