// Screenshots the Appearance settings pane in the dark and light themes, picks an app icon, and
// prints the selected icon and the size of each icon button.
// Usage: node checks/appearance-pane.mjs <out-prefix>
import { openWorkspace } from "../lib/open.mjs";

const [out] = process.argv.slice(2);
if (!out) throw new Error("Usage: node checks/appearance-pane.mjs <out-prefix>");

const { browser, page } = await openWorkspace();
await page.locator('button[aria-label="Settings"]').first().click();
const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
await dialog.getByRole("button", { name: "Appearance" }).click();
const icons = dialog.getByRole("radiogroup", { name: "App icon" }).getByRole("radio");
const selected = () => dialog.locator('[role="radio"][aria-checked="true"]').getAttribute("aria-label");
console.log(`icons: ${await icons.count()}, selected: ${await selected()}`);
const box = await icons.first().boundingBox();
console.log(`icon button: ${box.width}x${box.height}`);
await dialog.screenshot({ path: `${out}-dark.png` });
await icons.nth(2).click();
console.log(`selected after click: ${await selected()}`);
await dialog.getByRole("button", { name: "Light" }).click();
await dialog.screenshot({ path: `${out}-light.png` });
await browser.close();
console.log(`saved ${out}-dark.png and ${out}-light.png`);
