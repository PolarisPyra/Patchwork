import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  ArrowClockwiseIcon,
  BooksIcon,
  BrainIcon,
  CaretDownIcon,
  CaretRightIcon,
  CheckIcon,
  EyeIcon,
  GitDiffIcon,
  GlobeIcon,
  MagnifyingGlassIcon,
  NotePencilIcon,
  RobotIcon,
  TargetIcon,
  TerminalIcon,
  WarningCircleIcon,
  WrenchIcon,
} from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import type { AgentKind, Message, Part, ToolCall } from "~/api";
import { fullElapsed, isActive, titleOf } from "~/components/chat/LineageCard";
import ProposedPlanCard from "~/components/chat/ProposedPlanCard";
import ProviderLogos from "~/components/ProviderLogos";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.
import { isGoalClearedReply } from "~/components/chat/goalPresentation";
import Markdown from "~/components/chat/Markdown";
import StreamedMarkdown from "~/components/chat/StreamedMarkdown";

/** What a tool call does, which picks its icon, its verb, and how a group of calls is summed up. */
type Action = "edit" | "command" | "read" | "search" | "web" | "agent" | "skill" | "other";

const ACTIONS: Record<string, Action> = {
  Bash: "command",
  BashOutput: "command",
  Shell: "command",
  run_command: "command",
  Read: "read",
  NotebookRead: "read",
  view_file: "read",
  Edit: "edit",
  Write: "edit",
  MultiEdit: "edit",
  NotebookEdit: "edit",
  write_to_file: "edit",
  replace_file_content: "edit",
  multi_replace_file_content: "edit",
  Grep: "search",
  Glob: "search",
  LS: "search",
  grep_search: "search",
  find_by_name: "search",
  list_dir: "search",
  WebSearch: "web",
  WebFetch: "web",
  search_web: "web",
  read_url_content: "web",
  Skill: "skill",
};

/** Calls that are not shown: the plan they keep is shown above the composer. */
const HIDDEN_TOOLS = new Set(["TodoWrite"]);

function actionOf(tool: ToolCall): Action {
  if (tool.subagent) return "agent";
  const action = ACTIONS[tool.name] ?? "other";
  // Agents without a skill tool load a skill by reading its SKILL.md.
  return action === "read" && /(^|\/)SKILL\.md$/.test(tool.summary) ? "skill" : action;
}

/** The skill a call loads: Claude names Patchwork's as the "claude" plugin's, and a read
 * names its folder. */
function skillName(tool: ToolCall): string {
  if (tool.name !== "Skill") return tool.summary.split("/").slice(-2, -1)[0] ?? "";
  return tool.summary.replace(/^claude:/, "");
}

const ACTION_ICONS: Record<Action, ReactNode> = {
  edit: <NotePencilIcon size={15} />,
  command: <TerminalIcon size={15} />,
  read: <EyeIcon size={15} />,
  search: <MagnifyingGlassIcon size={15} />,
  web: <GlobeIcon size={15} />,
  agent: <RobotIcon size={15} />,
  skill: <BooksIcon size={15} />,
  other: <WrenchIcon size={15} />,
};

/** How a group's calls of one kind are counted, most telling first. */
const ACTION_COUNTS: [Action, string, string][] = [
  ["edit", "Changed", "file"],
  ["command", "Ran", "command"],
  ["read", "Read", "file"],
  ["search", "Searched code", "time"],
  ["web", "Searched web", "time"],
  ["agent", "Ran", "subagent"],
  ["skill", "Used", "skill"],
  ["other", "Used", "tool"],
];

/** The program a command runs: its first word, without a path. */
function programOf(command: string): string {
  const word = command.trim().split(/\s+/)[0] ?? "";
  return word.split("/").pop() || "command";
}

/** `mcp__server__tool` and Codex's `server.tool` name the tool last. */
function toolName(tool: ToolCall): string {
  if (tool.name.startsWith("mcp__")) return tool.name.split("__").pop() ?? tool.name;
  if (tool.name === "MCP") return tool.summary.split(".").pop() ?? tool.summary;
  return tool.name;
}

