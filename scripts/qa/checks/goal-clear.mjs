// Walks clearing a goal on a stub task: `/goal clear` typed while the goal's turn runs, then the
// banner's Clear goal, and the clear run the backend starts. Records the backend commands each
// step invokes, any composer error, and whether the clear run shows a "Working for" row, a
// `/goal clear` bubble, or the green notice. Saves a screenshot once the goal is cleared.
// Usage: node checks/goal-clear.mjs [out.png]
import { openWorkspace } from "../lib/open.mjs";

const [out = "/tmp/goal-clear.png"] = process.argv.slice(2);
const OBJECTIVE = "Keep the stripe aligned";

// Runs after the stub. Records every invoke, captures the agent-event listener so the check can
// send events, and gives qa-idle a streaming reply with an active goal.
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
    if (cmd === "cancel_turn" || cmd === "stop_goal" || cmd === "send_message") window.__qaInvokes.push({ cmd, text: args && args.text });
    const r = await invoke(cmd, args);
    if (cmd !== "list_threads") return r;
    return r.map((t) => (t.id === "qa-idle" ? { ...t, goal, sessionId: "qa-session", messages: [{ ...t.messages[0], id: "goal-reply", status: "streaming", finishedAt: null }] } : t));
  };
})();`;

const { browser, page } = await openWorkspace({ width: 1200, height: 800 });
await page.addInitScript({ content: script });
await page.reload({ waitUntil: "networkidle" });
await page.locator('button[aria-label="Tasks"]').waitFor({ timeout: 15000 });

const goal = (status) => ({ objective: OBJECTIVE, status, startedAt: Date.now() - 90000, checks: 2, tokensUsed: null, tokenBudget: null, timeUsedSeconds: null });
const message = (id, role, status, text, createdAt) => ({ id, role, text, parts: text ? [{ type: "text", text }] : [], createdAt, finishedAt: null, status, tools: [], plan: [], files: [], approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null });
const banner = page.locator("div.composer-strip", { hasText: OBJECTIVE });
const bannerButton = (text) => banner.locator("button", { hasText: text });
const composer = page.locator(".composer-editor .ProseMirror");
const invokes = () => page.evaluate(() => window.__qaInvokes.splice(0));
const emit = (threadId, event) => page.evaluate(([t, e]) => window.__qaEmit(t, e), [threadId, event]);
const working = () => page.locator("main", { hasText: "Working for" }).count();
const workingRows = async () => (await page.locator("main div", { hasText: /^Working for / }).count()) > 0;
const report = {};

await page.locator("aside div.group.cursor-pointer", { hasText: "Fix the stripe alignment" }).click();
await page.waitForTimeout(400);
report.workingRowWhileGoalRuns = await workingRows();

// Typed mid-turn, /goal clear does what Stop does: it cancels the turn, and sends nothing.
await composer.click();
await page.keyboard.type("/goal clear");
await page.keyboard.press("Enter");
await page.waitForTimeout(300);
report.typedClearInvokes = await invokes();
report.typedClearError = await page.locator("main", { hasText: "runs as a turn of its own" }).count();
report.composerEmptied = ((await composer.textContent()) ?? "").trim() === "";
// The backend pauses the goal as the cancelled turn ends.
await emit("qa-idle", { type: "goalUpdated", goal: goal("paused") });
await emit("qa-idle", { type: "turnCompleted", messageId: "goal-reply", status: "cancelled", error: null });
await page.waitForTimeout(400);
report.labelAfterStop = (await banner.locator("span.font-semibold").textContent())?.trim();

// The banner's Clear goal: the banner says it is clearing until the agent reports the goal gone.
await bannerButton("Clear goal").click();
await page.waitForTimeout(200);
report.bannerClearInvokes = await invokes();
report.labelWhileClearing = (await banner.locator("span.font-semibold").textContent())?.trim();
report.buttonsWhileClearing = await banner.locator("button[class*='px-2']").count();

// The clear run, as the backend sends it: its user message, then a reply with no text yet.
const at = Date.now();
await emit("qa-idle", { type: "messageStarted", message: message("clear-ask", "user", "complete", "/goal clear", at) });
await emit("qa-idle", { type: "messageStarted", message: message("clear-reply", "assistant", "streaming", "", at + 1) });
await page.waitForTimeout(300);
report.workingRowBeforeText = await workingRows();
report.thinkingRowBeforeText = (await page.locator("main", { hasText: "Thinking" }).count()) > 0;
// The reply's height at each stage: a change between them is a jump in the chat.
const replyHeight = () => page.evaluate(() => [...document.querySelectorAll("main section")].at(-1)?.getBoundingClientRect().height ?? 0);
report.replyHeights = { beforeText: await replyHeight() };
report.clearBubbles = await page.locator("main", { hasText: /^\/goal\s*clear$/ }).count();
report.clearChipShown = await page.locator("main span", { hasText: /^\/goal$/ }).count();

await emit("qa-idle", { type: "textDelta", messageId: "clear-reply", text: `Goal cleared: ${OBJECTIVE}` });
await emit("qa-idle", { type: "goalUpdated", goal: null });
await page.waitForTimeout(300);
report.workingRowWithText = await workingRows();
const notice = page.locator("main p", { hasText: `Goal cleared: ${OBJECTIVE}` });
report.noticeShown = (await notice.count()) > 0 && (await notice.first().getAttribute("class"))?.includes("text-fg-muted");
report.replyHeights.withText = await replyHeight();
await emit("qa-idle", { type: "turnCompleted", messageId: "clear-reply", status: "complete", error: null });
await page.waitForTimeout(400);
report.replyHeights.afterEnd = await replyHeight();
report.copyRetryRows = await page.locator("main section").last().locator("button", { hasText: "Retry" }).count();
report.bannerGone = (await banner.count()) === 0;
report.workingTextAnywhere = await working();
await page.locator("main").first().screenshot({ path: out });

await browser.close();
console.log(JSON.stringify(report, null, 2));
console.log(`saved ${out}`);
