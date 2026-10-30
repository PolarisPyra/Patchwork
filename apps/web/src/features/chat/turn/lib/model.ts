import { type Message, type Part, type ToolCall } from "~/api";
import { fullElapsed } from "~/lib/util";

/**
 * What a turn holds, derived from its message.
 *
 * Everything here is a plain function of the message, so the shape of a turn can be reasoned about
 * — and tested — without rendering anything. The components in this folder only draw what these
 * functions return.
 */

/** A call or a block of reasoning: the smallest thing a row shows. */
export type Entry =
  | { kind: "thinking"; part: Extract<Part, { type: "thinking" }> }
  | { kind: "tool"; tool: ToolCall };

/** One line of a turn, in the order it happened. */
export type Step =
  /** The agent's text, which is the reply's result and never folds away. */
  | { kind: "text"; text: string; index: number }
  /** Reasoning and calls between two texts, shown as one row or a summary row. */
  | { kind: "work"; entries: Entry[] }
  /** Subagents started one after another, shown as cards. */
  | { kind: "agents"; tools: ToolCall[] }
  /** A plan proposed in plan mode. */
  | { kind: "plan"; text: string; index: number }
  /** Approvals and questions asked one after another, by id. */
  | { kind: "approvals"; ids: string[] };

/** Calls that keep a plan the composer already shows, so the turn does not repeat it. */
const HIDDEN_TOOLS = new Set(["TodoWrite", "TaskCreate", "TaskUpdate", "TaskList", "TaskGet"]);

/**
 * The reply's parts as steps. A reply saved before parts were kept shows its calls, then its text.
 */
export function stepsOf(message: Message): Step[] {
  const byId = new Map(message.tools.map((tool) => [tool.id, tool]));
  const shown = (tool: ToolCall | undefined): tool is ToolCall =>
    tool !== undefined && !tool.parent && !HIDDEN_TOOLS.has(tool.name);
  const parts: Part[] =
    message.parts.length > 0
      ? message.parts
      : [
          ...message.tools.map((tool): Part => ({ type: "tool", id: tool.id })),
          ...(message.text ? [{ type: "text", text: message.text } as Part] : []),
        ];
  const steps: Step[] = [];
  for (const [index, part] of parts.entries()) {
    if (part.type === "text") {
      if (part.text.trim()) steps.push({ kind: "text", text: part.text.trim(), index });
      continue;
    }
    if (part.type === "plan") {
      steps.push({ kind: "plan", text: part.text, index });
      continue;
    }
    if (part.type === "approval") {
      const previous = steps.at(-1);
      if (previous?.kind === "approvals") previous.ids.push(part.id);
      else steps.push({ kind: "approvals", ids: [part.id] });
      continue;
    }
    const entry = entryOf(part, byId, shown);
    if (entry === null) continue;
    // A subagent is its own card, however many of them ran in a row.
    if (entry.kind === "tool" && entry.tool.subagent) {
      const previous = steps.at(-1);
      if (previous?.kind === "agents") previous.tools.push(entry.tool);
      else steps.push({ kind: "agents", tools: [entry.tool] });
      continue;
    }
    const previous = steps.at(-1);
    if (previous?.kind === "work") previous.entries.push(entry);
    else steps.push({ kind: "work", entries: [entry] });
  }
  return steps;
}

/** The entry a thinking or tool part becomes, or null when the part is not shown at all. */
function entryOf(
  part: Part,
  byId: Map<string, ToolCall>,
  shown: (tool: ToolCall | undefined) => tool is ToolCall,
): Entry | null {
  if (part.type === "thinking") {
    return part.text.trim() ? { kind: "thinking", part } : null;
  }
  if (part.type !== "tool") return null;
  const tool = byId.get(part.id);
  return shown(tool) ? { kind: "tool", tool } : null;
}

/** Whether the work this entry stands for is still going. */
export function entryActive(entry: Entry): boolean {
  return entry.kind === "thinking"
    ? entry.part.endedAt === null
    : entry.tool.status === "running" || entry.tool.status === "pending";
}

/** A stable identity for a row, so it keeps its place as the reply grows. */
export function workKey(entry: Entry): string {
  return entry.kind === "tool" ? entry.tool.id : `r${entry.part.startedAt}`;
}

/** What the agent is doing at the moment, shown in the line at the head of a turn. */
export type Phase =
  | { kind: "retry"; reason: string; attempt: number; maxAttempts: number }
  | { kind: "checking-goal" }
  | { kind: "compacting" }
  /** Nothing else is active: the model is between parts, or thinking about the next one. */
  | { kind: "working" };

