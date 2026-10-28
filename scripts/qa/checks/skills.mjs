// Screenshots the composer's "$" skills menu, a picked skill's chip, and the Skills settings pane.
// Prints the "/" and "$" menus' rows, and the Markdown a picked skill sends. With the last two
// paths, does the same for the "&" agents menu (with the project's own agents), the chat's agent
// in the composer, and the Custom agents pane, where it also opens and cancels a new agent. With --sigils=<png>,
// rebinds the skills sigil to "!" in Settings → Shortcuts → Sigils, screenshots that tab, and
// prints what "!" and "$" do in the composer before resetting it.
// Usage: node checks/skills.mjs <menu.png> <pane.png> [chip.png] [agents.png agents-pane.png]
//   [--sigils=<sigils.png>]
import { openWorkspace } from "../lib/open.mjs";

const args = process.argv.slice(2);
const sigilsOut = args.find((a) => a.startsWith("--sigils="))?.slice("--sigils=".length);
const [menuOut, paneOut, chipOut, agentsOut, agentsPaneOut] = args.filter((a) => !a.startsWith("--"));
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
  const role = page.locator(`main [title="This chat's agent"]`);
  console.log(`chat's agent: ${(await role.count()) ? JSON.stringify(await role.innerText()) : "none"}`);
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
  await dialog.getByRole("button", { name: "New agent" }).click();
  await page.keyboard.press("Escape");
  console.log(`Esc in the new agent's name keeps Settings open: ${await dialog.isVisible()}`);
  await dialog.getByRole("button", { name: "New agent" }).click();
  await page.keyboard.type("reviewer");
  await dialog.screenshot({ path: agentsPaneOut });
  await page.keyboard.press("Enter");
  await page.waitForTimeout(200);
  console.log(`after Create, the card is back: ${await dialog.getByRole("button", { name: "New agent" }).isVisible()}`);
}
if (sigilsOut) {
  await dialog.getByRole("button", { name: "Shortcuts" }).click();
  await dialog.getByRole("button", { name: "Sigils", exact: true }).click();
  await dialog.getByRole("button", { name: "Change sigil for Summon an agent" }).click();
  await page.keyboard.press("@");
  console.log(`"@" for agents: ${JSON.stringify(await dialog.locator(".text-danger").innerText())}`);
  await page.keyboard.press("Escape");
  await dialog.getByRole("button", { name: "Change sigil for Use a skill" }).click();
  await page.keyboard.press("!");
  await page.waitForTimeout(200);
  await dialog.screenshot({ path: sigilsOut });
  console.log(`saved: ${await page.evaluate(() => localStorage.getItem("patchworkSigils"))}`);
  await dialog.getByRole("button", { name: "Close" }).click();
  await editor.click();
  await page.keyboard.type("!");
  console.log(`"!" menu:\n  ${(await rows("Skills")).join("\n  ")}`);
  await page.keyboard.type("rel");
  await page.keyboard.press("Enter");
  await page.keyboard.type("now !release-notes ");
  const sent = await editor.evaluate((el) => el.editor?.getMarkdown());
  console.log(`picked "!rel" and typed "!release-notes ", sends: ${JSON.stringify(sent)}`);
  await page.keyboard.press("Control+A");
  await page.keyboard.press("Delete");
  await page.keyboard.type("$");
  await page.waitForTimeout(300);
  console.log(`"$" opens a menu: ${await page.locator('[role="listbox"]').count() > 0}`);
  await page.keyboard.press("Backspace");
  await page.locator('button[aria-label="Settings"]').first().click();
  await dialog.getByRole("button", { name: "Shortcuts" }).click();
  await dialog.getByRole("button", { name: "Sigils", exact: true }).click();
  await dialog.getByRole("button", { name: "Change sigil for Run a command" }).click();
  await page.keyboard.press(";");
  await dialog.getByRole("button", { name: "Change sigil for Mention a file or folder" }).click();
  await page.keyboard.press("#");
  console.log(`saved: ${await page.evaluate(() => localStorage.getItem("patchworkSigils"))}`);
  await dialog.getByRole("button", { name: "Close" }).click();
  await editor.click();
  await page.keyboard.type(";go");
  console.log(`";go" menu:\n  ${(await rows("Slash commands")).join("\n  ")}`);
  await page.keyboard.press("Enter");
  const goal = await editor.evaluate((el) => el.editor?.getMarkdown());
  console.log(`picked ";go", sends: ${JSON.stringify(goal)}`);
  await page.keyboard.press("Control+A");
  await page.keyboard.press("Delete");
  for (const sigil of ["@", "#"]) {
    await page.keyboard.type(`see ${sigil}`);
    await page.waitForTimeout(300);
    const files = page.locator("main .bottom-full button");
    const shown = (await files.count()) + Number(await page.getByText("No matching files").isVisible());
    console.log(`"${sigil}" opens the file menu: ${shown > 0}`);
    if (shown > 0) {
      await page.keyboard.press("Enter");
      console.log(`picked a file, sends: ${JSON.stringify(await editor.evaluate((el) => el.editor?.getMarkdown()))}`);
    }
    await page.keyboard.press("Control+A");
    await page.keyboard.press("Delete");
  }
  await page.locator('button[aria-label="Settings"]').first().click();
  await dialog.getByRole("button", { name: "Shortcuts" }).click();
  await dialog.getByRole("button", { name: "Reset all" }).click();
  console.log(`after Reset all: ${await page.evaluate(() => localStorage.getItem("patchworkSigils"))}`);
}
await browser.close();
console.log(`saved ${menuOut} and ${paneOut}`);
