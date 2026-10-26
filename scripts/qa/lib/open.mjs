// Opens the running dev session's workspace in headless Chromium.
// Connects only; it never starts the app.
import { chromium } from "playwright";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const URL = "http://localhost:1420/";

// `init` is a script run after the stub, for a check that needs more data than it has.
export async function openWorkspace({ width = 1200, height = 800, init } = {}) {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width, height } });
  // Replaces the Tauri IPC with canned data so the workspace renders without the Rust backend.
  await page.addInitScript({ content: readFileSync(join(here, "tauri-stub.js"), "utf8") });
  if (init) await page.addInitScript({ content: init });
  try {
    await page.goto(URL, { waitUntil: "networkidle", timeout: 15000 });
  } catch {
    await browser.close();
    throw new Error(`No dev server at ${URL}. Ask the user to start \`pnpm tauri dev\`.`);
  }
  try {
    await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });
  } catch {
    await browser.close();
    throw new Error("Workspace did not render (Tasks button missing). Check the stub and the dev session console.");
  }
  return { browser, page };
}
