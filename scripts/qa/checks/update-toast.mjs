// Sends the update statuses the backend reports and prints whether the top-right update card
// shows, with its text and button, for each. Then clicks its button and its close button, and
// prints the commands they invoked. Saves a screenshot per shown status as <out-prefix>-<status>.png.
// Usage: node checks/update-toast.mjs [out-prefix]
import { openWorkspace } from "../lib/open.mjs";

const [prefix = "/tmp/update-toast"] = process.argv.slice(2);

// Runs after the stub. Answers the update commands, records them, and captures the
// "update-status" listener so the check can send statuses.
const script = `(() => {
  localStorage.removeItem("patchwork:update-dismissed");
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
    if (cmd === "update_status") return { currentVersion: "0.6.5", canInstall: true, state: { status: "idle" } };
    if (cmd === "restart_to_update" || cmd === "open_url" || cmd.startsWith("plugin:dialog")) {
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
await page.waitForFunction(() => window.__qaUpdateHandler !== undefined);

const toast = page.locator('[role="status"]').filter({ hasText: "Patchwork 0.6.6" });
const states = [
  { status: "downloading", version: "0.6.6", downloaded: 1, total: 2 },
  { status: "available", version: "0.6.6" },
  { status: "ready", version: "0.6.6" },
];
for (const state of states) {
  const canInstall = state.status !== "available";
  await page.evaluate((s) => window.__qaEmit(s), { currentVersion: "0.6.5", canInstall, state });
  await page.waitForTimeout(100);
  if (!(await toast.count())) {
    console.log(`${state.status}: no card`);
    continue;
  }
  const box = await toast.boundingBox();
  const text = (await toast.innerText()).split("\n").join(" | ");
  console.log(`${state.status}: ${text} at x=${Math.round(box.x)} y=${Math.round(box.y)} w=${Math.round(box.width)}`);
  await page.screenshot({ path: `${prefix}-${state.status}.png` });
}

// Restart asks first while tasks run; the stub answers "no". Then the card is closed.
await toast.getByRole("button", { name: "Restart" }).click();
await page.waitForTimeout(200);
await toast.getByRole("button", { name: "Dismiss" }).click();
await page.waitForTimeout(100);
console.log(`after dismiss: ${(await toast.count()) ? "card shown" : "no card"}`);
const invokes = await page.evaluate(() => window.__qaInvokes);
console.log("invoked:", invokes.map((i) => `${i.cmd}${i.args && "force" in i.args ? `(force=${i.args.force})` : ""}`).join(", "));
await browser.close();
console.log(`saved ${prefix}-*.png`);
