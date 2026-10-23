// Opens the Usage popover with sample readings for every agent, prints whether it
// scrolls, and screenshots it.
// Usage: node checks/usage-popover.mjs <out.png> [agent to select]
import { openWorkspace } from "../lib/open.mjs";

const [out, pick] = process.argv.slice(2);
if (!out) throw new Error("Usage: node checks/usage-popover.mjs <out.png> [agent]");

const { browser, page } = await openWorkspace({ height: 900 });
// Connects all four agents and answers usage reads with canned numbers, then reloads.
await page.addInitScript(() => {
  const ipc = window.__TAURI_INTERNALS__;
  const base = ipc.invoke;
  const h = 3600_000;
  const now = Date.now();
  const usage = {
    claude: { plan: "Max", windows: [["Session (5h)", 42, 2.2 * h], ["Weekly", 71, 70 * h], ["Weekly Opus", 93, 70 * h]] },
    codex: { plan: "Plus", windows: [["5h limit", 18, 3.5 * h], ["Weekly", 55, 100 * h]] },
    antigravity: { plan: null, windows: [["Daily", 8, 9 * h]] },
    opencode: { plan: null, windows: [] },
  };
  ipc.invoke = async (cmd, args) => {
    if (cmd === "get_settings") {
      const s = await base(cmd, args);
      return { ...s, agents: ["claude", "codex", "antigravity", "opencode"] };
    }
    if (cmd === "detect_agent") return { installed: true, signedIn: args.kind !== "opencode", version: "1.0.0" };
    if (cmd === "get_usage") {
      const u = usage[args.kind];
      const windows = u.windows.map(([label, usedPercent, inMs]) => ({ label, usedPercent, resetsAt: now + inMs }));
      return { kind: args.kind, plan: u.plan, windows, error: null, updatedAt: now };
    }
    return base(cmd, args);
  };
});
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Usage"]').click();
const dialog = page.locator('[role="dialog"][aria-label="Usage"]');
await dialog.waitFor();
if (pick) await dialog.getByRole("tab", { name: pick }).click();
await page.waitForTimeout(400);
// Gaps from the popover to the sidebar edges, which should match.
const box = await dialog.evaluate((el) => {
  const r = el.getBoundingClientRect();
  const side = el.closest("aside").getBoundingClientRect();
  return {
    width: r.width,
    height: r.height,
    scrolls: el.scrollHeight > el.clientHeight,
    leftGap: r.left - side.left,
    // The sidebar draws its own 1px right border, which the gap stops at.
    rightGap: side.right - 1 - r.right,
  };
});
console.log(box);
await page.screenshot({ path: out, clip: await dialog.evaluate((el) => {
  const r = el.getBoundingClientRect();
  return { x: Math.max(0, r.x - 16), y: Math.max(0, r.y - 16), width: r.width + 32, height: r.height + 80 };
}) });
await browser.close();
console.log(`saved ${out}`);
