import { lazy, Suspense, useDeferredValue, useEffect, useState, type ReactNode } from "react";
import {
  BrainIcon,
  CaretDownIcon,
  CaretRightIcon,
  EyeIcon,
  GlobeIcon,
  MagnifyingGlassIcon,
  NotePencilIcon,
  RobotIcon,
  TerminalIcon,
  WarningCircleIcon,
  WrenchIcon,
} from "@phosphor-icons/react";
import type { Message, Part, ToolCall } from "~/api";
import { fullElapsed, titleOf } from "~/components/chat/LineageCard";

// Markdown pulls in the renderer, so it loads on first use; plain text shows until then.
const Markdown = lazy(() => import("~/components/chat/Markdown"));

/** What a tool call does, which picks its icon, its verb, and how a group of calls is summed up. */
type Action = "edit" | "command" | "read" | "search" | "web" | "agent" | "other";

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
};

/** Calls that are not shown: the plan they keep is shown above the composer. */
const HIDDEN_TOOLS = new Set(["TodoWrite"]);

function actionOf(tool: ToolCall): Action {
  return tool.subagent ? "agent" : (ACTIONS[tool.name] ?? "other");
}

const ACTION_ICONS: Record<Action, ReactNode> = {
  edit: <NotePencilIcon size={15} />,
  command: <TerminalIcon size={15} />,
  read: <EyeIcon size={15} />,
  search: <MagnifyingGlassIcon size={15} />,
  web: <GlobeIcon size={15} />,
  agent: <RobotIcon size={15} />,
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
type Step = { kind: "text"; text: string; index: number } | { kind: "work"; entries: Entry[] };

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
    let entry: Entry;
    if (part.type === "thinking") {
      if (!part.text.trim()) return;
      entry = { kind: "thinking", part };
    } else {
      const tool = byId.get(part.id);
      if (!shown(tool)) return;
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
export default function TurnTimeline({ message }: { message: Message }) {
  const streaming = message.status === "streaming";
  const steps = stepsOf(message);
  // The final text stays out of the fold, with any work after it. A failed reply stays open.
  const lastText = steps.map((s) => s.kind).lastIndexOf("text");
  const hidden = lastText === -1 ? steps.length : lastText;
  const stopped = message.status === "cancelled";
  const folds = !streaming && message.status !== "error" && (hidden > 0 || stopped);
  const [open, setOpen] = useState(false);
  const shown = folds && !open ? steps.slice(hidden) : steps;
  const last = steps.at(-1);
  // Text still streaming in may skip renders in favor of input and scrolling.
  const tail = useDeferredValue(last?.kind === "text" ? last.text : "");
  const idle =
    streaming && (last === undefined || (last.kind === "work" && !last.entries.some(entryActive)));
  return (
    <div className="flex min-w-0 flex-col">
      {streaming && <WorkingRow since={message.createdAt} />}
      {folds && (
        <FoldRow
          message={message}
          open={open}
          onToggle={() => setOpen((o) => !o)}
          hidden={hidden}
        />
      )}
      {shown.map((step) =>
        step.kind === "text" ? (
          <div key={`t${step.index}`} className="px-1 py-0.5 pb-2">
            <Suspense fallback={<div className="whitespace-pre-wrap">{step.text}</div>}>
              <Markdown text={step === last && tail ? tail : step.text} />
            </Suspense>
          </div>
        ) : (
          <WorkGroup key={workKey(step.entries[0])} entries={step.entries} live={streaming} />
        ),
      )}
      {idle && <LiveRow icon={<BrainIcon size={15} />} label="Thinking" />}
    </div>
  );
}

function workKey(entry: Entry): string {
  return entry.kind === "tool" ? entry.tool.id : `r${entry.part.startedAt}`;
}

function WorkingRow({ since }: { since: number }) {
  const now = useNow(true);
  return (
    <div className="mb-1.5 border-b border-border pt-1 pb-2">
      <div className="flex h-6 items-center px-1 text-[length:var(--fs-md)] text-fg-muted tabular-nums">
        Working for {fullElapsed(Math.max(0, now - since))}
      </div>
    </div>
  );
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
  const took =
    message.finishedAt === null ? null : fullElapsed(message.finishedAt - message.createdAt);
  const label = stopped
    ? took
      ? `You stopped after ${took}`
      : "You stopped the response"
    : took
      ? `Worked for ${took}`
      : "Worked";
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  return (
    <div className="mb-1.5 flex items-center border-b border-border pt-1 pb-2">
      <button
        type="button"
        aria-expanded={open}
        disabled={hidden === 0}
        onClick={onToggle}
        className="flex cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-[length:var(--fs-md)] text-fg-muted tabular-nums hover:text-fg disabled:cursor-default disabled:hover:text-fg-muted"
      >
        <span>{label}</span>
        {hidden > 0 && <Caret size={13} />}
      </button>
    </div>
  );
}

/** A run of work between texts: one row, or a summary row over several that opens to list them.
 * While its last call runs, the summary shows that call instead. */
function WorkGroup({ entries, live }: { entries: Entry[]; live: boolean }) {
  const [open, setOpen] = useState(false);
  if (entries.length === 1) {
    return (
      <div className="pb-2">
        <EntryRow entry={entries[0]} live={live} />
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
        <span className={`min-w-0 flex-1 truncate ${current ? "tool-shine" : ""}`}>
          {current ? entryLabel(current) : groupLabel(tools)}
        </span>
      </button>
      {open && (
        <div className="flex flex-col">
          {entries.map((entry) => (
            <EntryRow key={workKey(entry)} entry={entry} live={live} />
          ))}
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

function entryIcon(entry: Entry): ReactNode {
  if (entry.kind === "thinking") return <BrainIcon size={15} />;
  if (entry.tool.status === "error") return <WarningCircleIcon size={15} className="text-danger" />;
  return ACTION_ICONS[actionOf(entry.tool)];
}

/** Reasoning reads as its own first line once done. */
function entryLabel(entry: Entry): string {
  if (entry.kind === "tool") return toolLabel(entry.tool);
  if (entry.part.endedAt === null) return "Thinking";
  return entry.part.text.replace(/\s+/g, " ").trim() || "Thought";
}

/** One call or block of reasoning; it opens to show the command and its output, or the whole
 * reasoning. */
function EntryRow({ entry, live }: { entry: Entry; live: boolean }) {
  const [open, setOpen] = useState(false);
  const active = live && entryActive(entry);
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  return (
    <div className="min-w-0">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={ROW_CLASS}
      >
        <RowIcon>{entryIcon(entry)}</RowIcon>
        <span className={`min-w-0 flex-1 truncate ${active ? "tool-shine" : ""}`}>
          {entryLabel(entry)}
        </span>
        <Caret size={12} className="flex-none text-fg-subtle opacity-0 group-hover:opacity-100" />
      </button>
      {open && <EntryDetails entry={entry} />}
    </div>
  );
}

const DETAILS_CLASS =
  "ms-7 mt-0.5 mb-1 max-h-64 overflow-auto rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted selectable";

function EntryDetails({ entry }: { entry: Entry }) {
  if (entry.kind === "thinking") {
    return (
      <div className={DETAILS_CLASS}>
        <Suspense fallback={<div className="whitespace-pre-wrap">{entry.part.text}</div>}>
          <Markdown text={entry.part.text} />
        </Suspense>
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

/** A row for work going on now, its label shimmering. */
function LiveRow({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <div className="flex min-h-6 min-w-0 items-center gap-1.5 px-0.5 py-0.5 text-[length:var(--fs-md)] text-fg-muted">
      <RowIcon>{icon}</RowIcon>
      <span className="tool-shine min-w-0 truncate">{label}</span>
    </div>
  );
}
