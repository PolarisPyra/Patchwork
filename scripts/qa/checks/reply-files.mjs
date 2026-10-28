// Opens a chat whose reply edited two files, one with changes left and one without, and deleted a
// tracked file and an untracked one. Screenshots the reply's file list, the Changes view pinned
// to it, and a file opened from an edit row in the timeline.
// With --media, the reply also produced a PNG, an SVG, and a Markdown file, and a PNG it then
// deleted. Screenshots their cards, then the PNG opened from its thumbnail in the canvas.
// Usage: node checks/reply-files.mjs [out-prefix] [--media]   (default research/reply-files)

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { openWorkspace } from "../lib/open.mjs";
import { installMocks, message } from "../lib/fixtures.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const media = args.includes("--media");
const prefix = args.find((a) => !a.startsWith("--")) ?? "research/reply-files";

// What the backend would report for the produced files. Images are drawn from real bytes, since
// the webview loads them from the URL that `convertFileSrc` returns.
const root = "/home/qa/project/";
const logo = media ? readFileSync(join(here, "../../../assets/logo.png")) : null;
const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><circle cx="32" cy="32" r="28" fill="#3574f0"/></svg>';
const produced = media
  ? {
      "client/public/logo.png": { size: logo.length, kind: "image", url: `data:image/png;base64,${logo.toString("base64")}` },
      "client/public/logo.svg": { size: svg.length, kind: "image", url: `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}` },
      "client/CHANGES.md": { size: 1234, kind: "other" },
      "client/public/old.png": { size: 0, kind: "image", gone: true },
    }
  : {};
const replyFiles = ["client/src/styles.css", "client/src/App.tsx", "client/src/legacy.css", "client/notes.txt", ...Object.keys(produced)];

const now = Date.now() - 600000;
const tool = (id, summary, at) => ({ id, name: "Edit", summary, status: "done", subagent: null, parent: null, startedAt: at, endedAt: at + 2000 });
const reply = message({
  id: "r1", role: "assistant", status: "complete", createdAt: now, finishedAt: now + 90000,
  text: "Moved the background into the stylesheet.\n\nDone.",
  files: replyFiles,
  tools: [tool("e1", "client/src/styles.css", now + 5000), tool("e2", "client/src/App.tsx", now + 9000)],
  parts: [
    { type: "text", text: "Moved the background into the stylesheet." },
    { type: "tool", id: "e1" },
    { type: "tool", id: "e2" },
    { type: "text", text: "\n\nDone." },
  ],
});
const user = message({ id: "u1", role: "user", text: "Fix the background", status: "complete", createdAt: now - 1000, finishedAt: now - 1000 });

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await installMocks(page, {
  data: { produced, root },
  threads: { "qa-idle": [user, reply] },
  mocks: {
    git_changes: () => {
      const change = (path, additions, deletions, status = "modified") => ({ path, status, additions, deletions, staged: false });
      return [change("client/src/App.tsx", 4, 2), change("client/src/legacy.css", 0, 12, "deleted"), change("client/src/main.tsx", 78, 18), change("server/index.ts", 3, 104), change("README.md", 0, 0)];
    },
    missing_files: (args, data) => args.paths.filter((p) => p === "client/notes.txt" || data.produced[p]?.gone),
    file_media: (args, data) => args.paths.filter((p) => p in data.produced).map((p) => ({ path: p, absolute: data.root + p, size: data.produced[p].size, kind: data.produced[p].kind })),
    read_file: () => "body {\n  background: #111;\n}\n",
    git_file_diff: () => "",
  },
  convertFileSrc: (path, data) => data.produced[path.replace(data.root, "")]?.url ?? path,
});
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(1000);
if (media) {
  // The reply opens scrolled to its end; scroll up to the cards.
  await page.mouse.move(560, 300);
  await page.mouse.wheel(0, -400);
  await page.waitForTimeout(400);
}
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

const shots = [`${prefix}-reply.png`, `${prefix}-changes.png`, `${prefix}-opened.png`];
if (media) {
  await page.locator('button[title="client/public/logo.png"]').first().click();
  await page.waitForTimeout(600);
  await page.screenshot({ path: `${prefix}-image.png` });
  shots.push(`${prefix}-image.png`);
}

await browser.close();
console.log(shots.join(" "));
