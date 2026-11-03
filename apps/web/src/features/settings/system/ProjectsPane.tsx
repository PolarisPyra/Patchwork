import { useState } from "react";
import { TrashIcon } from "@phosphor-icons/react";
import { api, type CheckSettings } from "~/api";
import { TokenRow, TokenSwitch } from "~/features/settings/controls";
import type { PaneProps } from "~/features/settings/types";
import { syncProjects } from "~/lib/projects";
import { errorText } from "~/lib/util";

/**
 * The projects Patchwork knows, each with its path, and the way back into setup. Checks run a
 * project's own check command after every task, so a reply says whether the project's tests still
 * pass. Off until turned on here, since a test suite after every task is the person's call. A
 * project can answer for itself, for a suite too slow to run every time.
 */
export default function ProjectsPane({ settings, updateSettings }: PaneProps) {
  const [error, setError] = useState<string | null>(null);
  const { afterEveryTask, projects: own } = settings.checks;
  const updateChecks = (patch: Partial<CheckSettings>) =>
    void updateSettings((s) => ({ ...s, checks: { ...s.checks, ...patch } }));

  /** Back to whatever the default says, which is what a new project does. */
  function followDefault(id: string) {
    const rest = { ...own };
    delete rest[id];
    updateChecks({ projects: rest });
  }

  async function remove(id: string) {
    setError(null);
    try {
      await api.removeProject(id);
      await syncProjects(updateSettings);
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <>
      <TokenRow
        title="Run checks after every task"
        desc="The default for projects that have not answered for themselves. Only a task that changed a file is checked."
      >
        <TokenSwitch
          label="Run checks after every task"
          on={afterEveryTask}
          onChange={(on) => updateChecks({ afterEveryTask: on })}
        />
      </TokenRow>
      {settings.projects.length === 0 && (
        <div className="px-3 py-2 text-[length:var(--fs-sm)] text-fg-subtle">No projects yet</div>
      )}
      {settings.projects.map((p) => (
        <div
          key={p.id}
          role="group"
          aria-label={p.name}
          className="flex items-center gap-2 border-b border-border py-2"
        >
          <div className="min-w-0 flex-1">
            <div className="font-semibold">{p.name}</div>
            <div className="overflow-hidden text-ellipsis whitespace-nowrap font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] text-fg-muted">
              {p.path}
            </div>
          </div>
          {own[p.id] !== undefined && (
            <button
              className="cursor-pointer border-none bg-transparent p-0 text-[length:var(--fs-sm)] whitespace-nowrap text-accent hover:underline"
              aria-label={`Use the default checks setting in ${p.name}`}
              onClick={() => followDefault(p.id)}
            >
              Use the default
            </button>
          )}
          <span className="text-[length:var(--fs-sm)] text-fg-muted">Checks</span>
          <TokenSwitch
            label={`Run checks after every task in ${p.name}`}
            on={own[p.id] ?? afterEveryTask}
            onChange={(on) => updateChecks({ projects: { ...own, [p.id]: on } })}
          />
          <button
            className="btn-icon"
            title="Remove project"
            aria-label={`Remove ${p.name}`}
            onClick={() => void remove(p.id)}
          >
            <TrashIcon size={14} />
          </button>
        </div>
      ))}
      {error && (
        <div role="alert" className="mt-2 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
      <div className="mt-4">
        <button
          className="btn"
          onClick={() => void updateSettings((s) => ({ ...s, onboarded: false }))}
        >
          Run setup again
        </button>
      </div>
    </>
  );
}
