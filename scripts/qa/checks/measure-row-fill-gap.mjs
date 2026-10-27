// Measures the space between a selected chat row's fill and the hovered row's fill just below it,
// and screenshots the two. Adds two chats to the stub's QA project.
// Usage: node checks/measure-row-fill-gap.mjs [out.png]
import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2] ?? "row-fill-gap.png";
const chat = (id, title) => ({
  id,
  projectId: "qa-project",
  title,
  agent: "claude",
  model: null,
  accessMode: "full-access",
  effort: null,
  sessionId: null,
  createdAt: Date.now() - 2000,
  messages: [],
  goal: null,
  folderId: null,
  archivedAt: null,
});
const extra = [chat("qa-chat-1", "First chat"), chat("qa-chat-2", "Second chat")];
// Runs after the stub, so it wraps the stub's invoke: appends the chats to list_threads and
// answers get_thread for them, so selecting one opens it.
const init = `(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const extra = ${JSON.stringify(extra)};
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const own = cmd === "get_thread" && extra.find((t) => t.id === args.id);
    if (own) return own;
    const r = await invoke(cmd, args);
    return cmd === "list_threads" ? [...r, ...extra] : r;
  };
})();`;

const { browser, page } = await openWorkspace({ init });
const row = (text) => page.locator("aside div.group.cursor-pointer", { hasText: text }).first();
await row("First chat").click();
await row("Second chat").hover();
await page.waitForTimeout(300);

// The painted fill: the border box, less the borders when the background stops at the padding.
const fill = (text) =>
  row(text).evaluate((el) => {
    const box = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    const inset = s.backgroundClip === "padding-box";
    return {
      top: box.top + (inset ? parseFloat(s.borderTopWidth) : 0),
      bottom: box.bottom - (inset ? parseFloat(s.borderBottomWidth) : 0),
      rowTop: box.top,
      rowHeight: box.height,
      color: s.backgroundColor,
    };
  });
const first = await fill("First chat");
const second = await fill("Second chat");
const clip = {
  x: 0,
  y: first.rowTop - 6,
  width: 300,
  height: second.rowTop + second.rowHeight - first.rowTop + 12,
};
await page.screenshot({ path: out, clip });
await browser.close();

console.log(`selected  fill ${first.top}-${first.bottom}  row height ${first.rowHeight}  ${first.color}`);
console.log(`hovered   fill ${second.top}-${second.bottom}  row height ${second.rowHeight}  ${second.color}`);
console.log(`gap between fills: ${second.top - first.bottom}px  (rows ${second.rowTop - first.rowTop}px apart)`);
console.log(`screenshot: ${out}`);
