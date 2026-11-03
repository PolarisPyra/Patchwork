import { useEffect, useState } from "react";
import { ArrowRightIcon, MoonIcon, SunIcon } from "@phosphor-icons/react";
import { api, importSourcesFor, type AgentKind, type ImportSource, type Settings } from "~/api";
import type { UpdateSettings } from "~/api";
import AgentStep from "~/features/onboarding/components/AgentStep";
import DevicesStep from "~/features/onboarding/components/DevicesStep";
import ImportsStep, { useImportScans } from "~/features/onboarding/components/ImportsStep";
import ProjectsStep from "~/features/onboarding/components/ProjectsStep";
import Stepper from "~/features/onboarding/components/Stepper";
import { AGENT_KINDS, isReady, useAgentProbes } from "~/features/onboarding/agentProbes";
import { errorText } from "~/lib/util";

export interface OnboardingProps {
  settings: Settings;
  updateSettings: UpdateSettings;
  resolvedTheme: "light" | "dark";
  toggleTheme: () => void;
}

const STEPS = [
  {
    label: "Agent",
    title: "Connect your coding agents",
    subtitle:
      "Connect the agents already installed on this computer. You can pick which one runs each task.",
    helper: "Connect at least one agent to start your first task.",
  },
  {
    label: "Imports",
    title: "Import your conversations",
    subtitle:
      "Bring in conversations you've had with these agents outside Patchwork. Follow-ups continue the same session.",
    helper: "Importing is optional. You can import later from Settings.",
  },
  {
    label: "Projects",
    title: "Set up your projects",
    subtitle: "Choose where Patchwork should work. You can add and manage projects later.",
    helper: "Projects are optional. You can add more later.",
  },
  {
    label: "Devices",
    title: "Configure your devices",
    subtitle: "Choose where tasks run and how Patchwork connects to your development environments.",
    helper: "More devices can be added later.",
  },
];

const IMPORTS_STEP = 1;
const PROJECTS_STEP = 2;
const LAST_STEP = STEPS.length - 1;

