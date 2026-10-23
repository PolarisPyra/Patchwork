import { open } from "@tauri-apps/plugin-dialog";
import { api, type Project } from "~/api";
import type { UpdateSettings } from "~/app/App";
import { getCollapsed, setCollapsed } from "~/components/sidebar/collapsedState";

/** Re-reads the project list from the backend; never overwrites it with stale local copies. */
export async function syncProjects(updateSettings: UpdateSettings): Promise<void> {
  const fresh = await api.getSettings();
  await updateSettings((s) => ({
    ...s,
    projects: fresh.projects,
    activeProjectId: fresh.activeProjectId,
  }));
}

/** Registers folders as projects; the last becomes the active one. New projects start collapsed like
 * the rest of the list, and a folder that is already a project keeps its state. A file fails with
 * the backend's error. Resolves to the ids of the folders that were already projects. */
export async function addFolders(
  paths: string[],
  known: Project[],
  updateSettings: UpdateSettings,
): Promise<string[]> {
  const existing: string[] = [];
  try {
    for (const path of paths) {
      const added = await api.addProject(path);
      if (known.some((p) => p.id === added.id)) existing.push(added.id);
      else setCollapsed(new Set([...getCollapsed(), added.id]));
    }
  } finally {
    await syncProjects(updateSettings);
  }
  return existing;
}

/** Folder picker, then registers the folder as the active project. No-op if cancelled. Resolves
 * like `addFolders`. */
export async function openFolderAsProject(
  known: Project[],
  updateSettings: UpdateSettings,
): Promise<string[]> {
  const path = await open({ directory: true, multiple: false });
  if (typeof path !== "string") return [];
  return addFolders([path], known, updateSettings);
}

/** Folder picker, then points the project at the folder, such as after it was moved. No-op if
 * cancelled. */
export async function relocateProject(id: string, updateSettings: UpdateSettings): Promise<void> {
  const path = await open({ directory: true, multiple: false });
  if (typeof path !== "string") return;
  await api.relocateProject(id, path);
  await syncProjects(updateSettings);
}
