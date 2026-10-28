// Screenshots the composer's "$" skills menu, a picked skill's chip, and the Skills settings pane.
// Prints the "/" and "$" menus' rows, and the Markdown a picked skill sends. With the last two
// paths, does the same for the "&" agents menu and the Custom agents pane.
// Usage: node checks/skills.mjs <menu.png> <pane.png> [chip.png] [agents.png agents-pane.png]
import { openWorkspace } from "../lib/open.mjs";

const [menuOut, paneOut, chipOut, agentsOut, agentsPaneOut] = process.argv.slice(2);
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

if (agentsOut) {
  await page.keyboard.type("&");
  console.log(`"&" menu:\n  ${(await rows("Agents")).join("\n  ")}`);
  await page.keyboard.type("dev");
  await page.keyboard.press("Enter");
  await page.keyboard.type("add a login page");
  await page.locator("main").first().screenshot({ path: agentsOut });
  const sent = await editor.evaluate((el) => el.editor?.getMarkdown());
  console.log(`picked "&dev", sends: ${JSON.stringify(sent)}`);
  const agentChip = page.locator(".ProseMirror [data-agent-command]");
  console.log(`agent chip: ${await agentChip.count()} ${JSON.stringify(await agentChip.first().innerText())}`);
  await page.keyboard.press("Control+A");
  await page.keyboard.press("Delete");
  await page.keyboard.type("ask &doc-scout ");
  const typed = page.locator(".ProseMirror [data-agent-command]");
  console.log(`typed "&doc-scout ": ${await typed.count()} chip(s)`);
  await page.keyboard.press("Control+A");
  await page.keyboard.press("Delete");
}

await page.locator('button[aria-label="Settings"]').first().click();
const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
await dialog.getByRole("button", { name: "Skills" }).click();
await page.waitForTimeout(200);
await dialog.screenshot({ path: paneOut });
if (agentsPaneOut) {
  await dialog.getByRole("button", { name: "Custom agents" }).click();
  await page.waitForTimeout(200);
  await dialog.screenshot({ path: agentsPaneOut });
}
await browser.close();
console.log(`saved ${menuOut} and ${paneOut}`);
