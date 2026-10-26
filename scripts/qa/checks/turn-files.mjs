// Opens a chat whose reply has snapshots of its turn, after some of its files were committed,
// one reverted, one deleted, and one left untouched. Prints the reply's file list and
// screenshots it and a committed file's diff opened from it.
// Usage: node checks/turn-files.mjs [out-prefix]   (default /tmp/turn-files)

import { openWorkspace } from "../lib/open.mjs";

const prefix = process.argv[2] ?? "/tmp/turn-files";

const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const now = Date.now() - 600000;
  const base = { plan: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null, tools: [] };
  const files = ["src/app.rs", "src/skills.rs", "src/old.rs", "README.md", "src/lib.rs", "Cargo.toml"];
  const reply = {
    ...base, id: "r1", role: "assistant", status: "complete", createdAt: now, finishedAt: now + 90000,
    text: "Added skills.", files, parts: [{ type: "text", text: "Added skills." }],
    turn: { before: "b".repeat(40), after: "a".repeat(40) },
  };
  const user = { ...base, id: "u1", role: "user", text: "Add skills", status: "complete", createdAt: now - 1000, finishedAt: now - 1000, files: [], parts: [] };
  const turnFile = (path, additions, deletions, state) => ({ path, additions, deletions, state });
  internals.invoke = async (cmd, args) => {
    if (cmd === "git_changes") return [{ path: "src/lib.rs", status: "modified", additions: 9, deletions: 1, staged: false }];
    if (cmd === "turn_files") return [
      turnFile("src/app.rs", 40, 6, "committed"),
      turnFile("src/skills.rs", 180, 0, "committed"),
      turnFile("src/old.rs", 0, 52, "deleted"),
      turnFile("README.md", 3, 1, "reverted"),
      turnFile("src/lib.rs", 2, 0, "uncommitted"),
      turnFile("Cargo.toml", 0, 0, "unchanged"),
    ].filter((f) => args.paths.includes(f.path));
    if (cmd === "turn_file_diff") return "diff --git a/src/app.rs b/src/app.rs\\n--- a/src/app.rs\\n+++ b/src/app.rs\\n@@ -1,2 +1,3 @@\\n fn main() {\\n+    skills::init();\\n }\\n";
    if (cmd === "read_file") return "fn main() {\\n    skills::init();\\n}\\n";
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages: [user, reply] } : t));
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(1000);
const card = page.locator("div", { has: page.getByRole("button", { name: "Show changes" }) }).last();
console.log((await card.innerText()).replace(/\n+/g, " | "));
await card.screenshot({ path: `${prefix}-card.png` });

await page.getByRole("button", { name: /app\.rs/ }).first().click();
await page.waitForTimeout(600);
await page.screenshot({ path: `${prefix}-opened.png` });

await browser.close();
console.log(`${prefix}-card.png ${prefix}-opened.png`);
