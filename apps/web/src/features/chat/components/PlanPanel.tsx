import { useState } from "react";
import {
  CaretDownIcon,
  CaretRightIcon,
  CheckIcon,
  CircleIcon,
  ListChecksIcon,
  XCircleIcon,
} from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import type { PlanStep, StepStatus } from "~/api";

function StepIcon({ status }: { status: StepStatus }) {
  switch (status) {
    case "done":
      return <CheckIcon size={14} className="mt-0.5 flex-none text-accent-text" />;
    case "running":
      return <Spinner size={14} className="mt-0.5 text-accent-text" />;
    case "error":
      return <XCircleIcon size={14} className="mt-0.5 flex-none text-danger" />;
    case "pending":
      return <CircleIcon size={14} className="mt-0.5 flex-none text-fg-subtle" />;
  }
}

/** The agent's plan above the composer: how far it has got, opening to the steps. It arrives as
 * its one line: arriving open took the height of every step from the chat and moved it up. */
export default function PlanPanel({ steps }: { steps: PlanStep[] }) {
  const [open, setOpen] = useState(false);
  const done = steps.filter((s) => s.status === "done").length;
  const running = steps.find((s) => s.status === "running");
  // A stopped turn puts its running step back to pending; the header keeps naming it, not
  // whichever step it would otherwise fall back to.
  const [last, setLast] = useState(running?.text);
  if (running && running.text !== last) setLast(running.text);
  const current = running ?? steps.find((s) => s.text === last && s.status === "pending");
  const Caret = open ? CaretDownIcon : CaretRightIcon;
  return (
    <div className="mb-1.5 rounded-[var(--radius)] border border-border bg-panel text-[length:var(--fs-sm)]">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex w-full min-w-0 cursor-pointer items-center gap-1.5 border-none bg-transparent px-2.5 py-1.5 text-left text-fg-muted hover:text-fg"
      >
        <ListChecksIcon size={14} className="flex-none" />
        <span className="flex-none font-medium text-fg tabular-nums">
          Tasks {done}/{steps.length}
        </span>
        {current && <span className="min-w-0 flex-1 truncate">{current.text}</span>}
        {steps.length > 0 && (
          <span
            className="ml-auto flex h-2 flex-none items-center gap-1"
            role="img"
            aria-label={`${done} of ${steps.length} tasks complete${current ? `; current task: ${current.text}` : ""}`}
            title={`${done} of ${steps.length} tasks complete`}
          >
            {steps.slice(0, 8).map((step, i) => (
              <span
                key={i}
                className={`h-1.5 w-3 rounded-full transition-colors ${
                  step.status === "done"
                    ? "bg-accent-text"
                    : step.status === "running"
                      ? "bg-accent shadow-[0_0_6px_var(--color-accent)]"
                      : step.status === "error"
                        ? "bg-danger"
                        : "bg-fg-subtle/25"
                }`}
                title={`${step.status}: ${step.text}`}
              />
            ))}
            {steps.length > 8 && (
              <span className="ml-0.5 text-[length:var(--fs-xs)] text-fg-subtle">
                +{steps.length - 8}
              </span>
            )}
          </span>
        )}
        <Caret size={12} className={`flex-none ${steps.length > 0 ? "" : "ml-auto"}`} />
      </button>
      {open && (
        <ol className="m-0 flex list-none flex-col gap-1.5 border-t border-border px-3 py-2.5">
          {steps.map((step, i) => (
            <li key={i} className="flex items-start gap-2">
              <StepIcon status={step.status} />
              <span>{step.text}</span>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
