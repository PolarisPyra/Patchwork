// Screenshots Settings → Plugins, Memory, Scheduled and Codex CLI, so each new pane can be looked
// at. Usage: node checks/new-panes.mjs [outDir]  (default /tmp/new-panes)
import { mkdirSync } from "node:fs";
import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2] ?? "/tmp/new-panes";
mkdirSync(out, { recursive: true });

const { browser, page } = await openWorkspace({ width: 1100, height: 820 });

async function pane(name, file) {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name, exact: true }).click();
  await page.waitForTimeout(350);
  await page.screenshot({ path: `${out}/${file}.png` });
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await page.waitForTimeout(150);
}

await pane("Plugins", "plugins");
await pane("Memory", "memory");
await pane("Scheduled", "scheduled");
await pane("Codex CLI", "codex");
// The screen tools are under Permissions, past the rules, so the pane is scrolled to them.
await page.getByRole("button", { name: "Settings", exact: true }).click();
await page.getByRole("button", { name: "Permissions", exact: true }).click();
await page.mouse.move(650, 500);
await page.mouse.wheel(0, 3000);
await page.waitForTimeout(300);
await page.screenshot({ path: `${out}/permissions-screen.png` });
await page.getByRole("button", { name: "Close", exact: true }).click();
await page.waitForTimeout(150);

// The editor, as a task is added: the form is the part that cannot be reached from a row.
await page.getByRole("button", { name: "Settings", exact: true }).click();
await page.getByRole("button", { name: "Scheduled", exact: true }).click();
await page.getByRole("button", { name: "New task" }).click();
await page.waitForTimeout(200);
await page.screenshot({ path: `${out}/scheduled-editor.png` });

// Memory shows the picked file beside the tree.
await page.getByRole("button", { name: "Close", exact: true }).click();
await page.getByRole("button", { name: "Settings", exact: true }).click();
await page.getByRole("button", { name: "Memory", exact: true }).click();
await page.getByRole("button", { name: "AGENTS.md", exact: false }).first().click();
await page.waitForTimeout(250);
await page.screenshot({ path: `${out}/memory-file.png` });

await browser.close();
console.log(`saved ${out}`);
