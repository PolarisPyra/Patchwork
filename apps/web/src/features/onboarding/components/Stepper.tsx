import { cx } from "~/features/onboarding/lib/classNames";

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
          return (
            <li key={label} className="flex items-center">
              {i > 0 && (
                <span
                  className={cx("mx-3 h-px w-12", isFilled ? "bg-accent" : "bg-border-strong")}
                  aria-hidden
                />
              )}
              <button
                type="button"
                className="flex items-center gap-2 rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-1 text-fg-muted transition-[background-color] duration-[0.12s] ease-[ease] not-disabled:cursor-pointer not-disabled:hover:bg-hover disabled:cursor-default"
                disabled={i > reached}
                aria-current={isCurrent ? "step" : undefined}
                onClick={() => onSelect(i)}
              >
                <span
                  className={cx(
                    "inline-flex h-6 w-6 flex-none items-center justify-center rounded-full text-[length:var(--fs-sm)] font-semibold",
                    isFilled ? "bg-accent text-on-accent" : "bg-selected-muted text-fg-subtle",
                  )}
                >
                  {i + 1}
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
