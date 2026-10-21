import {
  createContext,
  lazy,
  memo,
  Suspense,
  useContext,
  useDeferredValue,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { LegendList, type LegendListRef } from "@legendapp/list/react";
import {
  ArrowClockwiseIcon,
  ArrowDownIcon,
  CaretDownIcon,
  CaretRightIcon,
  CheckCircleIcon,
  CheckIcon,
  CircleIcon,
  CircleNotchIcon,
  GitDiffIcon,
  ShieldWarningIcon,
  WarningIcon,
  XCircleIcon,
} from "@phosphor-icons/react";
import {
  AGENT_LABELS,
  type AgentKind,
  type Approval,
  type FileChange,
  type Message,
  type MessageStatus,
  type PlanStep,
  type StepStatus,
  type Thread,
  type TokenSettings,
  type TokenUsage,
  type ToolCall,
} from "~/api";
import type { CanvasTab } from "~/components/canvas/Canvas";
import AttachmentChip, { AttachmentTile } from "~/components/chat/AttachmentChip";
import { ACTION_CLASS, CopyButton } from "~/components/chat/CopyButton";
import { compactElapsed, fullElapsed, isActive, titleOf } from "~/components/chat/LineageCard";
import ProviderLogos from "~/components/ProviderLogos";
import { formatTokens } from "~/lib/tokens";
import { formatClock } from "~/lib/util";

// Markdown pulls in the renderer, so it loads on first use; plain text shows until then.
const Markdown = lazy(() => import("~/components/chat/Markdown"));

type OpenCanvas = (path: string, message: Message | null, tab: CanvasTab) => void;

/** The shell the user chose for commands, or null to follow the system. */
export const ShellContext = createContext<string | null>(null);

/** Claude names every shell command "Bash"; show the shell the command runs in instead. */
function toolLabel(name: string, shell: string | null): string {
  if (name !== "Bash") return name;
  return shell ? (shell.split("/").pop() ?? shell) : "shell";
}

interface MessagesProps {
  thread: Thread;
  changes: FileChange[];
  busy: boolean;
  /** Omitted for chats without a project: the change summary is hidden. */
  onOpenCanvas?: OpenCanvas;
  /** Opens the sidebar's Changes view. */
  onShowChanges: (paths: string[]) => void;
  onRetry: (message: Message) => void;
  tokens: TokenSettings;
}

export default function Messages({
  thread,
  changes,
  busy,
  onOpenCanvas,
  onShowChanges,
  onRetry,
  tokens,
}: MessagesProps) {
  // Queued messages wait above the composer until the turn ends, so they are not shown here.
  const messages = useMemo(() => thread.messages.filter((m) => !m.queued), [thread.messages]);
  const listRef = useRef<LegendListRef>(null);
  const [atEnd, setAtEnd] = useState(true);

  // The agent's working row follows the latest message, after any steering messages.
  // A null row is the working row.
  const working = messages.some((m) => m.role === "assistant" && m.status === "streaming");
  const rows: (Message | null)[] = working ? [...messages, null] : messages;

  // The same threshold the list uses to keep following the end, so the button only shows when the
  // list will not follow on its own.
  useEffect(
    () => listRef.current?.getState().listen("isWithinMaintainScrollAtEndThreshold", setAtEnd),
    [],
  );

  // Start at the latest message and jump to it again after each send. While a reply streams, the
  // list keeps following the end as long as the reader is at the bottom.
  const lastUserMessageId = [...messages].reverse().find((m) => m.role === "user")?.id;
  const lastSent = useRef(lastUserMessageId);
  useLayoutEffect(() => {
    if (lastSent.current === lastUserMessageId) return; // mount: initialScrollAtEnd handles it
    lastSent.current = lastUserMessageId;
    void listRef.current?.scrollToEnd();
  }, [lastUserMessageId]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <LegendList
        ref={listRef}
        data={rows}
        keyExtractor={(row) => row?.id ?? "working"}
        extraData={tokens}
        getItemType={(row) => row?.role ?? "working"}
        estimatedItemSize={90}
        initialScrollAtEnd
        maintainScrollAtEnd
        maintainVisibleContentPosition
        className="min-h-0 flex-1 overflow-x-hidden [overflow-anchor:none]"
        renderItem={({ item, index }) => {
          if (!item) {
            return (
              <div className="px-6">
                <div className="flex max-w-[820px] flex-col pb-5">
                  <WorkingRow agent={thread.agent} />
                </div>
              </div>
            );
          }
          if (item.role === "user" && item.text.trim() === "/compact") {
            const before = [...messages.slice(0, index)].reverse().find((m) => m.usage)?.usage;
            return (
              <div className="px-6">
                <div className="flex pb-5">
                  <CompactedDivider before={tokens.showUsage ? before?.context : undefined} />
                </div>
              </div>
            );
          }
          const previous = messages[index - 1];
          const usage =
            item.role === "assistant" && item.status !== "streaming" && tokens.showUsage;
          return (
            <div className="px-6">
              <div
                className={`flex flex-col pb-5${index === 0 ? " pt-6" : ""}${item.role === "user" ? "" : " max-w-[820px]"}`}
              >
                {item.role === "user" ? (
                  <UserMessage message={item} />
                ) : (
                  <AssistantMessage
                    message={item}
                    agent={thread.agent}
                    changes={changes}
                    retryMessage={previous?.role === "user" ? previous : null}
                    busy={busy}
                    onOpenCanvas={onOpenCanvas}
                    onShowChanges={onShowChanges}
                    onRetry={onRetry}
                  />
                )}
              </div>
              {usage && item.usage && (
                // Pulled up into the message's bottom padding, and out to the column's right edge.
                <div className="-mt-2 mb-1 flex justify-end">
                  <UsageLine usage={item.usage} warnAbove={tokens.warnAbove} />
                </div>
              )}
            </div>
          );
        }}
      />
      {!atEnd && (
        <button
          type="button"
          className="absolute bottom-3 left-1/2 grid h-8 w-8 -translate-x-1/2 cursor-pointer place-items-center rounded-full border border-border bg-panel text-fg shadow-[var(--shadow-popup)] hover:bg-hover"
          aria-label="Jump to latest message"
          onClick={() => void listRef.current?.scrollToEnd({ animated: true })}
        >
          <ArrowDownIcon size={14} />
        </button>
      )}
    </div>
  );
}

const UserMessage = memo(function UserMessage({ message }: { message: Message }) {
  return (
    <div className="flex max-w-[75%] flex-col items-end gap-1 self-end">
      <div className="px-2 text-[length:var(--fs-xs)] text-fg-subtle">
        You · {formatClock(message.createdAt)}
        {message.steer && " · Steer"}
      </div>
      {message.attachments.length > 0 && (
        <div className="flex flex-wrap justify-end gap-1.5">
          {message.attachments.map((a) =>
            a.kind === "image" ? (
              <AttachmentTile key={a.path} attachment={a} />
            ) : (
              <AttachmentChip key={a.path} attachment={a} />
            ),
          )}
        </div>
      )}
      {message.text && (
        <div className="rounded-[12px] bg-[color-mix(in_srgb,var(--text)_10%,var(--bg-editor))] px-4 py-2.5 leading-[1.55] whitespace-pre-wrap text-fg [word-break:break-word] selectable [&_h3]:mt-0 [&_h3]:mb-0">
          <Suspense fallback={<div className="whitespace-pre-wrap">{message.text}</div>}>
            <Markdown text={message.text} />
          </Suspense>
        </div>
      )}
    </div>
  );
});

interface AssistantProps {
  message: Message;
  agent: AgentKind;
  changes: FileChange[];
  retryMessage: Message | null;
  busy: boolean;
  onOpenCanvas?: OpenCanvas;
  onShowChanges: (paths: string[]) => void;
  onRetry: (message: Message) => void;
}

const AssistantMessage = memo(function AssistantMessage({
  message,
  agent,
  changes,
  retryMessage,
  busy,
  onOpenCanvas,
  onShowChanges,
  onRetry,
}: AssistantProps) {
  const streaming = message.status === "streaming";
  const subagents = message.tools.filter((t) => t.subagent && !t.parent);
  const actions = message.tools.filter((t) => !t.parent && !t.subagent);
  // The newest call still running, including calls a subagent is making.
  const running = streaming
    ? message.tools.filter((t) => t.status === "running" && !t.subagent).at(-1)
    : undefined;
  const live = useHeldTool(running);
  // While text streams in, React may skip renders of the reply in favor of input and scrolling.
  const text = useDeferredValue(message.text);
  return (
    <section className="flex min-w-0 flex-col gap-2.5">
      {/* While streaming, WorkingRow shows the agent instead, after the latest message. */}
      {!streaming && (
        <AgentLabel
          agent={agent}
          status={message.status}
          workedMs={message.finishedAt === null ? null : message.finishedAt - message.createdAt}
        />
      )}
      {text && (
        <Suspense fallback={<div className="whitespace-pre-wrap">{text}</div>}>
          <Markdown text={text} />
        </Suspense>
      )}
      {streaming && message.thinking && <ThinkingLine text={message.thinking} />}
      {message.plan.length > 0 && <PlanList steps={message.plan} />}
      {subagents.length > 0 && (
        <SubagentGroup subagents={subagents} tools={message.tools} agent={agent} />
      )}
      {actions.length > 0 && <ToolActivity tools={actions} />}
      {streaming && live && <LiveTool tool={live} />}
      {/* A pending approval is answered in the banner above the composer. */}
      {message.approvals.map((approval) =>
        approval.status === "pending" ? (
          <p
            key={approval.id}
            className="m-0 flex items-center gap-1.5 px-1 text-[length:var(--fs-sm)] text-warning"
          >
            <ShieldWarningIcon size={15} />
            Waiting for approval: {approval.title}
          </p>
        ) : (
          <ApprovalCard key={approval.id} approval={approval} />
        ),
      )}
      {onOpenCanvas && message.files.length > 0 && (
        <ChangeSummary
          paths={message.files}
          changes={changes}
          onShow={() => onShowChanges(message.files)}
        />
      )}
      {message.error && (
        <div className="rounded-[var(--radius)] bg-[var(--danger-soft)] px-3 py-2 text-[length:var(--fs-sm)] whitespace-pre-wrap text-danger [word-break:break-word]">
          {message.error}
        </div>
      )}
      {!streaming && (
        <div className="flex gap-1">
          <CopyButton text={message.text} />
          {retryMessage !== null && (
            <button className={ACTION_CLASS} disabled={busy} onClick={() => onRetry(retryMessage)}>
              <ArrowClockwiseIcon size={13} /> Retry
            </button>
          )}
          <span className="ml-1.5 inline-flex h-6 items-center text-[length:var(--fs-xs)] text-fg-muted">
            {formatClock(message.createdAt)}
          </span>
        </div>
      )}
    </section>
  );
});

/** The tokens a reply used. A context over `warnAbove` is flagged, since every turn re-reads it. */
function UsageLine({ usage, warnAbove }: { usage: TokenUsage; warnAbove: number }) {
  const large = usage.context > warnAbove;
  return (
    <p
      className={`m-0 flex flex-wrap items-center justify-end gap-x-3 px-1 text-[length:var(--fs-xs)] tabular-nums ${large ? "text-warning" : "text-fg-muted"}`}
    >
      <span className="inline-flex items-center gap-1">
        {large && <WarningIcon size={13} />}
        {large
          ? `Context over ${formatTokens(warnAbove)}`
          : `Context ${formatTokens(usage.context)}`}
      </span>
      <span>Cache read {formatTokens(usage.cacheRead)} tokens</span>
      <span>Cache write {formatTokens(usage.cacheWrite)} tokens</span>
      <span>Input {formatTokens(usage.input)} tokens</span>
      <span>Output {formatTokens(usage.output)} tokens</span>
    </p>
  );
}

/** A resolved approval, kept in the transcript with its outcome. */
function CompactedDivider({ before }: { before: number | undefined }) {
  return (
    <div className="flex w-full items-center gap-3 text-[length:var(--fs-xs)] text-fg-subtle">
      <span className="h-px flex-1 bg-border" />
      <span>
        Context compacted{before !== undefined && ` · was ${formatTokens(before)} tokens`}
      </span>
      <span className="h-px flex-1 bg-border" />
    </div>
  );
}

function ApprovalCard({ approval }: { approval: Approval }) {
  const { title, detail, status } = approval;
  return (
    <div className="flex flex-col gap-2 rounded-[var(--radius)] border border-border-strong bg-panel px-3 py-2.5 text-[length:var(--fs-sm)]">
      <div
        className={`flex items-center gap-1.5 ${status === "allowed" ? "text-success" : "text-danger"}`}
      >
        {status === "allowed" ? <CheckCircleIcon size={15} /> : <XCircleIcon size={15} />}
        <span className="font-semibold text-fg">{title}</span>
        <span className="ml-auto">{status === "allowed" ? "Allowed" : "Denied"}</span>
      </div>
      {detail && (
        <code className="rounded-[var(--radius-sm)] bg-editor px-2 py-1.5 font-[family-name:var(--font-mono)] break-all whitespace-pre-wrap selectable">
          {detail}
        </code>
      )}
    </div>
  );
}

function AgentLabel({
  agent,
  status,
  workedMs,
}: {
  agent: AgentKind;
  status: MessageStatus;
  /** How long the reply ran, once it has finished. */
  workedMs: number | null;
}) {
  return (
    <header className="flex min-w-0 items-center gap-2">
      <span className="flex-none text-[length:var(--fs-xs)] font-semibold tracking-[0.05em] text-fg-muted uppercase">
        {AGENT_LABELS[agent]}
      </span>
      <StatusBadge status={status} workedMs={workedMs} />
    </header>
  );
}

function WorkingRow({ agent }: { agent: AgentKind }) {
  return (
    <section className="flex min-w-0 flex-col gap-2.5">
      <AgentLabel agent={agent} status="streaming" workedMs={null} />
    </section>
  );
}

function StatusBadge({ status, workedMs }: { status: MessageStatus; workedMs: number | null }) {
  switch (status) {
    case "streaming":
      return (
        <span className="badge badge-accent">
          <CircleNotchIcon size={12} className="spin" />
          Working…
        </span>
      );
    case "complete":
      return (
        <span className="badge badge-accent min-w-0">
          <CheckIcon size={12} className="flex-none" />
          <span className="truncate">
            Task complete
            {workedMs !== null && ` · Worked for ${fullElapsed(workedMs)}`}
          </span>
        </span>
      );
    case "error":
      return <span className="badge badge-danger">Failed</span>;
    case "cancelled":
      return <span className="badge">Stopped</span>;
  }
}

function StepIcon({ status }: { status: StepStatus }) {
  switch (status) {
    case "done":
      return <CheckIcon size={14} className="mt-0.5 flex-none text-accent-text" />;
    case "running":
      return <CircleNotchIcon size={14} className="mt-0.5 flex-none spin text-accent-text" />;
    case "error":
      return <XCircleIcon size={14} className="mt-0.5 flex-none text-danger" />;
    case "pending":
      return <CircleIcon size={14} className="mt-0.5 flex-none text-fg-subtle" />;
  }
}

function PlanList({ steps }: { steps: PlanStep[] }) {
  return (
    <ol className="m-0 flex list-none flex-col gap-1.5 rounded-[var(--radius)] border border-border bg-panel px-3 py-2.5">
      {steps.map((step, i) => (
        <li key={i} className="flex items-start gap-2">
          <StepIcon status={step.status} />
          <span>{step.text}</span>
        </li>
      ))}
    </ol>
  );
}

function ToolActivity({ tools }: { tools: ToolCall[] }) {
  const [open, setOpen] = useState(false);
  const shell = useContext(ShellContext);
  return (
    <div>
      <button
        className="inline-flex cursor-pointer items-center gap-1 self-start rounded-[var(--radius-sm)] border-none bg-transparent px-1 py-0.5 text-[length:var(--fs-sm)] text-fg-muted hover:bg-hover hover:text-fg"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <CaretRightIcon size={14} className={open ? "rotate-90" : undefined} />
        {tools.length} {tools.length === 1 ? "action" : "actions"}
      </button>
      {open && (
        <ul className="m-0 mt-1 flex list-none flex-col gap-1 border-l-2 border-border px-2 py-1.5 font-[family-name:var(--font-mono)] text-[12px]">
          {tools.map((tool) => (
            <li key={tool.id} className="flex min-w-0 items-center gap-2 [&>svg]:mt-0">
              <StepIcon status={tool.status} />
              <span className="flex-none font-semibold">{toolLabel(tool.name, shell)}</span>
              <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-fg-muted">
                {tool.summary}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** The end of the agent's reasoning so far, muted, while it works. */
function ThinkingLine({ text }: { text: string }) {
  const tail = text.replace(/\s+/g, " ").trim().slice(-280);
  return (
    <p className="m-0 line-clamp-3 px-1 text-[length:var(--fs-sm)] text-fg-muted italic">{tail}</p>
  );
}

/** Shortest time a call stays on the live line, so a quick one can still be read. */
const MIN_SHOW_MS = 1500;

/**
 * The call the live line shows. A new call replaces the shown one only after it has been
 * up for MIN_SHOW_MS. When nothing is running the last one stays, so the line does not
 * blink out between calls.
 */
function useHeldTool(running: ToolCall | undefined): ToolCall | undefined {
  const [shown, setShown] = useState(running);
  const shownSince = useRef(Date.now());
  useEffect(() => {
    if (!running || running.id === shown?.id) return;
    const wait = Math.max(0, MIN_SHOW_MS - (Date.now() - shownSince.current));
    const id = window.setTimeout(() => {
      shownSince.current = Date.now();
      setShown(running);
    }, wait);
    return () => window.clearTimeout(id);
  }, [running, shown]);
  return shown;
}

/** The call running now, its label shimmering until it ends. */
function LiveTool({ tool }: { tool: ToolCall }) {
  const shell = useContext(ShellContext);
  return (
    <div className="flex min-w-0 items-center gap-1.5 px-1 text-[length:var(--fs-sm)]">
      <span className="tool-shine flex-none">Running {toolLabel(tool.name, shell)}</span>
      {tool.summary && (
        <span className="min-w-0 truncate font-[family-name:var(--font-mono)] text-[12px] text-fg-subtle">
          {tool.summary}
        </span>
      )}
    </div>
  );
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

/** Time from the first subagent's start to the last one's end (or now), if known. */
function groupElapsed(subagents: ToolCall[], now: number): number | null {
  const starts = subagents.flatMap((t) => (t.startedAt === null ? [] : [t.startedAt]));
  if (starts.length === 0) return null;
  const ends = subagents.map((t) => (isActive(t) ? now : (t.endedAt ?? now)));
  return Math.max(0, Math.max(...ends) - Math.min(...starts));
}

/** The subagents a message started, as one row: avatars, how many still work, elapsed time. */
function SubagentGroup({
  subagents,
  tools,
  agent,
}: {
  subagents: ToolCall[];
  /** Every call of the message, including the ones the subagents made. */
  tools: ToolCall[];
  agent: AgentKind;
}) {
  const [open, setOpen] = useState(false);
  const shell = useContext(ShellContext);
  const working = subagents.filter(isActive).length;
  const failed = subagents.filter((t) => t.status === "error").length;
  const now = useNow(working > 0);
  const elapsed = groupElapsed(subagents, now);
  const count = subagents.length;
  return (
    <div>
      <button
        className="flex w-full min-w-0 cursor-pointer items-center gap-2 rounded-[var(--radius-sm)] border-none bg-transparent px-1 py-1 text-left text-[length:var(--fs-sm)] hover:bg-hover"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="flex flex-none items-center -space-x-1.5">
          {subagents.slice(0, 3).map((t) => (
            <span
              key={t.id}
              className="grid h-5 w-5 place-items-center rounded-full bg-panel ring-2 ring-[var(--color-editor)]"
            >
              <ProviderLogos kind={agent} size={13} />
            </span>
          ))}
          {count > 3 && (
            <span className="grid h-5 min-w-5 place-items-center rounded-full bg-panel px-1 text-[10px] text-fg-muted ring-2 ring-[var(--color-editor)]">
              +{count - 3}
            </span>
          )}
        </span>
        <span className="flex-none font-medium text-fg">
          {count} {count === 1 ? "subagent" : "subagents"}
        </span>
        {working > 0 && <span className="flex-none text-accent-text">{working} working</span>}
        {failed > 0 && <span className="flex-none text-danger">{failed} failed</span>}
        <span className="flex-1" />
        {elapsed !== null && (
          <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle tabular-nums">
            {compactElapsed(elapsed)}
          </span>
        )}
        <CaretDownIcon
          size={14}
          className={`flex-none text-fg-muted transition-transform ${open ? "rotate-180" : ""}`}
        />
      </button>
      {open && (
        <ul className="m-0 mt-1 flex list-none flex-col gap-1 rounded-[var(--radius)] border border-border bg-panel p-1.5">
          {subagents.map((t) => {
            const calls = tools.filter((c) => c.parent === t.id);
            const current = calls.filter((c) => c.status === "running").at(-1);
            return (
              <li key={t.id} className="flex min-w-0 items-center gap-2 px-1 py-0.5">
                <StepIcon status={t.status} />
                <span className="min-w-0 flex-none truncate font-medium">{titleOf(t)}</span>
                {current ? (
                  <span className="tool-shine min-w-0 truncate text-[length:var(--fs-sm)]">
                    Running {toolLabel(current.name, shell)}
                  </span>
                ) : (
                  <span className="min-w-0 truncate text-[length:var(--fs-sm)] text-fg-muted">
                    {calls.length} {calls.length === 1 ? "action" : "actions"}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/** One line for the files a message touched; the list itself lives in the Changes view. */
function ChangeSummary({
  paths,
  changes,
  onShow,
}: {
  paths: string[];
  changes: FileChange[];
  onShow: () => void;
}) {
  const found = changes.filter((c) => paths.includes(c.path));
  const additions = found.reduce((sum, c) => sum + c.additions, 0);
  const deletions = found.reduce((sum, c) => sum + c.deletions, 0);
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[var(--radius)] border border-border bg-panel px-3 py-2 text-[length:var(--fs-sm)]">
      <GitDiffIcon size={14} className="flex-none text-fg-muted" />
      <span className="whitespace-nowrap">
        {paths.length} {paths.length === 1 ? "file" : "files"} changed
      </span>
      {found.length > 0 && (
        <>
          <span className="text-success">+{additions}</span>
          <span className="text-danger">−{deletions}</span>
        </>
      )}
      <span className="flex-1" />
      <button className="btn flex-none" onClick={onShow}>
        Show changes
      </button>
    </div>
  );
}
