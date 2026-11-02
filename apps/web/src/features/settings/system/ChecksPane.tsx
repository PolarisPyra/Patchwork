import type { CheckSettings } from "~/api";
import { TokenRow, TokenSwitch } from "~/features/settings/controls";
import type { PaneProps } from "~/features/settings/types";

/**
 * Checks: running a project's own check command after every task, so a reply says whether the
 * project's tests still pass. Off until turned on here, since a test suite after every task is
 * the person's call. A project can answer for itself, for a suite too slow to run every time.
 */
export default function ChecksPane({ settings, updateSettings }: PaneProps) {
  const { afterEveryTask, projects } = settings.checks;
  const update = (patch: Partial<CheckSettings>) =>
    void updateSettings((s) => ({ ...s, checks: { ...s.checks, ...patch } }));

  /** Back to whatever the default says, which is what a new project does. */
  function followDefault(id: string) {
    const rest = { ...projects };
    delete rest[id];
    update({ projects: rest });
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
          onChange={(on) => update({ afterEveryTask: on })}
        />
      </TokenRow>

      {settings.projects.length > 0 && (
        <>
          <div className="pt-4 pb-1 text-[length:var(--fs-xs)] font-semibold text-fg-subtle">
            Projects
          </div>
          {settings.projects.map((project) => {
            const own = projects[project.id];
            return (
              <div
                key={project.id}
                role="group"
                aria-label={project.name}
                className="flex items-center gap-3 border-b border-border py-2.5"
              >
                <div className="min-w-0 flex-1">
                  <div className="font-semibold">{project.name}</div>
                  <div className="text-[length:var(--fs-sm)] text-fg-muted">
                    {own === undefined ? (
                      "Follows the default"
                    ) : (
                      <>
                        Set for this project
                        <button
                          className="ml-2 cursor-pointer border-none bg-transparent p-0 text-[length:var(--fs-sm)] text-accent hover:underline"
                          onClick={() => followDefault(project.id)}
                        >
                          Use the default
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <TokenSwitch
                  label={`Run checks after every task in ${project.name}`}
                  on={own ?? afterEveryTask}
                  onChange={(on) => update({ projects: { ...projects, [project.id]: on } })}
                />
              </div>
            );
          })}
        </>
      )}
    </>
  );
}
