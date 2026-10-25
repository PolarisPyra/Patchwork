// Walks the CLI Providers settings pane through locating an agent CLI by hand: Codex not found,
// then a located file that is missing, then back to auto-detect, then a located file that runs.
// Prints the Codex row and the saved `agentPaths` for each step, and saves a screenshot per step
// as <out-prefix>-<step>.png.
// Usage: node checks/agent-paths.mjs [out-prefix]
import { openWorkspace } from "../lib/open.mjs";

const [prefix = "/tmp/agent-paths"] = process.argv.slice(2);

// Runs after the stub. Codex is found only at a located path that exists; the file picker
// answers with `window.__qaPick`, and `window.__qaExists` says whether that file is there.
const script = `(() => {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  let paths = {};
  window.__qaPaths = () => paths;
  window.__qaPick = "C:\\\\Users\\\\user\\\\AppData\\\\Local\\\\OpenAI\\\\Codex\\\\bin\\\\2993767bd1dfb009\\\\codex.exe";
  window.__qaExists = false;
  const status = (kind) => {
    const base = { kind, installed: true, version: "1.0.0", signedIn: true, detail: "" };
    if (kind !== "codex") return base;
    const path = paths.codex;
    if (path && window.__qaExists) return { ...base, version: "0.163.0" };
    return { kind, installed: false, version: null, signedIn: false, detail: path ? "No file at " + path : "Not installed" };
  };
  internals.invoke = async (cmd, args) => {
    if (cmd === "detect_agent") return status(args.kind);
    if (cmd === "save_settings") {
      paths = args.settings.agentPaths;
      return null;
    }
    if (cmd === "plugin:dialog|open") return window.__qaPick;
    return invoke(cmd, args);
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
await page.locator('button[aria-label="Settings"]').first().click();
const dialog = page.locator('[role="dialog"][aria-label="Settings"]');
await dialog.getByRole("button", { name: "CLI Providers", exact: true }).click();
const row = dialog.locator("div.border-b").filter({ has: page.locator('[aria-label="Connect Codex"]') });

const report = async (step) => {
  await page.waitForTimeout(200);
  const text = (await row.innerText()).split("\n").join(" | ");
  const saved = await page.evaluate(() => JSON.stringify(window.__qaPaths()));
  console.log(`${step}: ${text}  agentPaths=${saved}`);
  await dialog.screenshot({ path: `${prefix}-${step}.png` });
};

await row.getByText("Not installed").first().waitFor();
await report("missing");
await row.getByRole("button", { name: "Locate…" }).click();
await row.getByText("No file at").waitFor();
await report("gone");
await row.getByRole("button", { name: "Auto-detect" }).click();
await row.getByText("Not installed").first().waitFor();
await report("reset");
await page.evaluate(() => (window.__qaExists = true));
await row.getByRole("button", { name: "Locate…" }).click();
await row.getByText("Ready").waitFor();
await report("located");
await browser.close();
