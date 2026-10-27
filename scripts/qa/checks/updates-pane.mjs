// Walks the Updates settings pane through each update status the backend reports, and prints the
// status line and the button for each, and whether the update's notes show. Records the commands the button invokes, and saves a
// screenshot per status as <out-prefix>-<status>.png.
// Usage: node checks/updates-pane.mjs [out-prefix]
import { openWorkspace } from "../lib/open.mjs";

const [prefix = "/tmp/updates-pane"] = process.argv.slice(2);

// Runs after the stub. Answers the update commands, records them, and captures the
// "update-status" listener so the check can send statuses.
const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const callbacks = {};
  let next = 1;
  internals.transformCallback = (cb) => {
    const id = next++;
    callbacks[id] = cb;
    return id;
  };
  window.__qaInvokes = [];
  window.__qaEmit = (status) => callbacks[window.__qaUpdateHandler]({ payload: status });
  internals.invoke = async (cmd, args) => {
    if (cmd === "plugin:event|listen" && args && args.event === "update-status") window.__qaUpdateHandler = args.handler;
    if (cmd === "update_status") return { currentVersion: "0.6.3", canInstall: true, state: { status: "idle" }, notes: null };
    if (cmd === "check_for_updates" || cmd === "restart_to_update" || cmd.startsWith("plugin:dialog") || cmd.startsWith("plugin:opener")) {
      window.__qaInvokes.push({ cmd, args });
      if (cmd === "restart_to_update") return args.force ? { result: "restarting" } : { result: "busy", running: 2 };
      if (cmd.startsWith("plugin:dialog")) return false;
      return null;
    }
    return invoke(cmd, args);
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator('button[aria-label="Settings"]').first().click();
const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
await dialog.getByRole("button", { name: "Updates" }).click();
await page.waitForFunction(() => window.__qaUpdateHandler !== undefined);

const states = [
  { status: "idle" },
  { status: "checking" },
  { status: "upToDate" },
  { status: "downloading", version: "0.6.4", downloaded: 7_340_032, total: 18_874_368 },
  { status: "failed", message: "error sending request for url" },
  { status: "available", version: "0.6.4" },
  { status: "ready", version: "0.6.4" },
];
// The notes of the update the check found, as latest.json carries them.
const notes =
  "## [0.6.4] - 2026-10-28\n\n### Added\n\n- Settings → Skills now shows each skill as a card you can switch on or off.\n\n### Fixed\n\n- Chats no longer jump to the top when a reply finishes.\n";
const pane = dialog.locator("div.border-b").filter({ hasText: "Patchwork 0.6.3" });
for (const state of states) {
  const canInstall = state.status !== "available";
  await page.evaluate((s) => window.__qaEmit(s), {
    currentVersion: "0.6.3",
    canInstall,
    state,
    notes: state.status === "idle" ? null : notes,
  });
  await page.waitForTimeout(100);
  const button = pane.locator("button");
  const text = (await pane.innerText()).split("\n").join(" | ");
  const label = await button.innerText();
  const disabled = await button.isDisabled();
  const shown = (await dialog.getByText("0.6.4 · 2026-10-28").count()) > 0;
  console.log(`${state.status}: ${text}${disabled ? " (disabled)" : ""}; notes ${shown ? "shown" : "hidden"}`);
  if (state.status !== "idle" && !label) console.log("  no button");
  await dialog.screenshot({ path: `${prefix}-${state.status}.png` });
}

// The restart button asks first while tasks run; the stub answers the question with "no".
await pane.getByRole("button", { name: "Restart to update" }).click();
await page.waitForTimeout(200);
await page.evaluate((s) => window.__qaEmit(s), {
  currentVersion: "0.6.3",
  canInstall: true,
  state: { status: "upToDate" },
});
await pane.getByRole("button", { name: "Check for updates" }).click();
await page.waitForTimeout(100);
const invokes = await page.evaluate(() => window.__qaInvokes);
console.log("invoked:", invokes.map((i) => `${i.cmd}${i.args && "force" in i.args ? `(force=${i.args.force})` : ""}`).join(", "));
await browser.close();
console.log(`saved ${prefix}-*.png`);
