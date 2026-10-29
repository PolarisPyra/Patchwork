import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "~/api/invoke";
import type { Folder, Project, ProjectChanged } from "~/api/types";

export const projectsApi = {
  /** Registers an existing folder; also makes it the active project. */
  addProject: (path: string) => invoke<Project>("add_project", { path }),
  /** Creates `parent/name`, runs `git init`, registers it as the active project. */
  createProject: (parent: string, name: string) =>
    invoke<Project>("create_project", { parent, name }),
  renameProject: (id: string, name: string) => invoke<void>("rename_project", { id, name }),
  relocateProject: (id: string, path: string) => invoke<void>("relocate_project", { id, path }),
  isFolder: (path: string) => invoke<boolean>("is_folder", { path }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),
  listFolders: () => invoke<Folder[]>("list_folders"),
  createFolder: (projectId: string, name: string) =>
    invoke<Folder>("create_folder", { projectId, name }),
  renameFolder: (id: string, name: string) => invoke<void>("rename_folder", { id, name }),
  /** Its tasks move to the project's top level. */
  removeFolder: (id: string) => invoke<void>("remove_folder", { id }),
  /** Ids of the projects and folders collapsed in the sidebar. */
  getCollapsed: () => invoke<string[]>("get_collapsed"),
  setCollapsed: (ids: string[]) => invoke<void>("set_collapsed", { ids }),
  /** Files a mention can refer to, relative to the project root. */
  listFiles: (projectId: string) => invoke<string[]>("list_files", { projectId }),
  /** Watches the project's files in place of any other; `onProjectChanged` reports changes. */
  watchProject: (projectId: string) => invoke<void>("watch_project", { projectId }),
};

export function onProjectChanged(cb: (e: ProjectChanged) => void): Promise<UnlistenFn> {
  return listen<ProjectChanged>("project-changed", (e) => cb(e.payload));
}
