import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowsClockwiseIcon, ChartBarIcon } from "@phosphor-icons/react";
import { api, AGENT_LABELS, type AgentKind, type AgentStatus, type ProviderUsage } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import Spinner from "~/components/Spinner";
import { describeAgent, errorText } from "~/lib/util";
import { useDismiss } from "~/hooks/useDismiss";

/** "in 3h 12m", "in 2d 4h", "in 5m". */
function resetsIn(at: number): string {
  const mins = Math.max(0, Math.round((at - Date.now()) / 60000));
  const d = Math.floor(mins / 1440);
  const h = Math.floor((mins % 1440) / 60);
  const m = mins % 60;
  if (d > 0) return `in ${d}d ${h}h`;
  if (h > 0) return `in ${h}h ${m}m`;
  return `in ${m}m`;
}

/** "4m", "1h 5m": the wait left, rounded up so it never reads 0m. */
function waitLeft(at: number, now: number): string {
  const mins = Math.max(1, Math.ceil((at - now) / 60000));
  return mins >= 60 ? `${Math.floor(mins / 60)}h ${mins % 60}m` : `${mins}m`;
}

/** When the provider allows the next usage check, while it is rate-limiting them. */
function limitedUntil(entry: Entry | undefined, now: number): number | null {
  const at = entry?.usage?.retryAt;
  return at != null && at > now ? at : null;
}

function level(percent: number): string {
  if (percent >= 90) return "stroke-danger";
  if (percent >= 70) return "stroke-warning";
  return "stroke-accent";
}

/** A circular gauge filled to `percent`, with `children` centered inside. */
function Ring({
  percent,
  size,
  stroke,
  children,
}: {
  percent: number | null;
  size: number;
  stroke: number;
  children?: ReactNode;
}) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const p = Math.min(100, Math.max(0, percent ?? 0));
  return (
    <div
      className="relative grid flex-none place-items-center"
      style={{ width: size, height: size }}
    >
      <svg width={size} height={size} className="absolute inset-0 -rotate-90" aria-hidden="true">
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          strokeWidth={stroke}
          className="stroke-selected-muted"
        />
        {p > 0 && (
          <circle
            cx={size / 2}
            cy={size / 2}
            r={r}
            fill="none"
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={c}
            strokeDashoffset={c * (1 - p / 100)}
            className={`${level(p)} transition-[stroke-dashoffset] duration-500`}
          />
        )}
      </svg>
      {children}
    </div>
  );
}

type Entry = {
  usage?: ProviderUsage;
  error?: string;
  status?: AgentStatus | null;
};

/** The agents with plan limits to show, whether or not they are switched on for new tasks. */
const PLAN_AGENTS: AgentKind[] = ["claude", "codex", "antigravity"];

/** How long "Up to date" stands in for the plan badge before the badge comes back. */
const UP_TO_DATE_MS = 1200;

/** Sidebar footer button that opens subscription usage for each installed agent with a plan, and
 * for OpenCode while it is switched on. */