/** The row's label: what the call does while it runs, and what it did once done. */
function toolLabel(tool: ToolCall): string {
  const { summary } = tool;
  const running = tool.status === "running" || tool.status === "pending";
  switch (actionOf(tool)) {
    case "command":
      if (running) return `Running ${programOf(summary)}`;
      if (tool.status === "error") return `Failed ${programOf(summary)}`;
      return summary || "Ran command";
    case "read":
      return summary ? `${running ? "Reading" : "Read"} ${summary}` : "Read file";
    case "edit":
      return summary ? `${running ? "Editing" : "Changed"} ${summary}` : "Changed file";
    case "search":
      return summary ? `${running ? "Searching" : "Searched"} ${summary}` : "Searched files";
    case "web":
      if (tool.name === "WebFetch") return `${running ? "Fetching" : "Fetched"} ${summary}`;
      return summary ? `${running ? "Searching" : "Searched"} ${summary}` : "Searched web";
    case "agent":
      return titleOf(tool);
    case "skill": {
      const name = skillName(tool);
      if (!name) return running ? "Loading skill" : "Used skill";
      return `${running ? "Loading" : "Used"} the ${name} skill`;
    }
    case "other":
      return `${running ? "Using" : "Used"} ${toolName(tool)}`;
  }
}

/** "Ran 3 commands and read 2 files": the two kinds of call a group did most tellingly. */
function groupLabel(tools: ToolCall[]): string {
  const counts = ACTION_COUNTS.flatMap(([action, verb, noun]) => {
    const n = tools.filter((t) => actionOf(t) === action).length;
    return n === 0 ? [] : [`${verb} ${n} ${noun}${n === 1 ? "" : "s"}`];
  }).slice(0, 2);
  if (counts.length === 0) return "Thought";
  return counts.map((c, i) => (i === 0 ? c : c.toLowerCase())).join(" and ");
}

/** A step of the timeline: text, or a run of work (reasoning and calls) between texts. */
type Entry =
  | { kind: "thinking"; part: Extract<Part, { type: "thinking" }> }
  | {
      kind: "tool";
      tool: ToolCall;
    };
type Step =
  | { kind: "text"; text: string; index: number }
  | { kind: "work"; entries: Entry[] }
  /** Subagents started one after another, shown as cards. */
  | { kind: "agents"; tools: ToolCall[] }
  /** A plan proposed in plan mode. */
  | { kind: "plan"; text: string; index: number }
  /** Approvals and questions asked one after another, by id. */
  | { kind: "approvals"; ids: string[] };

/** The reply's parts as steps. Replies saved before parts were kept show their calls, then their
 * text. */
function stepsOf(message: Message): Step[] {
  const byId = new Map(message.tools.map((t) => [t.id, t]));
  const shown = (t: ToolCall | undefined): t is ToolCall =>
    t !== undefined && !t.parent && !HIDDEN_TOOLS.has(t.name);
  const parts: Part[] =
    message.parts.length > 0
      ? message.parts
      : [
          ...message.tools.map((t): Part => ({ type: "tool", id: t.id })),
          ...(message.text ? [{ type: "text", text: message.text } as Part] : []),
        ];
  const steps: Step[] = [];
  parts.forEach((part, index) => {
    if (part.type === "text") {
      if (part.text.trim()) steps.push({ kind: "text", text: part.text.trim(), index });
      return;
    }
    if (part.type === "plan") {
      steps.push({ kind: "plan", text: part.text, index });
      return;
    }
    if (part.type === "approval") {
      const last = steps.at(-1);
      if (last?.kind === "approvals") last.ids.push(part.id);
      else steps.push({ kind: "approvals", ids: [part.id] });
      return;
    }
    let entry: Entry;
    if (part.type === "thinking") {
      if (!part.text.trim()) return;
      entry = { kind: "thinking", part };
    } else {
      const tool = byId.get(part.id);
      if (!shown(tool)) return;
      if (tool.subagent) {
        const last = steps.at(-1);
        if (last?.kind === "agents") last.tools.push(tool);
        else steps.push({ kind: "agents", tools: [tool] });
        return;
      }
      entry = { kind: "tool", tool };
    }
    const last = steps.at(-1);
    if (last?.kind === "work") last.entries.push(entry);
    else steps.push({ kind: "work", entries: [entry] });
  });
  return steps;
}

