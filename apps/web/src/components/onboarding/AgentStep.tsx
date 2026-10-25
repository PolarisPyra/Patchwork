import { ArrowClockwiseIcon, CheckIcon, CircleNotchIcon } from "@phosphor-icons/react";
import { AGENT_LABELS, type AgentKind, type CliKind } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import { cx } from "~/components/onboarding/classNames";
import {
  AGENT_KINDS,
  isReady,
  type AgentProbe,
  type AgentProbes,
} from "~/components/onboarding/agentProbes";

interface AgentStepProps {
  probes: AgentProbes;
  checking: boolean;
  selected: AgentKind[];
  onToggle: (kind: AgentKind) => void;
  onRecheck: () => void;
}

export default function AgentStep({
  probes,
  checking,
  selected,
  onToggle,
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
          <ArrowClockwiseIcon size={14} className={checking ? "spin" : undefined} />
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
    </>
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
        "flex w-full items-center gap-4 rounded-[var(--radius)] border p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease] not-disabled:cursor-pointer not-disabled:hover:bg-hover",
        selected ? "border-accent bg-accent-soft" : "border-border bg-transparent",
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

function StatusBadge({ probe }: { probe: AgentProbe }) {
  if (probe.checking) {
    return (
      <span className="badge">
        <CircleNotchIcon size={12} className="spin" />
        Checking…
      </span>
    );
  }
  const { status } = probe;
  if (!status) return <span className="badge">Check failed</span>;
  if (!status.installed) return <span className="badge">Not installed</span>;
  if (!status.signedIn) return <span className="badge badge-danger">Not signed in</span>;
  return (
    <span className="badge badge-success">
      {status.version ? `Ready · v${status.version}` : "Ready"}
    </span>
  );
}
