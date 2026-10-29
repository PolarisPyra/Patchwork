import { useState } from "react";
import { CheckIcon } from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Settings, type UpdateSettings } from "~/api";
import { syncProjects } from "~/lib/projects";
import { errorText } from "~/lib/util";
import { cx } from "~/features/onboarding/lib/classNames";

interface ProjectsStepProps {
  settings: Settings;
  updateSettings: UpdateSettings;
}

type Action = "open" | "create";

export default function ProjectsStep({ settings, updateSettings }: ProjectsStepProps) {
  const [action, setAction] = useState<Action | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [parent, setParent] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (kind: Action, task: () => Promise<unknown>): Promise<boolean> => {
    setAction(kind);
    setBusy(true);
    setError(null);
    try {
      await task();
      await syncProjects(updateSettings);
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const pickFolder = async (): Promise<string | null> => {
    try {
      return await open({ directory: true });
    } catch (e) {
      setError(errorText(e));
      return null;
    }
  };

  const browseExisting = async () => {
    const path = await pickFolder();
    if (path) await run("open", () => api.addProject(path));
  };

  const chooseParent = async () => {
    const path = await pickFolder();
    if (path) setParent(path);
  };

  const createProject = async () => {
    const trimmed = name.trim();
    if (!parent || !trimmed) return;
    if (await run("create", () => api.createProject(parent, trimmed))) setName("");
  };

  const setActive = (id: string) => void updateSettings((s) => ({ ...s, activeProjectId: id }));

  return (
    <>
      <div>
        <div className="text-[length:var(--fs-md)] font-semibold">Project workspace</div>
        <div className="text-[length:var(--fs-sm)] text-fg-muted">
          Start from an existing folder or create a new workspace.
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <div
          className={cx(
            "flex w-full items-center gap-4 rounded-[var(--radius)] border p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease]",
            action === "open" ? "border-accent bg-accent-soft" : "border-border bg-transparent",
          )}
        >
          <span className="w-9 flex-none text-[length:var(--fs-xl)] font-semibold tabular-nums text-accent-text">
            01
          </span>
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="text-[length:var(--fs-lg)] font-semibold">Open a local project</span>
            <span className="text-[length:var(--fs-sm)] text-fg-muted">
              Select a folder from your computer to start working.
            </span>
          </span>
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() => void browseExisting()}
          >
            Browse folders
          </button>
        </div>

        <div
          className={cx(
            "flex w-full items-center gap-4 rounded-[var(--radius)] border p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease]",
            action === "create" ? "border-accent bg-accent-soft" : "border-border bg-transparent",
          )}
        >
          <span className="w-9 flex-none text-[length:var(--fs-xl)] font-semibold tabular-nums text-accent-text">
            02
          </span>
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="text-[length:var(--fs-lg)] font-semibold">Create a new project</span>
            <span className="text-[length:var(--fs-sm)] text-fg-muted">
              Choose a location and initialize an empty workspace.
            </span>
          </span>
          <button
            type="button"
            className="btn"
            disabled={busy}
            onClick={() => {
              setCreateOpen(true);
              setAction("create");
            }}
          >
            Create project
          </button>
        </div>

        {createOpen && (
          <div className="flex flex-col gap-3 rounded-[var(--radius)] border border-border p-4">
            <div className="flex items-center gap-3">
              <span className="w-[72px] flex-none text-[length:var(--fs-sm)] text-fg-muted">
                Location
              </span>
              <span
                className={cx(
                  "min-w-0 flex-1 overflow-hidden whitespace-nowrap text-ellipsis text-[length:var(--fs-sm)]",
                  parent
                    ? "font-[family-name:var(--font-mono)] text-fg"
                    : "font-[family-name:var(--font-ui)] text-fg-muted",
                )}
              >
                {parent ?? "No location chosen"}
              </span>
              <button
                type="button"
                className="btn"
                disabled={busy}
                onClick={() => void chooseParent()}
              >
                Choose…
              </button>
            </div>
            <div className="flex items-center gap-3">
              <label
                htmlFor="ob-project-name"
                className="w-[72px] flex-none text-[length:var(--fs-sm)] text-fg-muted"
              >
                Name
              </label>
              <input
                id="ob-project-name"
                className="input min-w-0 flex-1"
                value={name}
                placeholder="my-project"
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void createProject();
                }}
              />
            </div>
            <div className="flex justify-end">
              <button
                type="button"
                className="btn btn-primary"
                disabled={busy || !parent || !name.trim()}
                onClick={() => void createProject()}
              >
                Create
              </button>
            </div>
          </div>
        )}
      </div>

      {error && (
        <p className="m-0 text-[length:var(--fs-sm)] text-danger" role="alert">
          {error}
        </p>
      )}

      <div>
        <div className="mb-2 font-semibold">Recent projects</div>
        {settings.projects.length === 0 ? (
          <p className="m-0 text-[length:var(--fs-sm)] text-fg-muted">No projects yet.</p>
        ) : (
          <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
            {settings.projects.map((p) => {
              const active = p.id === settings.activeProjectId;
              return (
                <li key={p.id}>
                  <button
                    type="button"
                    className={cx(
                      "flex w-full cursor-pointer items-center gap-3 rounded-[var(--radius-sm)] border-none px-3 py-2 text-left transition-[background-color] duration-[0.12s] ease-[ease]",
                      active ? "bg-accent-soft" : "bg-transparent hover:bg-hover",
                    )}
                    aria-pressed={active}
                    onClick={() => setActive(p.id)}
                  >
                    <span className="inline-flex w-3.5 flex-none text-accent-text">
                      {active && <CheckIcon size={14} weight="bold" />}
                    </span>
                    <span className="flex-none font-medium text-fg">{p.name}</span>
                    <span className="min-w-0 flex-1 overflow-hidden whitespace-nowrap text-right text-ellipsis font-[family-name:var(--font-mono)] text-[length:var(--fs-sm)] text-fg-muted">
                      {p.path}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </>
  );
}
