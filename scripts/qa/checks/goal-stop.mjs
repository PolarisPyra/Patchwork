// Walks the stop and goal controls of two stub tasks that are both streaming, one with an
// active goal and one without, and records which backend commands each click invokes and what
// the banner shows after the events the backend would send back. Saves a screenshot of the
// goal task after Stop. Usage: node checks/goal-stop.mjs [out.png]
import { openWorkspace } from "../lib/open.mjs";

const [out = "/tmp/goal-stop.png"] = process.argv.slice(2);
const OBJECTIVE = "Keep the stripe aligned";

// Runs after the stub. Records every invoke, captures the agent-event listener so the check can
// send events, gives qa-idle a streaming reply with an active goal, and leaves qa-working as the
// stub has it: streaming, no goal.
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
  window.__qaEmit = (threadId, event) => callbacks[window.__qaAgentHandler]({ payload: { threadId, event } });
  const goal = { objective: ${JSON.stringify(OBJECTIVE)}, status: "active", startedAt: Date.now() - 90000, checks: 2, tokensUsed: null, tokenBudget: null, timeUsedSeconds: null };
  internals.invoke = async (cmd, args) => {
    if (cmd === "plugin:event|listen" && args && args.event === "agent-event") window.__qaAgentHandler = args.handler;
    if (cmd === "cancel_turn" || cmd === "stop_goal" || cmd === "send_message") window.__qaInvokes.push({ cmd, threadId: args && args.threadId });
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    const reply = (t, id) => [{ ...t.messages[0], id, status: "streaming", finishedAt: null }];
    return r.map((t) => (t.id === "qa-idle" ? { ...t, goal, messages: reply(t, "goal-reply") } : t.id === "qa-working" ? { ...t, messages: reply(t, "plain-reply") } : t));
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });

const row = (title) => page.locator("aside div.group.cursor-pointer", { hasText: title });
const composerStop = page.locator('button[aria-label="Stop"]');
const banner = page.locator(`div[title="${OBJECTIVE}"]`);
const bannerButton = (text) => banner.locator("button", { hasText: text });
const invokes = () => page.evaluate(() => window.__qaInvokes.splice(0));
const emit = (threadId, event) => page.evaluate(([t, e]) => window.__qaEmit(t, e), [threadId, event]);
const report = {};

// A streaming task without a goal: Stop shows and cancels that task's turn.
await row("Refactor the diff view").click();
await page.waitForTimeout(400);
report.plainStopVisible = await composerStop.isVisible();
await composerStop.click();
await page.waitForTimeout(200);
report.plainStopInvokes = await invokes();
await emit("qa-working", { type: "turnCompleted", messageId: "plain-reply", status: "cancelled", error: null });
await page.waitForTimeout(300);
report.plainStopGoneAfterCancel = !(await composerStop.isVisible());

// The goal task, still streaming while the other task was stopped: its goal is untouched.
await row("Fix the stripe alignment").click();
await page.waitForTimeout(400);
report.goalBannerLabel = (await banner.locator("span.font-semibold").textContent())?.trim();
report.goalBannerStopVisible = await bannerButton("Stop").isVisible();
report.goalComposerStopVisible = await composerStop.isVisible();
report.goalClearHiddenWhileWorking = !(await bannerButton("Clear").isVisible());

// Stop from the banner cancels the turn only; the backend then pauses the goal and ends the turn.
await bannerButton("Stop").click();
await page.waitForTimeout(200);
report.goalStopInvokes = await invokes();
await emit("qa-idle", { type: "goalUpdated", goal: { objective: OBJECTIVE, status: "paused", startedAt: Date.now() - 90000, checks: 2, tokensUsed: null, tokenBudget: null, timeUsedSeconds: null } });
await emit("qa-idle", { type: "turnCompleted", messageId: "goal-reply", status: "cancelled", error: null });
await page.waitForTimeout(400);
report.afterStopLabel = (await banner.locator("span.font-semibold").textContent())?.trim();
report.afterStopResumeVisible = await bannerButton("Resume").isVisible();
report.afterStopClearVisible = await bannerButton("Clear").isVisible();
report.afterStopBannerStopGone = !(await bannerButton("Stop").isVisible());
report.afterStopComposerStopGone = !(await composerStop.isVisible());
report.sidebarSpinners = await page.locator('aside [title="Working"]').count();
await page.locator("main").first().screenshot({ path: out });

// Clear asks the backend to clear the goal; once it reports the goal gone, the banner leaves.
await bannerButton("Clear").click();
await page.waitForTimeout(200);
report.clearInvokes = await invokes();
await emit("qa-idle", { type: "goalUpdated", goal: null });
await page.waitForTimeout(300);
report.bannerGoneAfterClear = (await banner.count()) === 0;

await browser.close();
console.log(JSON.stringify(report, null, 2));
console.log(`saved ${out}`);
