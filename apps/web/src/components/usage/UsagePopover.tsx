import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowsClockwiseIcon, ChartBarIcon } from "@phosphor-icons/react";
import { api, AGENT_LABELS, type AgentKind, type AgentStatus, type ProviderUsage } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import { describeAgent, errorText } from "~/lib/util";

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
  /** Why an explicit refresh did not bring new numbers. */
  refreshNote?: string;
};

/** Sidebar footer button that opens subscription usage for each connected agent. */
export default function UsagePopover({ agents }: { agents: AgentKind[] }) {
  const [open, setOpen] = useState(false);
  const [entries, setEntries] = useState<Partial<Record<AgentKind, Entry>>>({});
  /** Agents whose usage is still being read; each one finishes on its own. */
  const [pending, setPending] = useState<AgentKind[]>([]);
  const [selected, setSelected] = useState<AgentKind | null>(null);
  const current = selected && agents.includes(selected) ? selected : agents[0];
  const inFlight = useRef(new Set<AgentKind>());
  const ref = useRef<HTMLDivElement>(null);

  const load = useCallback(
    (refresh: boolean) => {
      // An agent still being read is skipped, so a slow one is not asked twice.
      agents.forEach(async (kind) => {
        if (inFlight.current.has(kind)) {
          if (refresh) {
            setEntries((prev) => ({
              ...prev,
              [kind]: { ...prev[kind], refreshNote: "Still reading; refresh skipped." },
            }));
          }
          return;
        }
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
        setEntries((prev) => {
          // The backend returns the last numbers when it throttles a refresh, so the
          // same reading time means no new numbers came back.
          const before = prev[kind]?.usage?.updatedAt;
          const after = entry.usage?.updatedAt;
          const throttled = refresh && before != null && after === before;
          if (throttled) {
            const time = new Date(after).toLocaleTimeString([], {
              hour: "numeric",
              minute: "2-digit",
            });
            entry.refreshNote = `Refresh is limited; showing numbers from ${time}.`;
          }
          return { ...prev, [kind]: entry };
        });
        inFlight.current.delete(kind);
        setPending((prev) => prev.filter((k) => k !== kind));
      });
    },
    [agents],
  );

  useEffect(() => {
    if (!open) return;
    load(false);
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node | null)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, load]);

  return (
    <div className="relative flex-none py-1.5 px-2 border-t border-border" ref={ref}>
      <button
        className={`flex items-center gap-1.5 w-full h-7 px-2 border-none rounded-[var(--radius-sm)] bg-transparent text-fg-muted text-[length:var(--fs-sm)] cursor-pointer hover:bg-hover hover:text-fg${open ? " bg-hover text-fg" : ""}`}
        aria-label="Usage"
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
              >
                {agents.map((kind) => (
                  <AgentTab
                    key={kind}
                    kind={kind}
                    entry={entries[kind]}
                    active={kind === current}
                    onSelect={() => setSelected(kind)}
                  />
                ))}
                <button
                  className="btn-icon mt-auto w-10"
                  title="Refresh"
                  aria-label="Refresh"
                  onClick={() => load(true)}
                >
                  <ArrowsClockwiseIcon
                    size={14}
                    className={pending.length > 0 ? "spin" : undefined}
                  />
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
  active,
  onSelect,
}: {
  kind: AgentKind;
  entry: Entry | undefined;
  active: boolean;
  onSelect: () => void;
}) {
  const top = peak(entry);
  return (
    <button
      className={`grid place-items-center w-10 h-10 p-0 border-none rounded-[var(--radius-sm)] cursor-pointer ${active ? "bg-hover" : "bg-transparent hover:bg-hover/60"}`}
      role="tab"
      aria-selected={active}
      aria-label={AGENT_LABELS[kind]}
      title={top == null ? AGENT_LABELS[kind] : `${AGENT_LABELS[kind]} · ${Math.round(top)}% used`}
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
  hidden,
}: {
  kind: AgentKind;
  entry: Entry | undefined;
  hidden: boolean;
}) {
  const usage = entry?.usage;
  const ready = entry?.status?.installed && entry.status.signedIn;
  const note = !entry
    ? "Loading…"
    : !ready
      ? describeAgent(entry.status)
      : (entry.error ??
        usage?.error ??
        (usage?.windows.length === 0 ? "No limits reported" : null));
  return (
    <section
      className={`[grid-area:1/1] flex min-w-0 flex-col gap-2.5 p-2.5${hidden ? " invisible" : ""}`}
      role="tabpanel"
      aria-label={AGENT_LABELS[kind]}
    >
      <div className="flex items-center gap-1.5 pb-2.5 border-b border-border">
        <span className="min-w-0 flex-1 truncate font-semibold">{AGENT_LABELS[kind]}</span>
        {usage?.plan && <span className="badge">{usage.plan}</span>}
      </div>
      {note && <div className="text-fg-muted text-[length:var(--fs-sm)]">{note}</div>}
      {entry?.refreshNote && (
        <div className="text-fg-subtle text-[length:var(--fs-xs)]">{entry.refreshNote}</div>
      )}
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
