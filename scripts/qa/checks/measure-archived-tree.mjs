// Compares where the Archived view's rows sit with the Tasks view's project tree, and saves a
// screenshot of each sidebar. Adds archived chats and tasks, in and out of a folder, to the stub.
// Usage: node scripts/qa/checks/measure-archived-tree.mjs [outDir]
import { openWorkspace } from "../lib/open.mjs";

const outDir = process.argv[2] ?? "/tmp";
const archived = (id, title, projectId, folderId) => ({
  id,
  projectId,
  title,
  agent: "claude",
  model: null,
  accessMode: "full-access",
  effort: null,
  sessionId: null,
  createdAt: Date.now() - 5000,
  messages: [],
  goal: null,
  folderId,
  archivedAt: Date.now() - 1000,
});
const extra = [
  archived("qa-arch-chat", "Archived chat", null, null),
  archived("qa-arch-folder", "Archived in folder", "qa-project", "qa-folder"),
  archived("qa-arch-top", "Archived at top level", "qa-project", null),
  archived("qa-arch-other", "Archived in Other", "qa-other", null),
];
const script = `(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const extra = ${JSON.stringify(extra)};
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const r = await invoke(cmd, args);
    return cmd === "list_threads" ? [...r, ...extra] : r;
  };
})();`;

// Each row's text and the left edge and height of its box.
const measure = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll("aside div.group.cursor-pointer")].map((r) => {
      const box = r.getBoundingClientRect();
      return `${r.textContent.trim().slice(0, 24).padEnd(24)} left=${Math.round(box.left)} h=${Math.round(box.height)}`;
    }),
  );

const { browser, page } = await openWorkspace();
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.waitForTimeout(300);
console.log("Tasks view:");
for (const line of await measure(page)) console.log(`  ${line}`);
await page.locator("aside").screenshot({ path: `${outDir}/tasks-tree.png` });
await page.locator('button[aria-label="Archived"]').click();
await page.waitForTimeout(300);
console.log("Archived view:");
for (const line of await measure(page)) console.log(`  ${line}`);
await page.locator("aside").screenshot({ path: `${outDir}/archived-tree.png` });
// The folder's menu, then the tree after Collapse all.
await page
  .locator("aside div.group.cursor-pointer", { hasText: /^Old$/ })
  .click({ button: "right" });
await page.waitForTimeout(200);
await page.screenshot({
  path: `${outDir}/archived-folder-menu.png`,
  clip: { x: 0, y: 0, width: 420, height: 400 },
});
await page.keyboard.press("Escape");
await page.getByText("Collapse all").click();
await page.waitForTimeout(300);
await page.locator("aside").screenshot({ path: `${outDir}/archived-collapsed.png` });
await browser.close();
console.log(
  `saved tasks-tree.png, archived-tree.png, archived-folder-menu.png and archived-collapsed.png in ${outDir}`,
);
