// Pastes text into the composer the ways other apps put it on the clipboard (plain text alone,
// plain text with the HTML of a rendered code block, a line naming a file and a URL, and terminal
// output with the styled monospace HTML Ghostty copies) and prints the editor's HTML and the
// Markdown it sends for each. Usage: node checks/composer-paste.mjs

import { openWorkspace } from "../lib/open.mjs";

const text = [
  "Do these steps in order:",
  "1. Read the file.",
  "2. Search the repo.",
  "3. Run `git status`.",
  "Don't edit any files.",
  "",
].join("\n");
const escaped = text.replace(/&/g, "&amp;").replace(/</g, "&lt;");
const lines = text.split("\n").map((l) => `<span class="line">${l.replace(/</g, "&lt;")}</span>`);
const cases = {
  "plain text": { "text/plain": text },
  "plain text again": { "text/plain": text },
  "code block html": {
    "text/plain": text,
    "text/html": `<pre><code>${escaped}</code></pre>`,
  },
  "highlighted code html": {
    "text/plain": text,
    "text/html": `<pre class="shiki"><code>${lines.join("\n")}</code></pre>`,
  },
  "one line with a file name and a url": {
    "text/plain": "run ./start.sh, then open https://example.com/docs",
  },
  "terminal html": {
    "text/plain": `~/project main > ./start.sh\n{\n  "region": {\n    "size": 49152\n  }\n}\n`,
    "text/html":
      '<div style="font-family: monospace; white-space: pre;">' +
      '<div style="display: inline; font-weight: bold;">~/project</div>' +
      '<div style="display: inline;"> main &gt; ./start.sh</div>\n{\n  "region": {\n    "size": 49152\n  }\n}\n</div>',
  },
};

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
const editor = page.locator(".ProseMirror").first();
await editor.waitFor({ timeout: 15000 });
for (const [name, data] of Object.entries(cases)) {
  await editor.click();
  await page.keyboard.press("Control+A");
  await page.keyboard.press("Delete");
  const html = await editor.evaluate((el, data) => {
    const dt = new DataTransfer();
    for (const [type, value] of Object.entries(data)) dt.setData(type, value);
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    // Tiptap keeps its editor on the editable element; the draft is sent as its Markdown.
    return new Promise((r) =>
      setTimeout(() => r({ html: el.innerHTML, markdown: el.editor?.getMarkdown() }), 200),
    );
  }, data);
  console.log(`== ${name}\n${html.html}\n-- markdown sent:\n${JSON.stringify(html.markdown)}\n`);
}
await browser.close();
