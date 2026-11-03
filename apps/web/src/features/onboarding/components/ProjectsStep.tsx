import { useState } from "react";
import { CheckIcon, FolderOpenIcon, PlusIcon } from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Settings, type UpdateSettings } from "~/api";
import { syncProjects } from "~/lib/projects";
import { errorText } from "~/lib/util";
import { cx } from "~/features/onboarding/classNames";

// The two ways in: large plain tiles, each with an icon.
const ACTION =
  "flex cursor-pointer flex-col items-center justify-center gap-3 rounded-[var(--radius-lg)] border-none px-6 py-7 text-[length:var(--fs-lg)] font-semibold text-fg not-disabled:hover:bg-hover disabled:cursor-default disabled:opacity-60";
const ICON =
  "inline-flex h-12 w-12 items-center justify-center rounded-[var(--radius)] bg-selected-muted text-fg-muted";

interface ProjectsStepProps {
  settings: Settings;
  updateSettings: UpdateSettings;
}

export default function ProjectsStep({ settings, updateSettings }: ProjectsStepProps) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (task: () => Promise<unknown>): Promise<boolean> => {
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

  const openExisting = async () => {
    const path = await pickFolder();
    if (path) await run(() => api.addProject(path));
  };

  // A new project is an empty folder with a git repository, made inside the folder picked here.
  const createIn = async () => {
    const trimmed = name.trim();
    if (!trimmed) return;
    const parent = await pickFolder();
    if (parent && (await run(() => api.createProject(parent, trimmed)))) {
      setName("");
      setCreating(false);
    }
  };

  const setActive = (id: string) => void updateSettings((s) => ({ ...s, activeProjectId: id }));

  return (
    <>
      <div className="grid grid-cols-2 gap-3">
        <button
          type="button"
          className={cx(ACTION, "bg-panel")}
          disabled={busy}
          onClick={() => void openExisting()}
        >
          <span className={ICON}>
            <FolderOpenIcon size={22} />
          </span>
          Open a folder
        </button>
        <button
          type="button"
          className={cx(ACTION, creating ? "choice-selected" : "bg-panel")}
          disabled={busy}
          aria-expanded={creating}
          onClick={() => setCreating((c) => !c)}
        >
          <span className={ICON}>
            <PlusIcon size={22} />
          </span>
          New project
        </button>
      </div>

      {creating && (
        <div className="flex gap-2 rounded-[var(--radius-lg)] bg-panel p-3">
          <input
            className="input min-w-0 flex-1"
            aria-label="Project name"
            value={name}
            placeholder="my-project"
            autoFocus
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void createIn();
              if (e.key === "Escape") setCreating(false);
            }}
          />
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || !name.trim()}
            onClick={() => void createIn()}
          >
            Create in…
          </button>
        </div>
      )}

      {error && (
        <p className="m-0 text-[length:var(--fs-sm)] text-danger" role="alert">
          {error}
        </p>
      )}

      {settings.projects.length > 0 && (
        <ul aria-label="Projects" className="m-0 flex list-none flex-col gap-0.5 p-0">
          {settings.projects.map((p) => {
            const active = p.id === settings.activeProjectId;
            return (
              <li key={p.id}>
                <button
                  type="button"
                  className={cx(
                    "flex w-full cursor-pointer items-center gap-3 rounded-[var(--radius-sm)] border border-transparent px-3 py-2 text-left",
                    active ? "choice-selected" : "bg-transparent hover:bg-hover",
                  )}
                  aria-pressed={active}
                  onClick={() => setActive(p.id)}
                >
                  <span className="inline-flex w-3.5 flex-none text-fg">
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
    </>
  );
}
