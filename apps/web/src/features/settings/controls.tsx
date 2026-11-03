import { useEffect, useState, type ReactNode } from "react";
import { MinusIcon, PlusIcon } from "@phosphor-icons/react";

/**
 * The controls every settings pane draws: a row, a switch, a row of choices, a text field and the
 * badges around them. They lived inside the dialog until panes of their own needed them, which
 * meant five modules had to import a 2000-line dialog to get a switch.
 */

/** The text input every pane shares: one height, border and focus colour. */
export const INPUT =
  "h-7 w-full min-w-0 rounded-[var(--radius-sm)] border border-border-strong bg-editor px-2 text-[length:var(--fs-sm)] text-fg outline-none focus:border-accent";

/** A text area in the same style, for text longer than a line. */
export const TEXTAREA = `${INPUT} h-auto min-h-[70px] w-full resize-y py-1.5`;

/** A filter chip, filled when it is the one showing. */
export function chip(on: boolean): string {
  return `flex h-7 flex-none cursor-pointer items-center gap-1.5 whitespace-nowrap rounded-full border px-2.5 text-[length:var(--fs-sm)] ${on ? "border-accent bg-accent-soft text-fg" : "border-border bg-transparent text-fg hover:bg-hover"}`;
}

/** A letter on a colour picked from the name, so each row is easy to spot. */
export function Badge({ name, size }: { name: string; size: number }) {
  let hash = 0;
  for (const c of name) hash = (hash * 31 + c.charCodeAt(0)) | 0;
  const hue = Math.abs(hash) % 360;
  return (
    <span
      className="grid flex-none place-items-center rounded-[var(--radius-md)] font-semibold uppercase"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.45,
        background: `hsl(${hue} 70% 55% / 0.18)`,
        color: `hsl(${hue} 75% 65%)`,
      }}
    >
      {name.replace(/[^a-z0-9]/gi, "").charAt(0) || "?"}
    </span>
  );
}

/** One setting: its name and what it does, with the control on the right. */
export function TokenRow({
  title,
  desc,
  children,
}: {
  title: string;
  desc?: string;
  children: ReactNode;
}) {
  return (
    <div
      role="group"
      aria-label={title}
      className="flex items-center gap-3 border-b border-border py-2.5"
    >
      <div className="min-w-0 flex-1">
        <div className="font-semibold">{title}</div>
        {desc && <div className="text-[length:var(--fs-sm)] text-fg-muted">{desc}</div>}
      </div>
      {children}
    </div>
  );
}

/** A heading over a run of rows, for a pane that holds more than one kind of setting. */
export function SectionHeading({ children }: { children: ReactNode }) {
  return (
    <h3 className="m-0 mt-4 mb-0.5 text-[length:var(--fs-sm)] font-medium text-fg-subtle first:mt-0">
      {children}
    </h3>
  );
}

export function TokenSwitch({
  label,
  on,
  onChange,
}: {
  label: string;
  on: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      className="toggle"
      role="switch"
      aria-label={label}
      aria-checked={on}
      onClick={() => onChange(!on)}
    />
  );
}

/** Two to five choices side by side, as the theme picker. */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex overflow-hidden rounded-[var(--radius-sm)] border border-border-strong"
    >
      {options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={value === o.value}
          className={`grid h-6 w-auto cursor-pointer place-items-center border-none px-2.5 py-0 text-[length:var(--fs-sm)] whitespace-nowrap tabular-nums [&+&]:border-l [&+&]:border-border-strong ${value === o.value ? "bg-selected text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A text field that saves when it loses focus or on Enter. */
export function TextField({
  label,
  value,
  placeholder,
  width = "w-[220px]",
  onSave,
}: {
  label: string;
  value: string;
  placeholder?: string;
  width?: string;
  onSave: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <input
      className={`input ${width} text-[length:var(--fs-sm)]`}
      aria-label={label}
      value={draft}
      placeholder={placeholder}
      spellCheck={false}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => draft !== value && onSave(draft)}
      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
    />
  );
}

/** A number field that saves a valid number when it loses focus or on Enter. */
export function NumberField({
  label,
  value,
  step = 1,
  min,
  max,
  onSave,
}: {
  label: string;
  value: number;
  step?: number;
  min?: number;
  max?: number;
  onSave: (value: number) => void;
}) {
  return (
    <TextField
      label={label}
      value={String(value)}
      width="w-[80px]"
      onSave={(text) => {
        const n = Number(text);
        if (text.trim() === "" || !Number.isFinite(n)) return;
        const clamped = Math.min(max ?? n, Math.max(min ?? n, n));
        onSave(step >= 1 ? Math.round(clamped) : clamped);
      }}
    />
  );
}

/** The minutes between update checks that the stepper offers. */
const CHECK_MINUTES = [10, 20, 30, 40, 50, 60];

/** Steps through `CHECK_MINUTES`. */
export function MinutesStepper({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  const at = Math.max(0, CHECK_MINUTES.indexOf(value));
  const step =
    "grid h-6 w-6 cursor-pointer place-items-center border-none bg-transparent p-0 text-fg-muted hover:bg-hover hover:text-fg disabled:cursor-default disabled:opacity-40 disabled:hover:bg-transparent";
  return (
    <div
      role="group"
      aria-label={label}
      className="inline-flex items-center overflow-hidden rounded-[var(--radius-sm)] border border-border-strong"
    >
      <button
        className={step}
        aria-label="Less often"
        disabled={at === 0}
        onClick={() => onChange(CHECK_MINUTES[at - 1])}
      >
        <MinusIcon size={12} />
      </button>
      <span className="w-14 border-x border-border-strong text-center text-[length:var(--fs-sm)] tabular-nums">
        {CHECK_MINUTES[at] === 60 ? "1 hr" : `${CHECK_MINUTES[at]} min`}
      </span>
      <button
        className={step}
        aria-label="More often"
        disabled={at === CHECK_MINUTES.length - 1}
        onClick={() => onChange(CHECK_MINUTES[at + 1])}
      >
        <PlusIcon size={12} />
      </button>
    </div>
  );
}