export default function UsagePopover({ openCode }: { openCode: boolean }) {
  const [open, setOpen] = useState(false);
  const [entries, setEntries] = useState<Partial<Record<AgentKind, Entry>>>({});
  const shown = useMemo<AgentKind[]>(
    () => (openCode ? [...PLAN_AGENTS, "opencode"] : PLAN_AGENTS),
    [openCode],
  );
  const agents = shown.filter((kind) => entries[kind]?.status?.installed !== false);
  /** Ticks while a rate limit counts down. */
  const [now, setNow] = useState(Date.now);
  const limited = agents.some((kind) => limitedUntil(entries[kind], now) != null);
  /** The agent whose last refresh was turned down, and how: no new numbers, or a rate limit. */
  const [denied, setDenied] = useState<{ kind: AgentKind; how: "shake" | "denied" } | null>(null);
  /** The agent whose last refresh brought nothing new, while "Up to date" shows for it. */
  const [upToDate, setUpToDate] = useState<{ kind: AgentKind } | null>(null);
  useEffect(() => {
    if (!upToDate) return;
    const timer = window.setTimeout(() => setUpToDate(null), UP_TO_DATE_MS);
    return () => window.clearTimeout(timer);
  }, [upToDate]);
  /** When each agent's numbers were read, to tell a turned-down refresh from a new reading. */
  const readAt = useRef(new Map<AgentKind, number>());
  /** Agents whose usage is still being read; each one finishes on its own. */
  const [pending, setPending] = useState<AgentKind[]>([]);
  const [selected, setSelected] = useState<AgentKind | null>(null);
  const current = selected && agents.includes(selected) ? selected : agents[0];
  /** The refresh button belongs to the agent being looked at, and reads only that one. */
  const currentDenied = denied?.kind === current ? denied.how : null;
  const currentLimited = limitedUntil(current && entries[current], now) != null;
  const inFlight = useRef(new Set<AgentKind>());
  const ref = useRef<HTMLDivElement>(null);

  const load = useCallback((refresh: boolean, kinds: AgentKind[] = PLAN_AGENTS) => {
    // An agent still being read is skipped, so a slow one is not asked twice.
    kinds.forEach(async (kind) => {
      if (inFlight.current.has(kind)) return;
      inFlight.current.add(kind);
      setPending((prev) => [...prev, kind]);
      const status = await api.detectAgent(kind).catch(() => null);
      const entry: Entry = { status };
      if (status?.installed && status.signedIn) {
        try {
          entry.usage = await api.getUsage(kind, refresh);
        } catch (e) {
          entry.error = errorText(e);
        }
      }
      // The backend returns the last numbers when it skips a refresh, so the same reading time
      // means no new numbers came back. Only a rate limit from the provider shows as one.
      const before = readAt.current.get(kind);
      const after = entry.usage?.updatedAt;
      if (after != null) readAt.current.set(kind, after);
      if (refresh) {
        if (limitedUntil(entry, Date.now()) != null) setDenied({ kind, how: "denied" });
        else if (before != null && after === before) {
          setDenied({ kind, how: "shake" });
          setUpToDate({ kind });
        }
      }
      setNow(Date.now());
      setEntries((prev) => ({ ...prev, [kind]: entry }));
      inFlight.current.delete(kind);
      setPending((prev) => prev.filter((k) => k !== kind));
    });
  }, []);

  useEffect(() => {
    if (!open) return;
    setNow(Date.now());
    load(false, shown);
  }, [open, load, shown]);
  useEffect(() => {
    if (!open || !limited) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [open, limited]);
  useDismiss(ref, open, () => setOpen(false));

  return (
    <div className="relative flex-none py-1.5 px-2 border-t border-border" ref={ref}>
      <button
        className={`flex items-center gap-1.5 w-full h-7 px-2 border-none rounded-[var(--radius-sm)] bg-transparent text-fg-muted text-[length:var(--fs-sm)] cursor-pointer hover:bg-hover hover:text-fg${open ? " bg-hover text-fg" : ""}`}
        aria-label="Usage"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <ChartBarIcon size={18} />
        <span>Usage</span>
      </button>
      {open && (
        <div
          className="absolute left-2 right-2 bottom-[calc(100%_+_10px)] z-20 flex bg-elevated border border-border-strong rounded-[var(--radius)] overflow-hidden shadow-[0_4px_12px_rgb(0_0_0/0.2)]"
          role="dialog"
          aria-label="Usage"
        >
          {agents.length === 0 ? (
            <div className="p-3 text-fg-muted text-[length:var(--fs-sm)]">No agents connected</div>
          ) : (
            <>
              <div
                className="flex flex-none flex-col items-center gap-1.5 p-2 border-r border-border bg-app/40"
                role="tablist"
                aria-orientation="vertical"
                aria-label="Agents"
              >
                {agents.map((kind) => (
                  <AgentTab
                    key={kind}
                    kind={kind}
                    entry={entries[kind]}
                    now={now}
                    active={kind === current}
                    onSelect={() => {
                      setSelected(kind);
                      setDenied(null);
                    }}
                  />
                ))}
                <button
                  className={`btn-icon mt-auto w-10${currentDenied ? ` refresh-${currentDenied}` : ""}`}
                  title={currentLimited ? "Rate limited" : "Refresh"}
                  aria-label={currentLimited ? "Rate limited" : "Refresh"}
                  onClick={() => current && load(true, [current])}
                  onAnimationEnd={() => setDenied(null)}
                >
                  {current && pending.includes(current) ? (
                    <Spinner size={14} />
                  ) : (
                    <ArrowsClockwiseIcon size={14} />
                  )}
                </button>
              </div>
              {/* Every panel shares one grid cell, so the popover keeps the tallest
                  one's height and the tabs stay put while switching. */}
              <div className="grid min-w-0 flex-1">
                {agents.map((kind) => (
                  <AgentUsage
                    key={kind}
                    kind={kind}
                    entry={entries[kind]}
                    now={now}
                    upToDate={upToDate?.kind === kind}
                    hidden={kind !== current}
                  />
                ))}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** The highest used percentage across an agent's limits, or null before numbers arrive. */
function peak(entry: Entry | undefined): number | null {
  const windows = entry?.usage?.windows ?? [];
  return windows.length > 0 ? Math.max(...windows.map((w) => w.usedPercent)) : null;
}

function AgentTab({
  kind,
  entry,
  now,
  active,
  onSelect,
}: {
  kind: AgentKind;
  entry: Entry | undefined;
  now: number;
  active: boolean;
  onSelect: () => void;
}) {
  const top = peak(entry);
  const used = top == null ? null : `${Math.round(top)}% used`;
  const limit = limitedUntil(entry, now) != null ? "Rate limited" : null;
  return (
    <button
      className={`grid place-items-center w-10 h-10 p-0 border-none rounded-[var(--radius-sm)] cursor-pointer ${active ? "bg-hover" : "bg-transparent hover:bg-hover/60"}`}
      role="tab"
      aria-selected={active}
      aria-label={AGENT_LABELS[kind]}
      title={[AGENT_LABELS[kind], used, limit].filter(Boolean).join(" · ")}
      onClick={onSelect}
    >
      <Ring percent={top} size={34} stroke={3}>
        <ProviderLogos kind={kind} size={18} />
      </Ring>
    </button>
  );
}

function AgentUsage({
  kind,
  entry,
  now,
  upToDate,
  hidden,
}: {
  kind: AgentKind;
  entry: Entry | undefined;
  now: number;
  /** Whether a refresh of this agent brought nothing new, while "Up to date" shows. */
  upToDate: boolean;
  hidden: boolean;
}) {
  const usage = entry?.usage;
  const ready = entry?.status?.installed && entry.status.signedIn;
  const limit = limitedUntil(entry, now);
  // A rate limit's error reads "trying again in about N min", which the badge counts down
  // instead, and which is out of date once the wait is over.
  const rateLimited = usage?.retryAt != null;
  const note = !entry
    ? "Loading…"
    : !ready
      ? describeAgent(entry.status)
      : (entry.error ??
        (rateLimited
          ? null
          : (usage?.error ?? (usage?.windows.length === 0 ? "No limits reported" : null))));
  return (
    <section
      className={`[grid-area:1/1] flex min-w-0 flex-col gap-2.5 p-2.5${hidden ? " invisible" : ""}`}
      role="tabpanel"
      aria-label={AGENT_LABELS[kind]}
    >
      <div className="flex items-center gap-1.5 pb-2.5 border-b border-border">
        <span className="min-w-0 flex-1 truncate font-semibold">{AGENT_LABELS[kind]}</span>
        {limit != null ? (
          <span className="badge badge-danger" role="status">
            Rate limited<span aria-hidden="true"> · {waitLeft(limit, now)}</span>
          </span>
        ) : upToDate ? (
          <span
            className="inline-flex h-5 items-center text-fg-subtle text-[length:var(--fs-xs)]"
            role="status"
          >
            Up to date
          </span>
        ) : (
          usage?.plan && <span className="badge">{usage.plan}</span>
        )}
      </div>
      {note && <div className="text-fg-muted text-[length:var(--fs-sm)]">{note}</div>}
      {usage?.windows.map((w) => (
        <div key={w.label} className="flex items-center gap-2">
          <Ring percent={w.usedPercent} size={42} stroke={3.5}>
            <span className="tabular-nums font-medium text-[length:11px]">
              {Math.round(w.usedPercent)}%
            </span>
          </Ring>
          <div className="flex min-w-0 flex-col">
            <span className="truncate text-[length:var(--fs-sm)]">{w.label}</span>
            {w.resetsAt && (
              <span className="truncate text-fg-subtle text-[length:var(--fs-xs)]">
                Resets {resetsIn(w.resetsAt)}
              </span>
            )}
          </div>
        </div>
      ))}
    </section>
  );
}
