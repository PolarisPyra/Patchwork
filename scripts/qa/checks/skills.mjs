// Screenshots the composer's "$" skills menu, a picked skill's chip, and the Skills settings pane.
// Prints the "/" and "$" menus' rows, and the Markdown a picked skill sends.
// Usage: node checks/skills.mjs <menu.png> <pane.png> [chip.png]
import { openWorkspace } from "../lib/open.mjs";

const [menuOut, paneOut, chipOut] = process.argv.slice(2);
if (!menuOut || !paneOut) throw new Error("Usage: node checks/skills.mjs <menu.png> <pane.png>");

const { browser, page } = await openWorkspace();
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
const editor = page.locator(".ProseMirror").first();
await editor.waitFor({ timeout: 15000 });
const rows = async (label) => {
  const menu = page.locator(`[role="listbox"][aria-label="${label}"]`);
  await menu.waitFor({ timeout: 5000 });
  await page.waitForTimeout(300);
  return (await menu.getByRole("option").allInnerTexts()).map((r) => r.replace(/\s+/g, " "));
};

await editor.click();
await page.keyboard.type("/");
console.log(`"/" menu:\n  ${(await rows("Slash commands")).join("\n  ")}`);
await page.keyboard.press("Backspace");
await page.keyboard.type("$");
console.log(`"$" menu:\n  ${(await rows("Skills")).join("\n  ")}`);
await page.locator("main").first().screenshot({ path: menuOut });
await page.keyboard.type("rel");
await page.keyboard.press("Enter");
await page.keyboard.type("for v1.2");
const markdown = await editor.evaluate((el) => el.editor?.getMarkdown());
console.log(`picked "$rel", sends: ${JSON.stringify(markdown)}`);
const chip = page.locator(".ProseMirror [data-skill-command]");
console.log(`chip: ${await chip.count()} ${JSON.stringify(await chip.first().innerText())}`);
if (chipOut) await editor.screenshot({ path: chipOut });
await page.keyboard.press("Control+A");
await page.keyboard.press("Delete");

await page.locator('button[aria-label="Settings"]').first().click();
const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
await dialog.getByRole("button", { name: "Skills" }).click();
await page.waitForTimeout(200);
await dialog.screenshot({ path: paneOut });
await browser.close();
console.log(`saved ${menuOut} and ${paneOut}`);