function entryActive(entry: Entry): boolean {
  return entry.kind === "thinking"
    ? entry.part.endedAt === null
    : entry.tool.status === "running" || entry.tool.status === "pending";
}

/** The current time, ticking each second while `live`. */
function useNow(live: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [live]);
  return live ? now : Date.now();
}

/**
 * A reply as it happened: text, reasoning, and tool calls in order. While the agent works, a
 * "Working for" row heads it; once it is done, everything before the final text folds into one
 * "Worked for" row.
 */
export default function TurnTimeline({
  message,
  agent,
  clearsGoal = false,
  onOpenThread,
  onOpenFile,
  renderApprovals,
}: {
  message: Message;
  agent: AgentKind;
  /** The reply answers `/goal clear`, so it shows no working row even before its text arrives. */
  clearsGoal?: boolean;
  /** Opens a subagent's thread. */
  onOpenThread: (id: string) => void;
  /** Opens a file; edit rows link to their file when given. */
  onOpenFile?: (path: string) => void;
  /** The approvals and questions with these ids, shown where they were asked. */
  renderApprovals?: (ids: string[]) => ReactNode;
}) {
  const streaming = message.status === "streaming";
  // A streaming reply re-renders on every delta, and its steps only change when its parts do.
  const steps = useMemo(() => stepsOf(message), [message.parts, message.tools, message.text]);
  // The final text stays out of the fold, with any work after it. A failed reply stays open.
  // A proposed plan is the reply's result, like its final text.
  const lastText = steps.map((s) => s.kind === "text" || s.kind === "plan").lastIndexOf(true);
  const textHidden = lastText === -1 ? steps.length : lastText;
  const stopped = message.status === "cancelled";
  // A reply a steer cut short folds whole, like a superseded attempt.
  const hidden = message.cutOff ? steps.length : textHidden;
  const folds =
    !streaming && message.status !== "error" && (hidden > 0 || stopped || message.cutOff);
  const [open, setOpen] = useState(false);
  // Subagents still working, and the questions the agent asked, stay in view when the reply folds.
  const asked = (s: Step) =>
    s.kind === "approvals" &&
    s.ids.some((id) => message.approvals.some((a) => a.id === id && a.questions?.length));
  const shown =
    folds && !open
      ? steps.filter(
          (s, i) => i >= hidden || (s.kind === "agents" && s.tools.some(isActive)) || asked(s),
        )
      : steps;
  const last = steps.at(-1);
  // The block still arriving settles into place; the ones before it are already read, so a chat
  // list that remounts them while scrolling does not replay their entrance.
  const arriving = streaming ? shown.at(-1) : undefined;
  const stepClass = (step: Step) => `px-1 py-0.5 pb-2${step === arriving ? " timeline-enter" : ""}`;
  const phase = phaseOf(message, streaming);
  // Did the turn do anything besides talk? A turn that only wrote text has no status line to show,
  // and one that ran a command keeps it for good.
  const didWork = steps.some((step) => step.kind === "work" || step.kind === "agents");
  // A reply that only reports a cleared goal did no work, so it shows as a status, not a turn.
  const goalCleared =
    clearsGoal ||
    (steps.length > 0 && steps.every((s) => s.kind === "text" && isGoalClearedReply(s.text)));
  return (
    <div className="flex min-w-0 flex-col">
      {/* The head of a turn, and the only place its status ever goes: what is happening now while
          it streams, what it took once it is done. It stays put above the reply in both states, so
          the turn does not lose a line or gain one as it finishes. */}
      {phase !== null && !goalCleared && <StatusLine phase={phase} since={message.createdAt} />}
      {/* Holds the status line's place until the agent's reply fills it, so the chat does not
          grow as the goal banner above the composer closes. */}
      {streaming && clearsGoal && steps.length === 0 && (
        <div className="px-1 py-0.5 pb-2">
          <GoalClearedNotice text="Clearing goal" live pending />
        </div>
      )}
      {!streaming && didWork && !goalCleared && (
        <FoldRow
          message={message}
          open={open}
          onToggle={() => setOpen((o) => !o)}
          hidden={actionsIn(steps.slice(0, hidden))}
        />
      )}
      {shown.map((step) =>
        step.kind === "text" ? (
          <div key={`t${step.index}`} className={stepClass(step)}>
            {goalCleared ? (
              <GoalClearedNotice text={step.text} live={streaming && !clearsGoal} />
            ) : (
              // The text still arriving is revealed at a steady rate, a word at a time.
              <StreamedMarkdown
                text={step.text}
                streaming={streaming && step === last && !goalCleared}
              />
            )}
          </div>
        ) : step.kind === "plan" ? (
          <div key={`p${step.index}`} className={stepClass(step)}>
            <ProposedPlanCard markdown={step.text} />
          </div>
        ) : step.kind === "approvals" ? (
          <div key={`q${step.ids[0]}`} className="flex flex-col gap-2.5 pb-2">
            {renderApprovals?.(step.ids)}
          </div>
        ) : step.kind === "agents" ? (
          <SubagentCards
            key={`a${step.tools[0].id}`}
            subagents={step.tools}
            calls={message.tools}
            agent={agent}
            onOpenThread={onOpenThread}
          />
        ) : (
          <WorkGroup
            key={workKey(step.entries[0])}
            entries={step.entries}
            live={streaming}
            onOpenFile={onOpenFile}
          />
        ),
      )}
    </div>
  );
}

