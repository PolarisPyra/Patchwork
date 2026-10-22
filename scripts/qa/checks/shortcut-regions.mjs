// Opens a long chat and reports what the shortcuts do: Ctrl+A and Escape in each part of the UI,
// remapping a shortcut in Settings, and the composer's Up and Down on a multi-line draft.
// Usage: node checks/shortcut-regions.mjs

import { openWorkspace } from "../lib/open.mjs";

// Runs after the stub, so it wraps the stub's invoke and gives the idle chat a long history.
const script = `(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const now = Date.now();
  const messages = Array.from({ length: 30 }, (_, i) => ({
    id: "long" + i, role: i % 2 ? "assistant" : "user",
    text: i % 2 ? "Paragraph " + i + ". " + "Lorem ipsum dolor sit amet. ".repeat(4) : "Question " + i + "\\nsecond line\\nthird line",
    parts: [], createdAt: now - (30 - i) * 60000, finishedAt: now - (30 - i) * 60000 + 1000, status: "complete",
    tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null,
  }));
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, messages } : t));
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
const openChat = async () => {
  await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
  await page.waitForTimeout(800);
};
await openChat();

const state = () =>
  page.evaluate(() => ({
    selected: window.getSelection()?.toString().length ?? 0,
    focus: document.activeElement?.closest("[data-shortcut-context]")?.getAttribute("data-shortcut-context") ?? document.activeElement?.tagName,
  }));
const report = {};

// A click in the transcript focuses its region; Ctrl+A selects its text, Escape clears it.
await page.mouse.click(600, 300);
await page.keyboard.press("Control+a");
report.transcriptCtrlA = await state();
await page.keyboard.press("Escape");
report.transcriptEscape = await state();

// In the composer, Ctrl+A is the editor's own: only the typed text is selected.
const composer = page.locator(".composer-editor .ProseMirror");
await composer.click();
await page.keyboard.type("hello");
await page.keyboard.press("Control+a");
report.composerCtrlA = { ...(await state()), selectedText: await page.evaluate(() => window.getSelection()?.toString()) };

// In the sidebar tree, Ctrl+A picks rows and leaves text selection alone.
await openChat();
await page.keyboard.press("Control+a");
report.sidebarCtrlA = {
  ...(await state()),
  pickedRows: await page.locator("aside div.group.cursor-pointer[class*='tree-inactive-selected']").count(),
};

// Remapping: Settings > Shortcuts, record Ctrl+J for "Open or close search", then use it.
await page.keyboard.press("Control+,");
await page.getByRole("button", { name: "Shortcuts" }).click();
await page.getByRole("button", { name: "Change shortcut for Open or close search" }).click();
await page.keyboard.press("Control+j");
await page.screenshot({ path: "/tmp/shortcuts-pane.png" });
await page.keyboard.press("Escape"); // closes settings
await page.keyboard.press("Control+j");
const paletteAfterNewKey = await page.locator("input[placeholder]").count();
await page.keyboard.press("Escape");
await page.keyboard.press("Control+k");
const paletteAfterOldKey = await page.locator('[role="dialog"], input').count();
report.remap = { paletteOpensWithCtrlJ: paletteAfterNewKey > 0, oldKeyOpensToo: paletteAfterOldKey > paletteAfterNewKey };
await page.keyboard.press("Escape");
// Put it back so the dev session keeps its keys.
await page.evaluate(() => localStorage.removeItem("patchworkKeybindings"));

// Composer arrows. The caret position and text come from the editor; a pause after each key lets
// the draft state settle, as a held key would.
const arrow = async (key) => {
  await page.keyboard.press(key);
  await page.waitForTimeout(150);
};
const caret = () =>
  composer.evaluate((el) => ({ at: el.editor.state.selection.from, text: el.editor.getText().slice(0, 11) }));
await page.reload({ waitUntil: "networkidle" });
await openChat();
await composer.click();
for (const [i, word] of ["one", "two", "three"].entries()) {
  if (i) await page.keyboard.press("Shift+Enter");
  await page.keyboard.type(word);
}
report.draftArrows = [await caret()];
for (const key of ["ArrowUp", "ArrowUp", "ArrowDown"]) {
  await arrow(key);
  report.draftArrows.push(await caret());
}

// A recalled multi-line message is walked line by line before the next one back is recalled.
await page.keyboard.press("Control+a");
await page.keyboard.press("Delete");
await arrow("ArrowUp");
report.recalled = [await caret()];
for (let i = 0; i < 4; i++) {
  await arrow("ArrowUp");
  report.recalled.push(await caret());
}
await arrow("ArrowDown");
report.recalled.push(await caret());

console.log(JSON.stringify(report, null, 2));
await browser.close();
