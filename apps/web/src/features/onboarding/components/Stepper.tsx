import { CheckIcon } from "@phosphor-icons/react";
import { cx } from "~/features/onboarding/classNames";

// A four-point star, drawn in a 10 by 10 box.
const SPARKLE = "M5 0Q5.6 4.4 10 5Q5.6 5.6 5 10Q4.4 5.6 0 5Q4.4 4.4 5 0Z";

interface StepperProps {
  labels: string[];
  current: number;
  /** Highest step index the user has reached; later steps are not clickable. */
  reached: number;
  onSelect: (index: number) => void;
}

export default function Stepper({ labels, current, reached, onSelect }: StepperProps) {
  return (
    <nav aria-label="Setup steps">
      <ol className="m-0 flex list-none items-center p-0">
        {labels.map((label, i) => {
          const isCurrent = i === current;
          const isFilled = i <= current;
          const isDone = i < current;
          return (
            <li key={label} className="flex items-center">
              {i > 0 && (
                <span
                  className="mx-3 h-1 w-12 overflow-hidden rounded-full bg-border-strong"
                  aria-hidden
                >
                  <span
                    className="step-line-fill block h-full"
                    style={{ transform: `scaleX(${isFilled ? 1 : 0})` }}
                  />
                </span>
              )}
              <button
                type="button"
                className="flex items-center gap-5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-1 text-fg-muted not-disabled:cursor-pointer disabled:cursor-default"
                disabled={i > reached}
                aria-current={isCurrent ? "step" : undefined}
                // The checkmark alone doesn't tell a screen reader the step is finished.
                aria-label={isDone ? `${label}, done` : undefined}
                onClick={() => onSelect(i)}
              >
                <span
                  className={cx(
                    // Tabular figures and no extra line height, so each digit sits in the middle.
                    "step-dot relative inline-flex h-6 w-6 flex-none items-center justify-center rounded-full text-[length:var(--fs-sm)] font-semibold leading-none tabular-nums",
                    isFilled ? "step-dot-filled" : "bg-selected-muted text-fg-subtle",
                    isCurrent && "step-dot-current",
                  )}
                >
                  {isDone ? <CheckIcon className="step-check-in" size={14} weight="bold" /> : i + 1}
                  {isCurrent && (
                    <span className="step-halo" aria-hidden>
                      <span className="step-halo-ring" />
                      <span className="step-bubble step-bubble-1" />
                      <span className="step-bubble step-bubble-2" />
                      <span className="step-bubble step-bubble-3" />
                      <svg className="step-sparkle step-sparkle-1" viewBox="0 0 10 10">
                        <path d={SPARKLE} />
                      </svg>
                      <svg className="step-sparkle step-sparkle-2" viewBox="0 0 10 10">
                        <path d={SPARKLE} />
                      </svg>
                    </span>
                  )}
                </span>
                <span className={cx(isFilled && "text-fg", isCurrent && "font-semibold")}>
                  {label}
                </span>
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