function workKey(entry: Entry): string {
  return entry.kind === "tool" ? entry.tool.id : `r${entry.part.startedAt}`;
}

/** The result of `/goal clear`: a status line, like "Worked for". It fades in as it arrives, not
 * each time the chat list mounts it again. */
function GoalClearedNotice({
  text,
  live,
  pending = false,
}: {
  text: string;
  live: boolean;
  pending?: boolean;
}) {
  return (
    <p
      className={`${live ? "goal-cleared-in " : ""}m-0 flex min-h-6 items-center gap-1.5 text-[length:var(--fs-md)] text-fg-muted [word-break:break-word]`}
    >
      {pending ? (
        <Spinner size={14} className="text-fg-subtle" />
      ) : (
        <CheckIcon size={14} className="flex-none text-fg-subtle" />
      )}
      {text}
    </p>
  );
}

/**
 * The row that heads a running reply. It is on screen for the whole turn and is the first thing
 * read, so it appears at once and stays put: fading it in reads as a flicker before the counter
 * settles. `tabular-nums` keeps the digits from shifting as the count grows.
 */
/**
 * What the agent is doing at the moment, when no row of the reply is already showing it.
 *
 * A reply with an active row does not get a status line as well: that row says what is running, in
 * the place it is running, and a second line repeating it a few pixels above is noise.
 */
type Phase =
  | { kind: "retry"; reason: string; attempt: number; maxAttempts: number }
  | { kind: "checking-goal" }
  /** Nothing in the reply is active: the model is between parts, or thinking about the next one. */
  | { kind: "working" };

function phaseOf(message: Message, streaming: boolean): Phase | null {
  if (!streaming) return null;
  if (message.retry) return { kind: "retry", ...message.retry };
  if (message.checkingGoal) return { kind: "checking-goal" };
  return { kind: "working" };
}

/**
 * The one status line a reply has: what is happening now, above what has already happened. It is
 * the same shape as the row that replaces it once the reply settles (see FoldRow), so a finished
 * turn reads as one line changing state rather than two lines swapping places.
 */
