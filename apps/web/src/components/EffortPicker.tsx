import { useMemo } from "react";
import { LightbulbIcon, CaretDownIcon } from "@phosphor-icons/react";
import type { AgentKind, ModelOption } from "~/api";
import Dropdown from "~/components/Dropdown";
import { CHIP_CLASS } from "~/components/chip";

const LABELS: Record<string, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
  ultra: "Ultra",
};

const label = (effort: string) =>
  LABELS[effort] ?? effort.charAt(0).toUpperCase() + effort.slice(1);

/**
 * The effort levels each CLI takes on every model it has. Used only for a model whose own
 * levels the CLI does not report: `claude --effort` and `agy --effort` take all five, Codex takes
 * `effort` on each turn, and the rest are the levels most providers share.
 */
export const EFFORTS: Record<AgentKind, string[]> = {
  claude: ["low", "medium", "high", "xhigh", "max"],
  codex: ["low", "medium", "high", "xhigh"],
  antigravity: ["low", "medium", "high", "xhigh", "max"],
  opencode: ["low", "medium", "high"],
  // Sent as `reasoning_effort`, which OpenAI-style reasoning models take.
  api: ["low", "medium", "high"],
};

/**
 * The levels `model` takes, as its CLI reports them: `None` when the CLI does not say, so the
 * agent's shared list is the best guess, and an empty list when the model takes none.
 */
export function modelEfforts(
  agent: AgentKind,
  model: string | null,
  models: Partial<Record<AgentKind, ModelOption[]>>,
): string[] | null {
  if (!model) return null;
  return models[agent]?.find((option) => option.id === model)?.efforts ?? null;
}

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
  levels,
  onPick,
}: {
  agent: AgentKind;
  effort: string | null;
  /** The current model's own levels, or null when its CLI does not report them. */
  levels: string[] | null;
  onPick: (effort: string | null) => void;
}) {
  const options = useMemo(() => levels ?? EFFORTS[agent], [levels, agent]);
  const current = effort && options.includes(effort) ? effort : null;
  // A model that takes no level has nothing to pick, so the chip would only take up room.
  if (options.length === 0) return null;
  return (
    <Dropdown
      up
      label="Effort"
      trigger={({ open, toggle }) => (
        <button
          className={CHIP_CLASS}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`Effort: ${current ? label(current) : "Default"}`}
          title="Effort"
          onClick={toggle}
        >
          <LightbulbIcon size={16} />
          <span>{current ? label(current) : "Default"}</span>
          <CaretDownIcon size={14} />
        </button>
      )}
    >
      {(close) =>
        [null, ...options].map((e) => (
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
            {e ? label(e) : "Default"}
          </button>
        ))
      }
    </Dropdown>
  );
}
