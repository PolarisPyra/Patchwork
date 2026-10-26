// Opens a chat whose reply edited two files, one with changes left and one without, and deleted a
// tracked file and an untracked one. Screenshots the reply's file list, the Changes view pinned
// to it, and a file opened from an edit row in the timeline.
// Usage: node checks/reply-files.mjs [out-prefix]   (default /tmp/reply-files)

import { openWorkspace } from "../lib/open.mjs";

const prefix = process.argv[2] ?? "/tmp/reply-files";

const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const now = Date.now() - 600000;
  const tool = (id, summary, at) => ({ id, name: "Edit", summary, status: "done", subagent: null, parent: null, startedAt: at, endedAt: at + 2000 });
  const base = { plan: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null };
  const reply = {
    ...base, id: "r1", role: "assistant", status: "complete", createdAt: now, finishedAt: now + 90000,
    text: "Moved the background into the stylesheet.\\n\\nDone.",
    files: ["client/src/styles.css", "client/src/App.tsx", "client/src/legacy.css", "client/notes.txt"],
    tools: [tool("e1", "client/src/styles.css", now + 5000), tool("e2", "client/src/App.tsx", now + 9000)],
    parts: [
      { type: "text", text: "Moved the background into the stylesheet." },
      { type: "tool", id: "e1" },
      { type: "tool", id: "e2" },
      { type: "text", text: "\\n\\nDone." },
    ],
  };
  const user = { ...base, id: "u1", role: "user", text: "Fix the background", status: "complete", createdAt: now - 1000, finishedAt: now - 1000, files: [], tools: [], parts: [] };
  internals.invoke = async (cmd, args) => {
    const change = (path, additions, deletions, status = "modified") => ({ path, status, additions, deletions, staged: false });
    if (cmd === "git_changes") return [change("client/src/App.tsx", 4, 2), change("client/src/legacy.css", 0, 12, "deleted"), change("client/src/main.tsx", 78, 18), change("server/index.ts", 3, 104), change("README.md", 0, 0)];
    if (cmd === "missing_files") return args.paths.filter((p) => p === "client/notes.txt");
    if (cmd === "read_file") return "body {\\n  background: #111;\\n}\\n";
    if (cmd === "git_file_diff") return "";
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages: [user, reply] } : t));
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(1000);
await page.screenshot({ path: `${prefix}-reply.png` });

await page.getByRole("button", { name: "Show changes" }).last().click();
await page.waitForTimeout(400);
await page.screenshot({ path: `${prefix}-changes.png` });

await page.getByRole("button", { name: /Worked for/ }).click();
await page.waitForTimeout(300);
await page.getByRole("button", { name: "Changed 2 files" }).click();
await page.waitForTimeout(300);
await page.getByRole("button", { name: "Changed client/src/styles.css" }).click();
await page.waitForTimeout(600);
await page.screenshot({ path: `${prefix}-opened.png` });

await browser.close();
console.log(`${prefix}-reply.png ${prefix}-changes.png ${prefix}-opened.png`);