/** The phase of a turn that is still streaming, or null once it has settled. */
export function phaseOf(message: Message, streaming: boolean): Phase | null {
  if (!streaming) return null;
  if (message.retry) return { kind: "retry", ...message.retry };
  if (message.checkingGoal) return { kind: "checking-goal" };
  if (message.compacting) return { kind: "compacting" };
  return { kind: "working" };
}

/** "1 step", "7 steps". */
export function countOf(n: number): string {
  return n === 1 ? "1 step" : `${n} steps`;
}

/**
 * How many things a step holds. A run of work is one step but several actions, and a reader
 * counting rows means the actions: "4 steps" for a thought and three calls, not "1 step" for the
 * run they were grouped into.
 */
export function sizeOf(step: Step): number {
  switch (step.kind) {
    case "work":
      return step.entries.length;
    case "agents":
      return step.tools.length;
    case "approvals":
      return step.ids.length;
    case "text":
    case "plan":
      return 1;
  }
}

/** How many actions a run of steps holds, which is what a fold's count reads out. */
export function actionsIn(steps: readonly Step[]): number {
  return steps.reduce((n, step) => n + sizeOf(step), 0);
}

/** Whether a turn folds, and what it would hide. */
export interface FoldLayout {
  folds: boolean;
  /** How many steps the fold holds. */
  hidden: number;
  /** How many actions those steps are, which is the number the fold shows. */
  actions: number;
}

/** What the line at the head of a settled turn says, and what opening it would do. */
export interface FoldSummary {
  /** "Worked for 1m 14s", "You stopped the response", "Worked". */
  label: string;
  /** "4 steps", or "Hide steps" once the fold is open; null when there is nothing to open. */
  action: string | null;
  /** The turn was cut short by a steer, so it reads as a superseded attempt. */
  superseded: boolean;
}

/**
 * The head line of a turn that has finished. A turn under a second has no duration worth reading,
 * and "Worked for 0s" reads as a bug, so it says "Worked" instead.
 */
export function foldHeadOf(message: Message, actions: number, open: boolean): FoldSummary {
  const action = actions === 0 ? null : open ? "Hide steps" : countOf(actions);
  if (message.cutOff) return { label: "Superseded attempt", action, superseded: true };
  const took = message.finishedAt === null ? null : message.finishedAt - message.createdAt;
  const duration = took !== null && took >= 1000 ? fullElapsed(took) : null;
  if (message.status === "cancelled") {
    return {
      label: duration ? `You stopped after ${duration}` : "You stopped the response",
      action,
      superseded: false,
    };
  }
  return { label: duration ? `Worked for ${duration}` : "Worked", action, superseded: false };
}

/**
 * The steps a turn shows. A settled turn folds everything before its last text into one row; a
 * subagent still working and a question the agent asked stay in view, since they are live.
 */
export function shownSteps(
  message: Message,
  steps: readonly Step[],
  fold: { folds: boolean; open: boolean; hidden: number },
): readonly Step[] {
  if (!fold.folds || fold.open) return steps;
  const asked = (step: Step) =>
    step.kind === "approvals" &&
    step.ids.some((id) => message.approvals.some((a) => a.id === id && a.questions?.length));
  const live = (step: Step) =>
    step.kind === "agents" && step.tools.some((tool) => entryActive({ kind: "tool", tool }));
  return steps.filter((step, i) => i >= fold.hidden || live(step) || asked(step));
}

/** Whether a settled turn folds, how many steps it hides, and how many actions those hold. */
export function foldLayoutOf(
  message: Message,
  steps: readonly Step[],
  streaming: boolean,
): FoldLayout {
  // The final text stays out, with any work after it. A failed reply stays open.
  const lastText = steps
    .map((step) => step.kind === "text" || step.kind === "plan")
    .lastIndexOf(true);
  const beforeText = lastText === -1 ? steps.length : lastText;
  // A reply a steer cut short folds whole.
  const hidden = message.cutOff ? steps.length : beforeText;
  const folds =
    !streaming &&
    message.status !== "error" &&
    (hidden > 0 || message.status === "cancelled" || message.cutOff);
  return { folds, hidden, actions: actionsIn(steps.slice(0, hidden)) };
}

/** The span the subagents ran in, or null while one is still going. */
export function spanOf(subagents: readonly ToolCall[]): number | null {
  const starts = subagents.flatMap((t) => (t.startedAt === null ? [] : [t.startedAt]));
  const ends = subagents.flatMap((t) => (t.endedAt === null ? [] : [t.endedAt]));
  if (starts.length === 0 || ends.length < subagents.length) return null;
  return Math.max(...ends) - Math.min(...starts);
}
