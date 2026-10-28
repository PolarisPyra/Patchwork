// Shows "$skill" chips in a user message, at its start and later in the text, and prints each
// chip's height next to the bubble's line height. Then types "/" and "$" words later in a draft:
// prints whether each menu opens, the draft a pick leaves, and the chip a typed "$name " becomes.
// Then "hello /comp" and "hello /nope": what "/" offers mid-draft, and the draft picking leaves.
// Usage: node checks/inline-commands.mjs [bubble.png] [composer.png]

import { openWorkspace } from "../lib/open.mjs";

const bubbleOut = process.argv[2] ?? "/tmp/inline-bubble.png";
const composerOut = process.argv[3] ?? "/tmp/inline-composer.png";
const text =
  "$release-notes only use learnrea / ghidra mcp or other tools when needed to make a fully offline patch, and use the $release-notes skill for the client side only, never the server side code";

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
// Puts the user message ahead of the stub thread's reply.
await page.addInitScript((text) => {
  const ipc = window.__TAURI_INTERNALS__;
  const invoke = ipc.invoke;
  ipc.invoke = async (cmd, args) => {
    const result = await invoke(cmd, args);
    if (cmd !== "get_thread" || !result) return result;
    const reply = result.messages[0];
    const user = { ...reply, id: "qa-user", role: "user", text, status: "complete", createdAt: reply.createdAt - 1000 };
    return { ...result, messages: [user, ...result.messages] };
  };
}, text);
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
const bubble = page.locator("div.rounded-\\[12px\\]", { hasText: "offline patch" }).first();
await bubble.waitFor({ timeout: 15000 });
const chips = await bubble.locator("span.bg-success\\/25").evaluateAll((spans) =>
  spans.map((s) => ({ text: s.textContent, height: Math.round(s.getBoundingClientRect().height) })),
);
console.log("bubble chips:", JSON.stringify(chips));
console.log("line-height:", await bubble.locator("p").first().evaluate((p) => getComputedStyle(p).lineHeight));
await bubble.screenshot({ path: bubbleOut });

const editor = page.locator(".ProseMirror").first();
await editor.click();
const menu = async (label) => {
  await page.waitForTimeout(300);
  return page.locator(`[role="listbox"][aria-label="${label}"]`).count();
};
await page.keyboard.type("please use the $re");
console.log(`"$re" later in the draft opens the skills menu: ${(await menu("Skills")) > 0}`);
await page.keyboard.press("Enter");
await page.keyboard.type("then /home/user/x");
console.log(`a path later in the draft opens a menu: ${(await menu("Slash commands")) > 0}`);
await page.keyboard.type(" and $release-notes again");
const markdown = await editor.evaluate((el) => el.editor?.getMarkdown());
console.log(`draft: ${JSON.stringify(markdown)}`);
console.log(`composer chips: ${await page.locator(".ProseMirror [data-skill-command]").count()}`);
await editor.screenshot({ path: composerOut });
await page.keyboard.press("Control+A");
await page.keyboard.press("Delete");
// "/" opens its menu later in a draft too, and a local command picked there becomes the whole draft.
await page.keyboard.type("hello /comp");
await page.waitForTimeout(300);
const offered = await page.locator('[role="listbox"][aria-label="Slash commands"] [role="option"]').allTextContents();
console.log(`"hello /comp" offers: ${JSON.stringify(offered.map((o) => o.trim().split(/\s/)[0]))}`);
await page.keyboard.press("Enter");
await page.waitForTimeout(200);
console.log(`draft after picking: ${JSON.stringify(await editor.evaluate((el) => el.editor?.getMarkdown()))}`);
await page.keyboard.press("Control+A");
await page.keyboard.press("Delete");
await page.keyboard.type("hello /nope");
console.log(`"hello /nope" opens a menu: ${(await menu("Slash commands")) > 0}`);
await page.keyboard.press("Control+A");
await page.keyboard.press("Delete");
await browser.close();
console.log(`saved ${bubbleOut} and ${composerOut}`);
