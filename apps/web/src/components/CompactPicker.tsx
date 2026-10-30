import { ArrowsInSimpleIcon, CaretDownIcon } from "@phosphor-icons/react";
import type { AgentKind, TokenSettings } from "~/api";
import Dropdown from "~/components/Dropdown";
import { CHIP_CLASS } from "~/components/chip";
import { formatTokens } from "~/lib/tokens";

/** Context sizes to compact at, each twice the one before. */
export const COMPACT_LEVELS = [128_000, 256_000, 512_000];

/** Context sizes a chat can compact at; 0 leaves it to the agent, near its full window. */
const WINDOWS = [...COMPACT_LEVELS, 0];

/** The Settings → Tokens field that holds each agent's compact window. */
export const COMPACT_SETTINGS = {
  claude: "claudeCompactWindow",
  codex: "codexCompactWindow",
  antigravity: "antigravityCompactWindow",
  opencode: "opencodeCompactWindow",
  api: "apiCompactWindow",
} as const satisfies Record<AgentKind, keyof TokenSettings>;

const label = (window: number) => (window === 0 ? "When full" : formatTokens(window));

const storageKey = (agent: AgentKind) => `patchwork-compact-window-${agent}`;

/** The size last picked for `agent`, used for new threads; null follows Settings. */
export function lastCompactWindow(agent: AgentKind): number | null {
  try {
    const saved = localStorage.getItem(storageKey(agent));
    return saved !== null && WINDOWS.includes(Number(saved)) ? Number(saved) : null;
  } catch {
    return null;
  }
}

function rememberCompactWindow(agent: AgentKind, window: number | null) {
  try {
    if (window !== null) localStorage.setItem(storageKey(agent), String(window));
    else localStorage.removeItem(storageKey(agent));
  } catch {
    /* New threads follow Settings. */
  }
}

/** How full the context is against the size it compacts at, as a ring that turns the warning
 * color once compacting is near. */
function FillRing({ fill }: { fill: number }) {
  const r = 6;
  const length = 2 * Math.PI * r;
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 16 16"
      aria-hidden
      className={`flex-none -rotate-90 ${fill >= 0.9 ? "text-warning" : ""}`}
    >
      <circle cx={8} cy={8} r={r} fill="none" stroke="currentColor" strokeWidth={2} opacity={0.3} />
      {fill > 0 && (
        <circle
          cx={8}
          cy={8}
          r={r}
          fill="none"
          stroke="currentColor"
          strokeWidth={2}
          strokeDasharray={`${Math.min(fill, 1) * length} ${length}`}
        />
      )}
    </svg>
  );
}

/** The context size a chat compacts at, with how full it is, and a way to compact it now. */
export default function CompactPicker({
  agent,
  window,
  fallback,
  full,
  context,
  onPick,
  onCompactNow,
}: {
  agent: AgentKind;
  /** The chat's own size, or null to follow Settings. */
  window: number | null;
  /** The size Settings gives `agent`. */
  fallback: number;
  /** The context the chat's model holds, which "When full" fills; null when its CLI does not say. */
  full: number | null;
  /** The chat's context now, or null before a reply has reported it. */
  context: number | null;
  onPick: (window: number | null) => void;
  /** Compacts the chat now; null when there is nothing to compact or a turn is running. */
  onCompactNow: (() => void) | null;
}) {
  const effective = window ?? fallback;
  const limit = effective > 0 ? effective : full;
  const fill = context !== null && limit ? context / limit : 0;
  return (
    <Dropdown
      up
      trigger={({ open, toggle }) => (
        <button
          className={CHIP_CLASS}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`Compact at: ${label(effective)}`}
          title={
            context !== null
              ? `Context ${formatTokens(context)}, compacts at ${label(effective).toLowerCase()}`
              : "Compact at"
          }
          onClick={toggle}
        >
          {limit ? <FillRing fill={fill} /> : <ArrowsInSimpleIcon size={16} />}
          <span>{label(effective)}</span>
          <CaretDownIcon size={14} />
        </button>
      )}
    >
      {(close) => (
        <>
          {[null, ...WINDOWS].map((w) => (
            <button
              key={w ?? "default"}
              role="menuitemradio"
              aria-checked={w === window}
              className={`flex min-w-[160px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover ${w === window ? "bg-selected-muted" : ""}`}
              onClick={() => {
                close();
                rememberCompactWindow(agent, w);
                onPick(w);
              }}
            >
              {w === null ? `Default (${label(fallback)})` : label(w)}
            </button>
          ))}
          {onCompactNow && (
            <>
              <div className="my-1 h-px bg-border" />
              <button
                role="menuitem"
                className="flex min-w-[160px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover"
                onClick={() => {
                  close();
                  onCompactNow();
                }}
              >
                Compact now
              </button>
            </>
          )}
        </>
      )}
    </Dropdown>
  );
}
