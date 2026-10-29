import type { Integrations, Settings } from "~/api";
import type { UpdateSettings } from "~/api";
import { cx } from "~/features/onboarding/lib/classNames";

interface PluginsStepProps {
  settings: Settings;
  updateSettings: UpdateSettings;
}

const PLUGINS: { key: keyof Integrations; letter: string; name: string; description: string }[] = [
  {
    key: "github",
    letter: "G",
    name: "GitHub",
    description: "Repository access and pull requests",
  },
  {
    key: "terminal",
    letter: "T",
    name: "Terminal",
    description: "Let the agent run shell commands without asking",
  },
  { key: "figma", letter: "F", name: "Figma", description: "Access design context and assets" },
  {
    key: "browser",
    letter: "B",
    name: "Browser",
    description: "Inspect and test local applications",
  },
];

export default function PluginsStep({ settings, updateSettings }: PluginsStepProps) {
  const toggle = (key: keyof Integrations) =>
    void updateSettings((s) => ({
      ...s,
      integrations: { ...s.integrations, [key]: !s.integrations[key] },
    }));

  return (
    <>
      <div className="text-[length:var(--fs-md)] font-semibold">Optional integrations</div>
      <div className="flex flex-col gap-2">
        {PLUGINS.map((p) => {
          const on = settings.integrations[p.key];
          // The whole row toggles. The switch inside has no handler of its own, so
          // its click (or keyboard activation) bubbles up to the row exactly once.
          return (
            <div
              key={p.key}
              className={cx(
                "flex w-full items-center gap-4 rounded-[var(--radius)] border p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease] not-disabled:cursor-pointer not-disabled:hover:bg-hover",
                on ? "border-accent bg-accent-soft" : "border-border bg-transparent",
              )}
              onClick={() => toggle(p.key)}
            >
              <span className="inline-flex h-10 w-10 flex-none items-center justify-center rounded-[var(--radius)] bg-selected-muted text-[length:var(--fs-lg)] font-semibold text-accent-text">
                {p.letter}
              </span>
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="text-[length:var(--fs-lg)] font-semibold">{p.name}</span>
                <span className="text-[length:var(--fs-sm)] text-fg-muted">{p.description}</span>
              </span>
              <button
                type="button"
                role="switch"
                aria-checked={on}
                aria-label={`${p.name} integration`}
                className="toggle"
              />
            </div>
          );
        })}
      </div>
      <p className="mt-auto mb-0 text-[length:var(--fs-sm)] text-fg-muted">
        You can change integration permissions in Settings.
      </p>
    </>
  );
}
