import { useEffect, useRef, useState } from "react";
import {
  CaretRightIcon,
  CheckIcon,
  DotsThreeIcon,
  PlugsIcon,
  SquareIcon,
  TerminalIcon,
  XCircleIcon,
  XIcon,
} from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import { api, type StepStatus, type Thread, type ToolCall } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import { errorText } from "~/lib/util";

export const DOT_COLOR: Record<StepStatus, string> = {
  pending: "bg-fg-subtle",
  running: "bg-accent",
  done: "bg-success",
  error: "bg-danger",
};

/** How long a row is hovered before its details card opens. */
const HOVER_DELAY_MS = 200;

/** Tool calls of the current turn: every assistant message since the last non-steer user message. */
function turnTools(thread: Thread): ToolCall[] {
  let start = -1;
  thread.messages.forEach((m, i) => {
    if (m.role === "user" && !m.steer) start = i;
  });
  return thread.messages
    .slice(start + 1)
    .filter((m) => m.role === "assistant")
    .flatMap((m) => m.tools);
}

/** Running or about to run. Finished subagents are "done" or "error". */
export function isActive(tool: ToolCall): boolean {
  return tool.status === "running" || tool.status === "pending";
}

export function titleOf(tool: ToolCall): string {
  return tool.subagent?.description || tool.subagent?.kind || tool.name;
}

/** Time a call has run, or null when its start is unknown. */
export function elapsedMs(tool: ToolCall, now: number): number | null {
  if (tool.startedAt === null) return null;
  return Math.max(0, (tool.endedAt ?? now) - tool.startedAt);
}

/** Compact duration for a row: "42s", "3m", "1.5h", "12h". */
export function compactElapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = s / 3600;
  return h < 10 ? `${Number(h.toFixed(1))}h` : `${Math.floor(h)}h`;
}

/** Full duration for the details card: "42s", "3m 05s", "2h 14m". */
export function fullElapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

/** Floating card to the left of a row: what the subagent is and what it did last. */
function DetailsCard({
  tool,
  tools,
  elapsed,
}: {
  tool: ToolCall;
  tools: ToolCall[];
  elapsed: number | null;
}) {
  const actions = tools.filter((t) => t.parent === tool.id && !t.subagent);
  const last = actions[actions.length - 1];
  return (
    <div className="pointer-events-none absolute top-0 right-full z-20 mr-2 flex w-64 flex-col gap-1.5 rounded-[var(--radius)] border border-border bg-panel p-2.5 text-[length:var(--fs-sm)] shadow-[var(--shadow-popup)]">
      <div className="font-semibold break-words">{titleOf(tool)}</div>
      <div className="text-fg-muted">
        {`${tool.subagent?.model ?? "inherits model"} · ${tool.subagent?.kind ?? ""}`}
      </div>
      <div className="flex items-center gap-1.5">
        {tool.status === "running" && (
          <>
            <Spinner className="text-accent" />
            <span className="text-accent">Running</span>
          </>
        )}
        {tool.status === "done" && (
          <>
            <CheckIcon size={12} className="flex-none text-success" />
            <span className="text-success">Done</span>
          </>
        )}
        {tool.status === "error" && (
          <>
            <XCircleIcon size={12} className="flex-none text-danger" />
            <span className="text-danger">Failed</span>
          </>
        )}
        {tool.status === "pending" && <span className="text-fg-subtle">Pending</span>}
        {elapsed !== null && (
          <span className="ml-auto tabular-nums text-fg-muted">{fullElapsed(elapsed)}</span>
        )}
      </div>
      {last && (
        <div className="flex min-w-0 items-center gap-1.5 font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] text-fg-muted">
          <TerminalIcon size={12} className="flex-none" />
          <span className="truncate">{`${last.name} ${last.summary}`}</span>
        </div>
      )}
    </div>
  );
}

/** One subagent: provider avatar with a status dot, title, elapsed time, and a stop button while running. */
function SubagentRow({
  tool,
  thread,
  tools,
  now,
  onError,
}: {
  tool: ToolCall;
  thread: Thread;
  tools: ToolCall[];
  now: number;
  onError: (message: string) => void;
}) {
  const [hovered, setHovered] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const elapsed = elapsedMs(tool, now);
  const stoppable = tool.status === "running" && thread.agent === "claude";
  const title = titleOf(tool);

  function enter() {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setHovered(true), HOVER_DELAY_MS);
  }

  function leave() {
    window.clearTimeout(timer.current);
    setHovered(false);
  }

  function stop() {
    api.stopSubagent(thread.id, tool.id).catch((e: unknown) => onError(errorText(e)));
  }

  return (
    <div
      className="group relative flex min-w-0 items-center gap-2 rounded-[var(--radius)] px-1.5 py-1 hover:bg-hover"
      onMouseEnter={enter}
      onMouseLeave={leave}
    >
      <span className="relative grid h-5 w-5 flex-none place-items-center">
        <ProviderLogos kind={thread.agent} model={thread.model} size={16} />
        <span
          className={`absolute right-0 bottom-0 h-[7px] w-[7px] rounded-full ring-2 ring-[var(--color-panel)] ${DOT_COLOR[tool.status]}`}
        />
      </span>
      <span className="min-w-0 flex-1 truncate text-[length:var(--fs-sm)] font-medium text-fg">
        {title}
      </span>
      <span className="relative flex flex-none items-center justify-end">
        {tool.status === "error" ? (
          <span className="text-[length:var(--fs-xs)] text-danger">Failed</span>
        ) : (
          elapsed !== null && (
            <span
              className={`tabular-nums text-[length:var(--fs-xs)] text-fg-subtle${stoppable ? " transition-opacity group-hover:opacity-0" : ""}`}
            >
              {compactElapsed(elapsed)}
            </span>
          )
        )}
        {stoppable && (
          <button
            type="button"
            className="absolute top-1/2 right-0 grid h-5 w-5 -translate-y-1/2 cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-transparent p-0 text-danger opacity-0 hover:bg-hover focus-visible:opacity-100 group-hover:opacity-100"
            aria-label={`Stop subagent ${title}`}
            title="Stop subagent"
            onClick={stop}
          >
            <SquareIcon size={10} weight="fill" />
          </button>
        )}
      </span>
      {hovered && <DetailsCard tool={tool} tools={tools} elapsed={elapsed} />}
    </div>
  );
}

