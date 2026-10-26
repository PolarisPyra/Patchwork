// Measures how dense the sidebar tree is: for a chat, project, folder, and task row, the
// height, title font size, title indent, the width left for the title, and how many trailing
// items show without hovering.
// Usage: node scripts/qa/checks/measure-tree-density.mjs [screenshot.png]

import { openWorkspace } from "../lib/open.mjs";

const out = process.argv[2];
// The stub has no chats, so one is added.
const init = `
  const invoke = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const result = await invoke(cmd, args);
    if (cmd !== "list_threads") return result;
    return [...result, { ...result[0], id: "qa-chat", projectId: null, folderId: null, title: "Explain the release workflow" }];
  };
`;
const { browser, page } = await openWorkspace({ width: 1200, height: 800, init });
await page.waitForTimeout(800);
// Keeps the pointer off the tree, so only the always-visible items count.
await page.mouse.move(1100, 700);

const result = await page.evaluate(() => {
  const aside = document.querySelector("aside");
  const left = aside.getBoundingClientRect().left;
  const visible = (el) => getComputedStyle(el).display !== "none" && el.getBoundingClientRect().width > 0;
  const describe = (row, title) => {
    const box = row.getBoundingClientRect();
    const text = title.getBoundingClientRect();
    const trailing = [...row.children].filter((c) => c !== title && visible(c) && c.getBoundingClientRect().left > text.left);
    return {
      height: Math.round(box.height * 10) / 10,
      fontSize: getComputedStyle(title).fontSize,
      titleIndent: Math.round(text.left - left),
      titleWidth: Math.round(text.width),
      rowWidth: Math.round(box.width),
      trailingVisible: trailing.length,
    };
  };
  const chat = document.querySelector('aside [data-thread-id="qa-chat"]');
  const thread = document.querySelector('aside [data-thread-id="qa-working"]');
  const threadTitle = thread?.querySelector("span");
  const headers = [...aside.querySelectorAll("div.cursor-pointer")].filter((r) => r.querySelector("svg") && !r.dataset.threadId);
  const project = headers.find((r) => r.querySelector(".font-medium"));
  const folder = headers.find((r) => r !== project && r.querySelector("span.truncate"));
  return {
    sidebarWidth: Math.round(aside.getBoundingClientRect().width),
    chat: chat && describe(chat, chat.querySelector("span")),
    project: project && describe(project, project.querySelector(".font-medium")),
    folder: folder && describe(folder, folder.querySelector("span.truncate")),
    thread: thread && describe(thread, threadTitle),
  };
});
if (out) await page.locator("aside").first().screenshot({ path: out });
await browser.close();
console.log(JSON.stringify(result, null, 2));
