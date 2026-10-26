// Opens the Changes view pinned to a reply whose files have short and long names, and measures
// each row: where its folder starts, and whether its name runs into its counts. Screenshots the
// sidebar at its width, then, when it has a resize handle, dragged narrow and wide.
// Usage: node checks/sidebar-reply-files.mjs [out-prefix]   (default /tmp/sidebar-reply-files)

import { openWorkspace } from "../lib/open.mjs";

const prefix = process.argv[2] ?? "/tmp/sidebar-reply-files";

const files = [
  "apps/server/src/import/mod.rs",
  "apps/server/src/lib.rs",
  "apps/server/src/orchestration/turn.rs",
  "apps/server/src/persistence/migrations/0019_message_turn.sql",
  "apps/web/src/components/chat/Messages.tsx",
  "apps/web/src/hooks/useTurnFiles.ts",
  "scripts/qa/checks/turn-files.mjs",
];

const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const now = Date.now() - 600000;
  const base = { plan: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null, tools: [] };
  const files = ${JSON.stringify(files)};
  const reply = { ...base, id: "r1", role: "assistant", status: "complete", createdAt: now, finishedAt: now + 90000, text: "Done.", files, parts: [{ type: "text", text: "Done." }] };
  const user = { ...base, id: "u1", role: "user", text: "Go", status: "complete", createdAt: now - 1000, finishedAt: now - 1000, files: [], parts: [] };
  internals.invoke = async (cmd, args) => {
    if (cmd === "git_changes") return files.slice(0, 6).map((path, i) => ({ path, status: "modified", additions: [1, 22, 131, 5, 68, 36][i], deletions: [0, 1, 4, 0, 25, 0][i], staged: false }));
    if (cmd === "missing_files") return [];
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages: [user, reply] } : t));
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(800);
await page.getByRole("button", { name: "Show changes" }).last().click();
await page.waitForTimeout(500);

const measure = (label) =>
  page.evaluate(
    ({ files, label }) => {
      const aside = document.querySelector("aside");
      const lines = [`${label}: sidebar ${Math.round(aside.getBoundingClientRect().width)}px`];
      for (const path of files) {
        const row = aside.querySelector(`button[title="${path}"]`);
        if (!row) continue;
        const name = path.split("/").pop();
        const dir = path.slice(0, path.lastIndexOf("/"));
        const spans = [...row.querySelectorAll("span")];
        const nameEl = spans.find((s) => s.textContent === name);
        const dirEl = spans.find((s) => s.textContent === dir);
        const tail = spans.find((s) => /^[+]|^No changes$|^Deleted$/.test(s.textContent ?? ""));
        const n = nameEl.getBoundingClientRect();
        const d = dirEl?.getBoundingClientRect();
        const t = tail?.getBoundingClientRect();
        const clipped = nameEl.scrollWidth > nameEl.clientWidth + 1;
        const overlap = t && Math.round(n.left + Math.min(nameEl.scrollWidth, n.width) - t.left);
        lines.push(
          `  ${name.padEnd(24)} folder at ${d && d.width > 0 ? Math.round(d.left) : "hidden"}` +
            `${clipped ? ", name clipped" : ""}${overlap > 0 ? `, name overlaps counts by ${overlap}px` : ""}`,
        );
      }
      return lines.join("\n");
    },
    { files, label },
  );

console.log(await measure("default"));
await page.locator("aside").first().screenshot({ path: `${prefix}-default.png` });

const handle = page.locator('[aria-label="Resize sidebar"]');
if (await handle.count()) {
  for (const width of [200, 420]) {
    const box = await handle.boundingBox();
    const asideBox = await page.locator("aside").first().boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + 200);
    await page.mouse.down();
    await page.mouse.move(asideBox.x + width, box.y + 200, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(200);
    console.log(await measure(`dragged to ${width}`));
    await page.locator("aside").first().screenshot({ path: `${prefix}-${width}.png` });
  }
}

await browser.close();
console.log(`screenshots: ${prefix}-*.png`);