/** Subagents of the current turn, split into running ones and previous ones, with a disconnect menu. */
export default function LineageCard({
  thread,
  working,
  onDisconnect,
  onError,
}: {
  thread: Thread;
  working: boolean;
  onDisconnect: () => void;
  onError: (message: string) => void;
}) {
  const [now, setNow] = useState(() => Date.now());
  // The newest subagent per thread when the card was closed; it reopens once another starts.
  const [closedAt, setClosedAt] = useState<Record<string, string>>({});
  const [previousOpen, setPreviousOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  const tools = turnTools(thread);
  const subagents = tools.filter((t) => t.subagent);
  const live = subagents.some(isActive);

  // Ticks the elapsed times once a second, only while a subagent is still going.
  useEffect(() => {
    if (!live) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [live]);

  useEffect(() => {
    if (!menuOpen) return;
    function onPointerDown(e: MouseEvent) {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") setMenuOpen(false);
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [menuOpen]);

  const newest = subagents.at(-1)?.id;
  if (newest === undefined || closedAt[thread.id] === newest) return null;
  const shownUpTo = newest;

  const active = subagents.filter(isActive);
  const previous = subagents.filter((t) => !isActive(t));
  const running = subagents.filter((t) => t.status === "running").length;

  function close() {
    setMenuOpen(false);
    setClosedAt((c) => ({ ...c, [thread.id]: shownUpTo }));
  }

  return (
    <div className="absolute top-3 right-4 z-10 w-[280px] rounded-[var(--radius-lg)] border border-border bg-panel text-[length:var(--fs-sm)] shadow-[var(--shadow-popup)]">
      <div className="flex items-center gap-2 px-3 py-2">
        <span className="flex-1 truncate font-medium text-[length:var(--fs-xs)] text-fg-muted">
          {running > 0 ? `Lineage · ${running} running` : "Lineage"}
        </span>
        <div className="relative" ref={menuRef}>
          <button
            type="button"
            className="grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-transparent p-0 text-fg-subtle hover:bg-hover hover:text-fg"
            aria-label="Lineage actions"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((o) => !o)}
          >
            <DotsThreeIcon size={14} />
          </button>
          {menuOpen && (
            <div
              role="menu"
              className="absolute top-full right-0 z-20 mt-1 flex w-56 flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]"
            >
              <button
                type="button"
                role="menuitem"
                disabled={!working}
                className="flex w-full cursor-pointer items-center gap-2 rounded-[var(--radius)] border-none bg-transparent px-2 py-1.5 text-left text-fg hover:bg-hover disabled:cursor-default disabled:text-fg-subtle disabled:hover:bg-transparent"
                onClick={() => {
                  setMenuOpen(false);
                  onDisconnect();
                }}
              >
                <PlugsIcon size={14} className="flex-none text-fg-muted" />
                Disconnect agent session
              </button>
              <button
                type="button"
                role="menuitem"
                className="flex w-full cursor-pointer items-center gap-2 rounded-[var(--radius)] border-none bg-transparent px-2 py-1.5 text-left text-fg hover:bg-hover"
                onClick={close}
              >
                <XIcon size={14} className="flex-none text-fg-muted" />
                Close lineage
              </button>
            </div>
          )}
        </div>
      </div>
      {active.length > 0 && (
        <div className="flex flex-col gap-px px-1.5 pb-1.5">
          {active.map((tool) => (
            <SubagentRow
              key={tool.id}
              tool={tool}
              thread={thread}
              tools={tools}
              now={now}
              onError={onError}
            />
          ))}
        </div>
      )}
      {previous.length > 0 && (
        <div className="border-t border-border">
          <button
            type="button"
            className="flex w-full cursor-pointer items-center gap-1.5 border-none bg-transparent px-3 py-1.5 text-left text-[length:var(--fs-xs)] text-fg-muted hover:text-fg"
            aria-expanded={previousOpen}
            onClick={() => setPreviousOpen((o) => !o)}
          >
            <CaretRightIcon size={10} className={previousOpen ? "rotate-90" : undefined} />
            Previous agents
            {!previousOpen && ` (${previous.length})`}
          </button>
          {previousOpen && (
            <div className="flex flex-col gap-px px-1.5 pb-1.5">
              {previous.map((tool) => (
                <SubagentRow
                  key={tool.id}
                  tool={tool}
                  thread={thread}
                  tools={tools}
                  now={now}
                  onError={onError}
                />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