export default function Onboarding({
  settings,
  updateSettings,
  resolvedTheme,
  toggleTheme,
}: OnboardingProps) {
  const [step, setStep] = useState(0);
  const [reached, setReached] = useState(0);
  const [saving, setSaving] = useState(false);
  // Connected agents, kept in AGENT_KINDS order so the first is the default.
  const [agents, setAgents] = useState<AgentKind[] | null>(null);
  const { probes, check, checking } = useAgentProbes();
  const importSources = importSourcesFor(agents ?? []);
  const { scans, scanning } = useImportScans(importSources, step === IMPORTS_STEP);
  // Providers to import from; defaults to every one with something new.
  const [importKinds, setImportKinds] = useState<ImportSource[] | null>(null);
  const [importError, setImportError] = useState<string | null>(null);

  useEffect(() => {
    if (step !== IMPORTS_STEP || scanning || importKinds !== null) return;
    setImportKinds(importSourcesFor(agents ?? []).filter((k) => (scans[k]?.new ?? 0) > 0));
  }, [step, scanning, scans, agents, importKinds]);

  const toggleImport = (kind: ImportSource) =>
    setImportKinds((current) => {
      const set = current ?? [];
      return set.includes(kind) ? set.filter((k) => k !== kind) : [...set, kind];
    });

  // Only ready agents can be selected. Start from the saved agents (or every
  // ready agent on first run), then keep the user's choices while still ready.
  useEffect(() => {
    if (checking) return;
    const ready: AgentKind[] = AGENT_KINDS.filter((k) => isReady(probes[k]));
    setAgents((current) => {
      const saved = settings.agents.filter((k) => ready.includes(k));
      const base = current ?? (saved.length > 0 ? saved : ready);
      return ready.filter((k) => base.includes(k));
    });
  }, [checking, probes, settings.agents]);

  const toggleAgent = (kind: AgentKind) =>
    setAgents((current) => {
      const set = current ?? [];
      const next = set.includes(kind) ? set.filter((k) => k !== kind) : [...set, kind];
      return AGENT_KINDS.filter((k) => next.includes(k));
    });

  const canContinue =
    !saving && (step !== 0 || (agents?.length ?? 0) > 0) && (step !== IMPORTS_STEP || !scanning);
  const isLast = step === LAST_STEP;
  const current = STEPS[step];

  const goTo = (index: number) => {
    setStep(index);
    setReached((r) => Math.max(r, index));
  };

  const complete = () => updateSettings((s) => ({ ...s, onboarded: true }));

  /** Imports the chosen providers' conversations; false if it failed. */
  const importSelected = async () => {
    const kinds = (importKinds ?? []).filter((k) => (scans[k]?.new ?? 0) > 0);
    if (kinds.length === 0) return true;
    setImportError(null);
    try {
      await api.importThreads(kinds);
      // Importing adds projects on the backend; pick them up before saving settings again.
      await updateSettings(await api.getSettings());
      setImportKinds(null);
      return true;
    } catch (e) {
      setImportError(errorText(e));
      return false;
    }
  };

  const next = async () => {
    if (!canContinue) return;
    setSaving(true);
    try {
      if (isLast) {
        await complete();
      } else {
        if (step === 0 && agents) await updateSettings((s) => ({ ...s, agents }));
        if (step === IMPORTS_STEP && !(await importSelected())) return;
        goTo(step + 1);
      }
    } finally {
      setSaving(false);
    }
  };

  // Enter continues from anywhere that is not a text field or button, so it
  // never hijacks the project name input or a focused control.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Enter" || !canContinue) return;
      if (!(e.target instanceof HTMLElement)) return;
      if (e.target.closest("input, textarea, select, button, [contenteditable='true']")) return;
      e.preventDefault();
      void next();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const renderStep = () => {
    switch (step) {
      case 0:
        return (
          <AgentStep
            probes={probes}
            checking={checking}
            selected={agents ?? []}
            onToggle={toggleAgent}
            onRecheck={check}
          />
        );
      case IMPORTS_STEP:
        return (
          <ImportsStep
            sources={importSources}
            scans={scans}
            selected={importKinds ?? []}
            onToggle={toggleImport}
            error={importError}
          />
        );
      case 2:
        return <ProjectsStep settings={settings} updateSettings={updateSettings} />;
      default:
        return <DevicesStep />;
    }
  };

  return (
    <div className="flex h-full flex-col bg-app text-fg">
      <header className="flex-none grid grid-cols-[1fr_auto_1fr] items-center h-[88px] px-6 border-b border-border bg-app">
        <div className="flex items-center gap-3">
          {/* Patchwork's logo, as in site/favicon.svg. */}
          <svg className="w-9 h-9 flex-none" viewBox="0 0 512 512" fill="none" aria-hidden="true">
            <g strokeWidth="26" strokeLinejoin="round">
              <polygon points="256,256 63.7,145 256,34 448.3,145" fill="#FF7EB3" stroke="#FF7EB3" />
              <polygon
                points="256,256 448.3,145 448.3,367 256,478"
                fill="#9B5DE5"
                stroke="#9B5DE5"
              />
              <polygon points="256,256 256,478 63.7,367 63.7,145" fill="#5BC0EB" stroke="#5BC0EB" />
            </g>
            <g
              stroke="#FFFFFF"
              strokeOpacity="0.75"
              strokeWidth="8"
              strokeDasharray="18 12"
              strokeLinecap="round"
            >
              <path d="M256 256 L421.3 160.5" />
              <path d="M256 256 L256 446.9" />
              <path d="M256 256 L90.7 160.5" />
            </g>
          </svg>
          <span className="text-[length:var(--fs-xl)] font-semibold">Patchwork</span>
        </div>

        <Stepper
          labels={STEPS.map((s) => s.label)}
          current={step}
          reached={reached}
          onSelect={goTo}
        />

        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            className="btn btn-icon"
            onClick={toggleTheme}
            title={
              resolvedTheme === "dark"
                ? "Dark theme (switch to light)"
                : "Light theme (switch to dark)"
            }
            aria-label="Dark theme"
            aria-pressed={resolvedTheme === "dark"}
          >
            {resolvedTheme === "dark" ? <MoonIcon size={16} /> : <SunIcon size={16} />}
          </button>
          <button
            type="button"
            className="btn btn-ghost text-fg-muted"
            onClick={() => void complete()}
          >
            Skip setup
          </button>
        </div>
      </header>

      <main className="flex-1 min-h-0 overflow-y-auto [scrollbar-gutter:stable_both-edges] py-10">
        <div className="w-full max-w-[820px] mx-auto px-6">
          <h1 className="m-0 text-[length:var(--fs-xxl)] font-semibold leading-[1.25]">
            {current.title}
          </h1>
          <p className="mt-2 mb-0 text-[length:var(--fs-lg)] text-fg-muted">{current.subtitle}</p>
          {/* The projects step is two buttons and a list, which need no box around them. */}
          <section
            className={
              step === PROJECTS_STEP
                ? "mt-6 flex flex-col gap-4"
                : "card mt-6 flex min-h-[520px] flex-col gap-4 bg-editor p-6"
            }
          >
            {renderStep()}
          </section>
        </div>
      </main>

      <footer className="flex-none h-[84px] border-t border-border bg-app">
        <div className="w-full max-w-[820px] mx-auto px-6 h-full flex items-center justify-between gap-4">
          <span className="text-[length:var(--fs-sm)] text-fg-muted">{current.helper}</span>
          <div className="flex gap-2">
            {step > 0 && (
              <button type="button" className="btn btn-lg" onClick={() => goTo(step - 1)}>
                Back
              </button>
            )}
            {/* Moves on without importing, also while the scans are still running, or without
                adding a project. */}
            {(step === IMPORTS_STEP || step === PROJECTS_STEP) && (
              <button
                type="button"
                className="btn btn-lg"
                disabled={saving}
                onClick={() => goTo(step + 1)}
              >
                Skip
              </button>
            )}
            <button
              type="button"
              className="btn btn-primary btn-lg w-[190px]"
              disabled={!canContinue}
              onClick={() => void next()}
            >
              {isLast ? (
                "Finish setup"
              ) : (
                <>
                  Continue <ArrowRightIcon size={16} />
                </>
              )}
            </button>
          </div>
        </div>
      </footer>
    </div>
  );
}
