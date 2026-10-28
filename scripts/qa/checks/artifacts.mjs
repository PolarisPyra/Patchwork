// Draws an artifact reply: a Mermaid diagram and an HTML artifact, as any provider's reply would
// be rendered. Prints the diagram's size, whether the frame ran, and what copying the whole reply
// puts on the clipboard, then saves a screenshot.
// Usage: node checks/artifacts.mjs [out.png]  (default /tmp/artifacts.png)
import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2] ?? "/tmp/artifacts.png";

const reply = `Here is the flow, and a small artifact to go with it.

\`\`\`mermaid
flowchart LR
  A[Prompt] --> B{Tool needed?}
  B -- yes --> C[Run the tool]
  C --> B
  B -- no --> D[Answer]
\`\`\`

And an artifact:

\`\`\`html title=hello.html
<!doctype html>
<html><body style="font:14px system-ui;margin:0;padding:12px;background:#fff">
  <h3 style="margin:0 0 6px">Hello from an artifact</h3>
  <p>An HTML artifact runs in a sandboxed frame.</p>
</body></html>
\`\`\`
`;

// A thread of its own, so the reply is the one on screen when the check looks at it.
const init = `(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const now = Date.now();
  const thread = {
    id: "qa-artifacts", projectId: "qa-project", title: "Artifacts", agent: "claude", model: null,
    accessMode: "full-access", effort: null, sessionId: null, createdAt: now,
    messages: [{ id: "m-art", role: "assistant", text: ${JSON.stringify(reply)}, parts: [],
      createdAt: now, finishedAt: now, status: "complete", tools: [], plan: [], files: [],
      approvals: [], attachments: [], background: [], error: null, steer: false, queued: false,
      usage: null }],
    goal: null, folderId: null, archivedAt: null,
  };
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    if (cmd === "list_threads") return [thread, ...(await invoke(cmd, args))];
    if (cmd === "get_thread" && args.id === "qa-artifacts") return thread;
    return invoke(cmd, args);
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1100, height: 900, init });
await page.getByText("Artifacts", { exact: true }).first().click();
// Mermaid renders after its lazy import and a quiet delay.
await page.waitForSelector("div[data-code='mermaid'] svg, section svg", { timeout: 25000 });
await page.waitForTimeout(600);
// The chat follows a streaming reply to its end; scroll back so the whole diagram is in frame.
await page.locator("svg[id^='patchwork-diagram-']").first().scrollIntoViewIfNeeded();
await page.waitForTimeout(400);
const frames = page.locator("iframe[title='Artifact']");
const diagrams = page.locator("svg[id^='patchwork-diagram-']");
const box = await diagrams.first().boundingBox();
const frameBox = await frames.first().boundingBox();
// Copying the whole reply keeps each drawn block's source as a fence.
const copied = await page.evaluate(() => {
  const reply = document.querySelector("[data-artifact]")?.parentElement;
  if (!reply) return null;
  let text = null;
  const read = (e) => (text = e.clipboardData.getData("text/plain"));
  window.addEventListener("copy", read);
  const range = document.createRange();
  range.selectNodeContents(reply);
  getSelection().removeAllRanges();
  getSelection().addRange(range);
  document.execCommand("copy");
  window.removeEventListener("copy", read);
  getSelection().removeAllRanges();
  return text;
});
console.log(`copied: ${JSON.stringify(copied)}`);
await page.screenshot({ path: out });
await browser.close();
if (!box) throw new Error("The Mermaid diagram did not render");
if (!frameBox) throw new Error("The artifact frame did not render");
console.log(`diagram ${Math.round(box.width)}x${Math.round(box.height)}, frame ${Math.round(frameBox.width)}x${Math.round(frameBox.height)}`);
console.log(`saved ${out}`);
