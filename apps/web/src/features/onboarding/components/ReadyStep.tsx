import { useId, type KeyboardEvent } from "react";
import { HardDrivesIcon, LaptopIcon } from "@phosphor-icons/react";
import { AGENT_LABELS, type AgentKind, type Settings } from "~/api";
import { cx } from "~/features/onboarding/classNames";

/** What the Imports step brought in: threads added, and projects made for their folders. */
export interface ImportResult {
  threads: number;
  projects: number;
}

interface ReadyStepProps {
  /** The connected agents, the default first. */
  agents: AgentKind[];
  imported: ImportResult | null;
  settings: Settings;
}

export default function ReadyStep({ agents, imported, settings }: ReadyStepProps) {
  const project = settings.projects.find((p) => p.id === settings.activeProjectId);
  const rows: [string, string][] = [
    [
      "Agents",
      agents.map((k, i) => (i === 0 ? `${AGENT_LABELS[k]} (default)` : AGENT_LABELS[k])).join(", "),
    ],
    ["Conversations", describeImport(imported)],
    ["Project", project?.name ?? "None yet"],
  ];
  return (
    <>
      <dl className="m-0 flex flex-col">
        {rows.map(([term, value]) => (
          <div
            key={term}
            className="grid grid-cols-[160px_1fr] gap-4 border-b border-border py-3 last:border-b-0"
          >
            <dt className="text-fg-muted">{term}</dt>
            <dd className="m-0 min-w-0 font-medium">{value}</dd>
          </div>
        ))}
      </dl>
      <RunsOn />
    </>
  );
}

function describeImport(imported: ImportResult | null): string {
  if (!imported || imported.threads === 0) return "None imported";
  const threads = `${imported.threads} ${imported.threads === 1 ? "conversation" : "conversations"}`;
  if (imported.projects === 0) return `${threads} imported`;
  const projects = `${imported.projects} new ${imported.projects === 1 ? "project" : "projects"}`;
  return `${threads} imported into ${projects}`;
}

const OPTION =
  "flex w-full items-center gap-4 rounded-[var(--radius)] border p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease]";
const TILE =
  "inline-flex h-10 w-10 flex-none items-center justify-center rounded-[var(--radius)] bg-selected-muted text-accent-text";

/** Where tasks run. Only this computer for now; a remote device is listed but can't be picked yet,
 *  and stays focusable so keyboard users and agents can read why. */
function RunsOn() {
  const label = useId();
  const soon = useId();
  // Arrow keys move between the options, as in any radio group.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)) return;
    e.preventDefault();
    const radios = [...e.currentTarget.querySelectorAll<HTMLElement>("[role=radio]")];
    const at = radios.indexOf(document.activeElement as HTMLElement);
    const step = e.key === "ArrowUp" || e.key === "ArrowLeft" ? -1 : 1;
    radios[(at + step + radios.length) % radios.length]?.focus();
  };
  return (
    <div className="mt-2 flex flex-col gap-2">
      <span id={label} className="text-[length:var(--fs-md)] font-semibold">
        Where tasks run
      </span>
      <div
        role="radiogroup"
        aria-labelledby={label}
        className="flex flex-col gap-2"
        onKeyDown={onKeyDown}
      >
        <button
          type="button"
          role="radio"
          aria-checked
          className={cx(OPTION, "cursor-default choice-selected")}
        >
          <span className={TILE}>
            <LaptopIcon size={20} />
          </span>
          <span className="flex-1 text-[length:var(--fs-lg)] font-semibold">This computer</span>
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={false}
          aria-disabled
          aria-describedby={soon}
          tabIndex={-1}
          className={cx(OPTION, "cursor-default border-border bg-transparent")}
        >
          <span className={cx(TILE, "opacity-60")}>
            <HardDrivesIcon size={20} />
          </span>
          <span className="flex-1 text-[length:var(--fs-lg)] font-semibold text-fg-muted">
            Remote device
          </span>
          <span id={soon} className="badge">
            Coming soon
          </span>
        </button>
      </div>
    </div>
  );
}
