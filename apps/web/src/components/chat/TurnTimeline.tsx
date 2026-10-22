import { useDeferredValue, useEffect, useState, type ReactNode } from "react";
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
import type { AgentKind, Message, Part, ToolCall } from "~/api";
import { fullElapsed, isActive, titleOf } from "~/components/chat/LineageCard";
import ProposedPlanCard from "~/components/chat/ProposedPlanCard";
import ProviderLogos from "~/components/ProviderLogos";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.
import Markdown from "~/components/chat/Markdown";

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
type Step =
  | { kind: "text"; text: string; index: number }
  | { kind: "work"; entries: Entry[] }
  /** Subagents started one after another, shown as cards. */
  | { kind: "agents"; tools: ToolCall[] }
  /** A plan proposed in plan mode. */
  | { kind: "plan"; text: string; index: number };

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
  onOpenThread,
}: {
  message: Message;
  agent: AgentKind;
  /** Opens a subagent's thread. */
  onOpenThread: (id: string) => void;
}) {
  const streaming = message.status === "streaming";
  const steps = stepsOf(message);
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
  // Subagents still working stay in view when the reply folds.
  const shown =
    folds && !open
      ? steps.filter((s, i) => i >= hidden || (s.kind === "agents" && s.tools.some(isActive)))
      : steps;
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
            <Markdown text={step === last && tail ? tail : step.text} />
          </div>
        ) : step.kind === "plan" ? (
          <div key={`p${step.index}`} className="px-1 py-0.5 pb-2">
            <ProposedPlanCard markdown={step.text} />
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
  if (message.cutOff) {
    const Caret = open ? CaretDownIcon : CaretRightIcon;
    return (
      <div className="mb-1.5 flex items-center gap-2 border-b border-border pt-1 pb-2">
        <button
          type="button"
          aria-expanded={open}
          disabled={hidden === 0}
          onClick={onToggle}
          className="flex cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-[length:var(--fs-md)] text-fg-muted hover:text-fg disabled:cursor-default disabled:hover:text-fg-muted"
        >
          <span>Superseded attempt</span>
          {hidden > 0 && <Caret size={13} />}
        </button>
        <span className="text-[length:var(--fs-sm)] text-fg-subtle">Cut off by steer</span>
      </div>
    );
  }
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

/** A row for work going on now, its label shimmering. */
function LiveRow({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <div className="flex min-h-6 min-w-0 items-center gap-1.5 px-0.5 py-0.5 text-[length:var(--fs-md)] text-fg-muted">
      <RowIcon>{icon}</RowIcon>
      <span className="tool-shine min-w-0 truncate">{label}</span>
    </div>
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
