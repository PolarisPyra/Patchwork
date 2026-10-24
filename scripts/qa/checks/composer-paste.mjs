// Pastes text into the composer the ways other apps put it on the clipboard (plain text alone,
// plain text with the HTML of a rendered code block, a line naming a file and a URL, terminal
// output with the styled monospace HTML Ghostty copies, a web page, a VS Code copy, code copied
// from web pages and code views, and a text long enough to attach) and prints the editor's HTML, the Markdown it
// sends, and the attachments for each. Usage: node checks/composer-paste.mjs

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
  "web page html": {
    "text/plain": "Release notes\n\nFixed the paste.\nAdded a check.\n",
    "text/html":
      '<h2 style="color: red">Release notes</h2><p>Fixed the <b>paste</b>.<br>Added a ' +
      '<a href="https://example.com">check</a>.</p><script>alert(1)</script>',
  },
  "vs code copy": {
    "text/plain": "fn main() {\n    println!(\"hi\");\n}\n",
    "text/html":
      '<div style="font-family: monospace; white-space: pre;"><span>fn main() {</span></div>',
    "vscode-editor-data": JSON.stringify({ mode: "rust" }),
  },
  "docs page code block": {
    "text/plain": "pub fn root() -> String {\n    String::new()\n}",
    "text/html":
      '<pre style="color: rgb(0, 0, 0);"><code class="language-rust">pub fn root() -&gt; String {\n' +
      "    String::new()\n}</code></pre>",
  },
  "github code without its wrapper": {
    "text/plain": "def main():\n    return 1",
    "text/html":
      '<pre id="b"><span class="pl-k">def</span> <span class="pl-en">main</span>():\n' +
      '    <span class="pl-k">return</span> <span class="pl-c1">1</span></pre>',
  },
  "json from a code view": {
    "text/plain": '{\n  "identifier": "default",\n  "windows": [\n    "main"\n  ]\n}',
  },
  "short rust snippet": { "text/plain": "fn main() {\n    tauri_build::build()\n}" },
  "paragraph and code block together": {
    "text/plain": "Docs paragraph before.\n\npub fn root() -> String {\n    String::new()\n}",
    "text/html":
      '<p>Docs paragraph before.</p><pre><code class="language-rust">pub fn root() -&gt; String {\n' +
      "    String::new()\n}</code></pre>",
  },
  "long text": { "text/plain": "x".repeat(12_000) },
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
  const html = await paste(data);
  console.log(`== ${name}\n${html.html}\n-- markdown sent:\n${JSON.stringify(html.markdown)}\n`);
}
// A paste inside a code block goes in as copied, without starting another block.
await editor.click();
await page.keyboard.press("Control+A");
await page.keyboard.press("Delete");
await paste(cases["terminal html"]);
await page.locator(".ProseMirror pre").first().click();
await page.waitForTimeout(200);
const inBlock = await paste({ "text/plain": "added line\nand another" });
console.log(`== inside a code block\n-- markdown sent:\n${JSON.stringify(inBlock.markdown)}\n`);
const chips = await page.getByText("Pasted text.txt").count();
console.log(`== attachments\nPasted text.txt chips: ${chips}`);
await browser.close();

function paste(data) {
  return editor.evaluate((el, data) => {
    const dt = new DataTransfer();
    for (const [type, value] of Object.entries(data)) dt.setData(type, value);
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    // Tiptap keeps its editor on the editable element; the draft is sent as its Markdown.
    return new Promise((r) =>
      setTimeout(() => r({ html: el.innerHTML, markdown: el.editor?.getMarkdown() }), 300),
    );
  }, data);
}