function StatusLine({ phase, since }: { phase: Phase; since: number }) {
  const now = useNow(phase.kind === "working");
  const label =
    phase.kind === "retry"
      ? `Retrying: ${phase.reason} (${phase.attempt}/${phase.maxAttempts})`
      : phase.kind === "checking-goal"
        ? "Checking goal"
        : `Working for ${fullElapsed(Math.max(0, now - since))}`;
  return (
    <div className="mb-1.5 border-b border-border pt-1 pb-2">
      <div className="flex h-6 min-w-0 items-center gap-0.5 px-1 text-[length:var(--fs-md)] text-fg-muted tabular-nums">
        <RowIcon>
          {phase.kind === "retry" ? (
            <ArrowClockwiseIcon size={15} />
          ) : phase.kind === "checking-goal" ? (
            <TargetIcon size={15} />
          ) : (
            <Spinner size={13} className="text-fg-subtle" />
          )}
        </RowIcon>
        <span className="min-w-0 truncate">{label}</span>
      </div>
    </div>
  );
}

const FOLD_CLASS =
  "flex cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-[length:var(--fs-md)] text-fg-muted hover:text-fg disabled:cursor-default disabled:hover:text-fg-muted";

/** "1 step", "7 steps". */
function countOf(n: number): string {
  return n === 1 ? "1 step" : `${n} steps`;
}

/**
 * How many things a step holds. A run of work is one step but several actions, and a reader
 * counting rows means the actions: "4 steps" for a thought and three calls, not "1 step" for the
 * run they were grouped into.
 */
