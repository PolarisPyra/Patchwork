import { LightbulbIcon, CaretDownIcon } from "@phosphor-icons/react";
import type { AgentKind } from "~/api";
import Dropdown from "~/components/workspace/Dropdown";
import { CHIP_CLASS } from "~/components/chat/ModelPicker";

const LABELS: Record<string, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
};

/**
 * The thinking levels each CLI accepts: `claude --effort` and `agy --effort` take all five;
 * Codex takes `effort` on each turn, and these are the levels its current models share.
 */
export const EFFORTS: Record<AgentKind, string[]> = {
  claude: ["low", "medium", "high", "xhigh", "max"],
  codex: ["low", "medium", "high", "xhigh"],
  antigravity: ["low", "medium", "high", "xhigh", "max"],
  // OpenCode passes these as the model's variant; the levels most providers share.
  opencode: ["low", "medium", "high"],
};

/** `effort` if `agent` accepts it, else null (the CLI's default). */
export function effortFor(agent: AgentKind, effort: string | null): string | null {
  return effort && EFFORTS[agent].includes(effort) ? effort : null;
}

const storageKey = (agent: AgentKind) => `patchwork-effort-${agent}`;

/** The level last picked for `agent`, used for new threads. */
export function lastEffort(agent: AgentKind): string | null {
  try {
    return effortFor(agent, localStorage.getItem(storageKey(agent)));
  } catch {
    return null;
  }
}

function rememberEffort(agent: AgentKind, effort: string | null) {
  try {
    if (effort) localStorage.setItem(storageKey(agent), effort);
    else localStorage.removeItem(storageKey(agent));
  } catch {
    /* New threads fall back to the CLI's default. */
  }
}

export default function EffortPicker({
  agent,
  effort,
  onPick,
}: {
  agent: AgentKind;
  effort: string | null;
  onPick: (effort: string | null) => void;
}) {
  const current = effortFor(agent, effort);
  const label = current ? LABELS[current] : "Default";
  const options: (string | null)[] = [null, ...EFFORTS[agent]];
  return (
    <Dropdown
      up
      trigger={({ open, toggle }) => (
        <button
          className={CHIP_CLASS}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`Thinking: ${label}`}
          title="Thinking"
          onClick={toggle}
        >
          <LightbulbIcon size={16} />
          <span>{label}</span>
          <CaretDownIcon size={14} />
        </button>
      )}
    >
      {(close) =>
        options.map((e) => (
          <button
            key={e ?? "default"}
            role="menuitemradio"
            aria-checked={e === current}
            className={`flex min-w-[160px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover ${e === current ? "bg-selected-muted" : ""}`}
            onClick={() => {
              close();
              rememberEffort(agent, e);
              onPick(e);
            }}
          >
            {e ? LABELS[e] : "Default"}
          </button>
        ))
      }
    </Dropdown>
  );
}
