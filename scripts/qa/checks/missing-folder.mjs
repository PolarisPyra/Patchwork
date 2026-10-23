// Reports the project's folder as missing, then saves a screenshot of the composer with the
// prompt above it. Usage: node checks/missing-folder.mjs <out.png>
import { openWorkspace } from "../lib/open.mjs";

const [out = "/tmp/missing-folder.png"] = process.argv.slice(2);
// Runs after the stub, so it wraps the stub's invoke.
const script = `(() => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) =>
    cmd === "is_folder" ? false : invoke(cmd, args);
  const list = window.__TAURI_INTERNALS__.invoke;
  const goal = { objective: "Keep the stripe aligned", status: "active", startedAt: Date.now(), checks: null, tokensUsed: null, tokenBudget: null, timeUsedSeconds: null };
  // A goal on the open task puts the goal banner next to the prompt, to compare them.
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const r = await list(cmd, args);
    if (cmd === "list_threads") return r.map((t) => ({ ...t, goal }));
    if (cmd === "get_thread" && r) return { ...r, goal };
    return r;
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });

const prompt = page.getByRole("button", { name: "Change folder" });
await prompt.waitFor({ timeout: 5000 });
const box = await page.evaluate(() => {
  const button = [...document.querySelectorAll("button")].find((b) => b.textContent === "Change folder");
  const card = button.parentElement.getBoundingClientRect();
  const composer = button.closest("div.mx-auto, main")?.getBoundingClientRect();
  return { card: { x: card.x, width: card.width, height: card.height }, composerX: composer?.x };
});
console.log(JSON.stringify(box));
// The composer should take neither the caret nor typing, and Send should stay off.
await page.locator(".composer-editor").click({ force: true });
await page.keyboard.type("hello");
const composer = await page.evaluate(() => ({
  focused: document.activeElement?.closest(".composer-editor") !== null,
  editable: document.querySelector(".composer-editor [contenteditable]")?.getAttribute("contenteditable"),
  text: document.querySelector(".composer-editor")?.textContent,
  sendDisabled: document.querySelector('button[aria-label="Send"]')?.disabled,
}));
console.log(JSON.stringify(composer));
await page.screenshot({ path: out, clip: { x: 0, y: 400, width: 1200, height: 400 } });
console.log(`saved ${out}`);
await browser.close();
