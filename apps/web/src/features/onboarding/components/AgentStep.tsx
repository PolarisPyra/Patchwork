import { useId, type KeyboardEvent } from "react";
import { ArrowClockwiseIcon, CheckIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import { AGENT_LABELS, type AgentKind, type CliKind } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import { cx } from "~/features/onboarding/classNames";
import {
  AGENT_KINDS,
  isReady,
  type AgentProbe,
  type AgentProbes,
} from "~/features/onboarding/agentProbes";

interface AgentStepProps {
  probes: AgentProbes;
  checking: boolean;
  selected: AgentKind[];
  onToggle: (kind: AgentKind) => void;
  /** The agent new tasks start with; one of `selected`. */
  defaultAgent: AgentKind | null;
  onDefault: (kind: AgentKind) => void;
  onRecheck: () => void;
}

export default function AgentStep({
  probes,
  checking,
  selected,
  onToggle,
  defaultAgent,
  onDefault,
  onRecheck,
}: AgentStepProps) {
  return (
    <>
      <div className="flex items-center justify-between">
        <span className="text-[length:var(--fs-md)] font-semibold">Available coding agents</span>
        <button
          type="button"
          className="link disabled:cursor-default disabled:opacity-50 disabled:hover:no-underline"
          disabled={checking}
          onClick={onRecheck}
        >
          {checking ? <Spinner size={14} /> : <ArrowClockwiseIcon size={14} />}
          Check again
        </button>
      </div>
      <div className="flex flex-col gap-2" role="group" aria-label="Coding agents">
        {AGENT_KINDS.map((kind) => (
          <AgentRow
            key={kind}
            kind={kind}
            probe={probes[kind]}
            selected={selected.includes(kind)}
            onToggle={onToggle}
          />
        ))}
      </div>
      {selected.length > 0 && (
        <DefaultAgent selected={selected} value={defaultAgent} onChange={onDefault} />
      )}
    </>
  );
}

/** Picks which connected agent new tasks start with. */
function DefaultAgent({
  selected,
  value,
  onChange,
}: {
  selected: AgentKind[];
  value: AgentKind | null;
  onChange: (kind: AgentKind) => void;
}) {
  const label = useId();
  // Arrow keys pick the next or previous agent, as in any radio group.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 }[e.key];
    if (!step) return;
    e.preventDefault();
    const at = value ? selected.indexOf(value) : 0;
    const next = (at + step + selected.length) % selected.length;
    onChange(selected[next]);
    e.currentTarget.querySelectorAll<HTMLElement>("[role=radio]")[next]?.focus();
  };
  return (
    <div className="mt-2 flex flex-col gap-4">
      <span id={label} className="text-[length:var(--fs-md)] font-semibold">
        Default agent
      </span>
      {/* One tile per connected agent, as many to a row as fit. */}
      <div
        role="radiogroup"
        aria-labelledby={label}
        className="grid grid-cols-[repeat(auto-fit,minmax(150px,1fr))] gap-2"
        onKeyDown={onKeyDown}
      >
        {selected.map((kind) => {
          const checked = kind === value;
          return (
            <button
              key={kind}
              type="button"
              role="radio"
              aria-checked={checked}
              tabIndex={checked ? 0 : -1}
              className={cx(
                "flex cursor-pointer flex-col items-center gap-2.5 rounded-[var(--radius)] border px-3 pt-4 pb-3 text-[length:var(--fs-sm)] transition-[background-color,border-color,color] duration-[0.12s] ease-[ease]",
                checked
                  ? "choice-selected font-semibold text-fg"
                  : "border-border bg-transparent text-fg-muted hover:bg-hover hover:text-fg",
              )}
              onClick={() => onChange(kind)}
            >
              <span className="inline-flex h-10 w-10 items-center justify-center rounded-[var(--radius)] bg-selected-muted">
                <ProviderLogos kind={kind} size={22} />
              </span>
              {AGENT_LABELS[kind]}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function AgentRow({
  kind,
  probe,
  selected,
  onToggle,
}: {
  kind: CliKind;
  probe: AgentProbe;
  selected: boolean;
  onToggle: (kind: AgentKind) => void;
}) {
  const ready = isReady(probe);
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={selected}
      disabled={!ready}
      className={cx(
        "flex w-full items-center gap-4 rounded-[var(--radius)] border p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease] not-disabled:cursor-pointer",
        selected ? "choice-selected" : "border-border bg-transparent not-disabled:hover:bg-hover",
      )}
      onClick={() => onToggle(kind)}
    >
      <span className="inline-flex h-11 w-11 flex-none items-center justify-center rounded-[var(--radius)] bg-selected-muted text-[length:var(--fs-lg)] font-semibold text-accent-text">
        <ProviderLogos kind={kind} size={26} />
      </span>
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-[length:var(--fs-lg)] font-semibold">{AGENT_LABELS[kind]}</span>
        <span className="text-[length:var(--fs-sm)] text-fg-muted">
          {probe.status?.detail ?? ""}
        </span>
      </span>
      <span className="flex flex-none items-center gap-3">
        <StatusBadge probe={probe} />
        {ready && (
          <span
            className={cx(
              "inline-flex h-[18px] w-[18px] flex-none items-center justify-center rounded-[var(--radius-sm)] border text-on-accent",
              selected ? "border-accent bg-accent" : "border-border-strong",
            )}
          >
            {selected && <CheckIcon size={12} weight="bold" />}
          </span>
        )}
      </span>
    </button>
  );
}

// Every badge is as wide as the longest usual one, so they line up from row to row.
const BADGE = "badge min-w-[112px] justify-center tabular-nums";

function StatusBadge({ probe }: { probe: AgentProbe }) {
  if (probe.checking) {
    return (
      <span className={BADGE}>
        <Spinner />
        Checking…
      </span>
    );
  }
  const { status } = probe;
  if (!status) return <span className={BADGE}>Check failed</span>;
  if (!status.installed) return <span className={BADGE}>Not installed</span>;
  if (!status.signedIn) return <span className={`${BADGE} badge-danger`}>Not signed in</span>;
  return (
    <span className={`${BADGE} badge-success`}>
      {status.version ? `Ready · ${status.version}` : "Ready"}
    </span>
  );
}