function sizeOf(step: Step): number {
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
function actionsIn(steps: Step[]): number {
  return steps.reduce((n, step) => n + sizeOf(step), 0);
}

function FoldRow({
  message,
  open,
  onToggle,
  hidden,
}: {
  message: Message;
  open: boolean;
  onToggle: () => void;
  /** How many steps the fold holds; nothing to unfold when 0. */
  hidden: number;
}) {
  const stopped = message.status === "cancelled";
  // What the fold holds, so the control says what opening it would show.
  const action = hidden === 0 ? null : open ? "Hide steps" : countOf(hidden);
  if (message.cutOff) {
    const Caret = open ? CaretDownIcon : CaretRightIcon;
    return (
      <div className="mb-1.5 flex flex-col gap-1.5 border-b border-border pt-1 pb-2">
        <button
          type="button"
          aria-expanded={open}
          disabled={hidden === 0}
          onClick={onToggle}
          aria-label={action === null ? undefined : `Superseded attempt, ${action}`}
          className={FOLD_CLASS}
        >
          <span>Superseded attempt</span>
          {action !== null && (
            <>
              <span className="text-fg-subtle">{action}</span>
              <Caret size={13} />
            </>
          )}
        </button>
        <span className="text-[length:var(--fs-sm)] text-fg-subtle">Cut off by steer</span>
      </div>
    );
  }
  // A turn under a second has no duration worth reading, and "Worked for 0s" reads as a bug.
  const took = message.finishedAt === null ? null : message.finishedAt - message.createdAt;
  const duration = took !== null && took >= 1000 ? fullElapsed(took) : null;
  const label = stopped
    ? duration
      ? `You stopped after ${duration}`
      : "You stopped the response"
    : duration
      ? `Worked for ${duration}`
      : "Worked";
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  return (
    <div className="mb-1.5 flex flex-col items-start gap-1.5 border-b border-border pt-1 pb-2">
      <button
        type="button"
        aria-expanded={open}
        disabled={hidden === 0}
        onClick={onToggle}
        aria-label={action === null ? undefined : `${label}, ${action}`}
        className={`${FOLD_CLASS} tabular-nums`}
      >
        <span>{label}</span>
        {action !== null && (
          <>
            <span className="text-fg-subtle">{action}</span>
            <Caret size={13} />
          </>
        )}
      </button>
    </div>
  );
}

/** A run of work between texts: one row, or a summary row over several that opens to list them.
 * While its last call runs, the summary shows that call instead. */
function WorkGroup({
  entries,
  live,
  onOpenFile,
}: {
  entries: Entry[];
  live: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const lastEntry = entries.at(-1);
  if (entries.length === 1) {
    return (
      <div className="pb-2">
        <EntryRow entry={entries[0]} live={live} entering={live} onOpenFile={onOpenFile} />
      </div>
    );
  }
  const current = live ? [...entries].reverse().find(entryActive) : undefined;
  const tools = entries.flatMap((e) => (e.kind === "tool" ? [e.tool] : []));
  const failed = tools.some((t) => t.status === "error");
  const first = tools[0];
  return (
    <div className="pb-2">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={ROW_CLASS}
      >
        <RowIcon>
          {current ? (
            entryIcon(current)
          ) : failed ? (
            <WarningCircleIcon size={15} className="text-danger" />
          ) : first ? (
            ACTION_ICONS[actionOf(first)]
          ) : (
            <BrainIcon size={15} />
          )}
        </RowIcon>
        <RowLabel entering={live} running={current !== undefined}>
          {current ? entryLabel(current) : groupLabel(tools)}
        </RowLabel>
      </button>
      {open && (
        <div className="timeline-expand timeline-expand-enter overflow-hidden">
          <div className="flex flex-col">
            {entries.map((entry) => (
              <EntryRow
                key={workKey(entry)}
                entry={entry}
                live={live}
                entering={live && entry === lastEntry}
                onOpenFile={onOpenFile}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

const ROW_CLASS =
  "group flex min-h-6 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-0.5 py-0.5 text-left text-[length:var(--fs-md)] text-fg-muted hover:text-fg";

function RowIcon({ children }: { children: ReactNode }) {
  return <span className="grid w-6 flex-none place-items-center opacity-70">{children}</span>;
}

/**
 * The text of a row that has an icon beside it, and the sign that its work is still running.
 *
 * Nothing here fades. A label that dims and brightens on a 1.6s loop reads as text that cannot
 * settle, and on a row with an icon the icon looks like it is going with it. So the text is painted
 * once at full strength and the row shows a small ring-and-arc spinner while it runs: the same cue
 * the pending goal notice uses, and one that leaves the words alone. Only a row that has just
 * arrived fades its text in, and the rows that persist for a whole turn do not.
 */
function RowLabel({
  entering = false,
  running = false,
  className = "",
  children,
}: {
  /** The row has just arrived, so its text fades in. */
  entering?: boolean;
  /** Its work is running, so the row carries a spinner. */
  running?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span
      className={`flex min-w-0 flex-1 items-center gap-1.5 ${entering ? "timeline-status-enter" : ""} ${className}`}
    >
      <span className="min-w-0 truncate">{children}</span>
      {running && <Spinner size={11} className="text-fg-subtle" />}
    </span>
  );
}

function entryIcon(entry: Entry): ReactNode {
  if (entry.kind === "thinking") return <BrainIcon size={15} />;
  if (entry.tool.status === "error") return <WarningCircleIcon size={15} className="text-danger" />;
  return ACTION_ICONS[actionOf(entry.tool)];
}

/**
 * A row's label. Reasoning reads as how long it took once it has ended, never as its own text: the
 * text is a paragraph, and a paragraph truncated to one line says less than a duration does. It is
 * still one click away, in the row's details.
 */
function entryLabel(entry: Entry): string {
  if (entry.kind === "tool") return toolLabel(entry.tool);
  const { startedAt, endedAt } = entry.part;
  if (endedAt === null) return "Thinking";
  const took = endedAt - startedAt;
  // Under a second there is no duration worth reading, so it is just "Thought".
  return took >= 1000 ? `Thought for ${fullElapsed(took)}` : "Thought";
}

/** The one file an edit call changes, or null for other calls, failed edits, and edits of
 * several files at once. */
function editedFile(entry: Entry): string | null {
  if (entry.kind !== "tool" || actionOf(entry.tool) !== "edit") return null;
  const { summary, status } = entry.tool;
  return status === "error" || !summary || summary.includes(", ") ? null : summary;
}

/** One call or block of reasoning; it opens to show the command and its output, or the whole
 * reasoning. An edit opens its file instead. */
function EntryRow({
  entry,
  live,
  entering = false,
  onOpenFile,
}: {
  entry: Entry;
  live: boolean;
  /** The row has just arrived in a running reply, so it settles into place. */
  entering?: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const active = live && entryActive(entry);
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  const file = onOpenFile ? editedFile(entry) : null;
  if (file !== null && onOpenFile) {
    return (
      <div className="min-w-0">
        <button type="button" title={file} onClick={() => onOpenFile(file)} className={ROW_CLASS}>
          <RowIcon>{entryIcon(entry)}</RowIcon>
          <RowLabel
            entering={entering}
            running={active}
            className="underline-offset-2 group-hover:underline"
          >
            {entryLabel(entry)}
          </RowLabel>
          <GitDiffIcon
            size={13}
            className="flex-none text-fg-subtle opacity-0 group-hover:opacity-100"
          />
        </button>
      </div>
    );
  }
  return (
    <div className="min-w-0">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={ROW_CLASS}
      >
        <RowIcon>{entryIcon(entry)}</RowIcon>
        <RowLabel entering={entering} running={active}>
          {entryLabel(entry)}
        </RowLabel>
        <Caret size={12} className="flex-none text-fg-subtle opacity-0 group-hover:opacity-100" />
      </button>
      {open && (
        <div className="timeline-expand timeline-expand-enter overflow-hidden">
          <EntryDetails entry={entry} />
        </div>
      )}
    </div>
  );
}

const DETAILS_CLASS =
  "ms-7 mt-0.5 mb-1 max-h-64 overflow-auto rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted selectable";

function EntryDetails({ entry }: { entry: Entry }) {
  if (entry.kind === "thinking") {
    return (
      <div className={DETAILS_CLASS}>
        <Markdown text={entry.part.text} />
      </div>
    );
  }
  const { tool } = entry;
  const input = actionOf(tool) === "command" ? `$ ${tool.summary}` : tool.summary;
  const body = [input, tool.output].filter(Boolean).join("\n\n");
  return (
    <pre
      className={`${DETAILS_CLASS} m-0 font-[family-name:var(--font-mono)] break-words whitespace-pre-wrap`}
    >
      {body || "No output"}
    </pre>
  );
}

const SUBAGENT_STATUS: Record<ToolCall["status"], { dot: string; label: string }> = {
  pending: { dot: "bg-accent", label: "Queued" },
  running: { dot: "bg-accent", label: "Running" },
  done: { dot: "bg-success", label: "Completed" },
  error: { dot: "bg-danger", label: "Failed" },
};

/** Time from the start of the first subagent to the end of the last, or null while one runs. */
function spanOf(subagents: ToolCall[]): number | null {
  const starts = subagents.flatMap((t) => (t.startedAt === null ? [] : [t.startedAt]));
  const ends = subagents.flatMap((t) => (t.endedAt === null ? [] : [t.endedAt]));
  if (starts.length === 0 || ends.length < subagents.length) return null;
  return Math.max(...ends) - Math.min(...starts);
}

/** One subagent as a card, or several under a summary row that opens to list them. */
function SubagentCards({
  subagents,
  calls,
  agent,
  onOpenThread,
}: {
  subagents: ToolCall[];
  /** Every call of the reply, including the ones the subagents made. */
  calls: ToolCall[];
  agent: AgentKind;
  onOpenThread: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const active = subagents.some(isActive);
  const now = useNow(active);
  if (subagents.length === 1) {
    return (
      <div className="pb-2">
        <SubagentCard tool={subagents[0]} calls={calls} agent={agent} onOpenThread={onOpenThread} />
      </div>
    );
  }
  const working = subagents.filter(isActive).length;
  const done = subagents.filter((t) => t.status === "done").length;
  const failed = subagents.filter((t) => t.status === "error").length;
  const summary = [
    working > 0 && `${working} working`,
    done > 0 && `${done} done`,
    failed > 0 && `${failed} failed`,
  ]
    .filter(Boolean)
    .join(" · ");
  const span = spanOf(subagents);
  const first = Math.min(...subagents.map((t) => t.startedAt ?? now));
  const elapsed = active ? now - first : span;
  return (
    <div className="pb-2">
      <button
        type="button"
        aria-expanded={open}
        aria-label={`${subagents.length} subagents`}
        onClick={() => setOpen((o) => !o)}
        className={`flex w-full min-w-0 cursor-pointer items-center gap-2.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-1.5 text-left hover:bg-hover ${open || active ? "text-fg" : "text-fg-muted opacity-55"}`}
      >
        <span className="flex flex-none items-center -space-x-1.5">
          {subagents.slice(0, 3).map((t) => (
            <Avatar key={t.id} agent={agent} />
          ))}
          {subagents.length > 3 && (
            <span className="grid h-6 min-w-6 place-items-center rounded-full bg-panel px-1 text-[10px] text-fg-muted ring-2 ring-[var(--color-editor)]">
              +{subagents.length - 3}
            </span>
          )}
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[length:var(--fs-xs)] font-semibold">
            {subagents.length} subagents
          </span>
          <span
            className={`truncate text-[length:var(--fs-xs)] ${failed > 0 ? "text-danger" : active ? "text-accent-text" : "text-fg-subtle"}`}
          >
            {summary}
          </span>
        </span>
        {elapsed !== null && (
          <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle tabular-nums">
            {fullElapsed(Math.max(0, elapsed))}
          </span>
        )}
        <CaretDownIcon
          size={14}
          className={`flex-none text-fg-muted transition-transform duration-200 ${open ? "rotate-180" : ""}`}
        />
      </button>
      {open && (
        <div className="timeline-expand timeline-expand-enter overflow-hidden">
          <div className="mt-1 flex flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1">
            {subagents.map((t) => (
              <SubagentCard
                key={t.id}
                tool={t}
                calls={calls}
                agent={agent}
                onOpenThread={onOpenThread}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function Avatar({ agent, dot }: { agent: AgentKind; dot?: string }) {
  return (
    <span className="relative grid h-6 w-6 flex-none place-items-center rounded-full border border-border bg-panel ring-2 ring-[var(--color-editor)]">
      <ProviderLogos kind={agent} size={13} />
      {dot && (
        <span
          className={`absolute -right-0.5 -bottom-0.5 h-2 w-2 rounded-full ring-2 ring-[var(--color-editor)] ${dot}`}
        />
      )}
    </span>
  );
}

/** A subagent: what it was asked to do, what it is doing or found, and for how long. It opens
 * its own thread when it has one. */
function SubagentCard({
  tool,
  calls,
  agent,
  onOpenThread,
}: {
  tool: ToolCall;
  calls: ToolCall[];
  agent: AgentKind;
  onOpenThread: (id: string) => void;
}) {
  const active = isActive(tool);
  const now = useNow(active);
  const status = SUBAGENT_STATUS[tool.status];
  const title = titleOf(tool);
  // While it works, what it is doing; once done, the first line of what it reported.
  const current = [...calls].reverse().find((c) => c.parent === tool.id && isActive(c));
  const result = tool.output
    ?.replace(/[`*_#>]/g, "")
    .split("\n")
    .map((l) => l.trim())
    .find(Boolean);
  const detail = active ? (current ? toolLabel(current) : undefined) : result;
  const elapsed =
    tool.startedAt === null ? null : Math.max(0, (tool.endedAt ?? now) - tool.startedAt);
  const body = (
    <>
      <Avatar agent={agent} dot={status.dot} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex min-w-0 items-baseline gap-2">
          <span className="truncate text-[length:var(--fs-xs)] font-medium text-fg">{title}</span>
          {detail && (active || tool.status !== "done") && (
            <span
              className={`flex-none text-[10px] ${tool.status === "error" ? "text-danger" : "text-fg-subtle"}`}
            >
              {status.label}
            </span>
          )}
        </span>
        <span className="truncate text-[11px] text-fg-muted">{detail ?? status.label}</span>
      </span>
      {elapsed !== null && (
        <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle tabular-nums">
          {fullElapsed(elapsed)}
        </span>
      )}
      {tool.childThreadId && <CaretRightIcon size={14} className="flex-none text-fg-subtle" />}
    </>
  );
  const className =
    "flex w-full min-w-0 items-center gap-2.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-1.5 text-left";
  const childThreadId = tool.childThreadId;
  if (!childThreadId) return <div className={className}>{body}</div>;
  return (
    <button
      type="button"
      aria-label={`Open ${title}`}
      onClick={() => onOpenThread(childThreadId)}
      className={`${className} cursor-pointer hover:bg-hover`}
    >
      {body}
    </button>
  );
}
