// Screenshots the API Providers settings pane: the provider list with the preset grid, and the editor
// for a DeepSeek provider with its models fetched, then a Custom provider with a logo picked.
// Prints the nav labels, the editor's fields, and the picked logo's size.
// Usage: node checks/api-providers-pane.mjs <out-prefix>
import { openWorkspace } from "../lib/open.mjs";

const [prefix] = process.argv.slice(2);
if (!prefix) throw new Error("Usage: node checks/api-providers-pane.mjs <out-prefix>");

const { browser, page } = await openWorkspace();
await page.locator('button[aria-label="Settings"]').first().click();
const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
console.log("nav:", (await dialog.locator("nav button").allInnerTexts()).join(" | "));
await dialog.getByRole("button", { name: "API Providers" }).click();
const preset = dialog.getByRole("button", { name: "DeepSeek", exact: true });
await preset.waitFor();
await dialog.screenshot({ path: `${prefix}-list.png` });
await preset.click();
await dialog.getByRole("button", { name: "Fetch models" }).click();
await dialog.getByRole("button", { name: "Headers" }).click();
await dialog.getByRole("button", { name: "Add header" }).click();
console.log(
  "fields:",
  (await dialog.locator("label > span").allInnerTexts()).join(" | "),
  "| key focused:",
  await page.evaluate(() => document.activeElement?.getAttribute("type")),
);
await dialog.screenshot({ path: `${prefix}-editor.png` });
await dialog.getByRole("button", { name: "Cancel" }).click();
await dialog.getByRole("button", { name: "Custom", exact: true }).click();
await dialog.screenshot({ path: `${prefix}-custom.png` });
await dialog
  .locator('input[type="file"]')
  .setInputFiles(new URL("../../../assets/claude-color.svg", import.meta.url).pathname);
const logo = dialog.getByRole("button", { name: "Change logo" });
await logo.waitFor();
await logo.hover();
console.log("logo:", JSON.stringify(await logo.locator("img").boundingBox()));
await dialog.screenshot({ path: `${prefix}-custom-logo.png` });
await browser.close();
console.log(`saved ${prefix}-*.png`);
