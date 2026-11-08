import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "~/api/invoke";
import type {
  DirListing,
  Folder,
  Machine,
  Project,
  ProjectChanged,
  SshHost,
  SshTest,
} from "~/api/types";

export const projectsApi = {
  /** Registers an existing folder; also makes it the active project. */
  addProject: (path: string) => invoke<Project>("add_project", { path }),
  /** Creates `parent/name`, runs `git init`, registers it as the active project. */
  createProject: (parent: string, name: string) =>
    invoke<Project>("create_project", { parent, name }),
  renameProject: (id: string, name: string) => invoke<void>("rename_project", { id, name }),
  relocateProject: (id: string, path: string) => invoke<void>("relocate_project", { id, path }),
  isFolder: (path: string) => invoke<boolean>("is_folder", { path }),
  /** The machines the Add Project dialog offers: this computer, then the SSH hosts in the config. */
  listMachines: () => invoke<Machine[]>("list_machines"),
  /** The sub-folders of `path`, or this computer's home folder when it is null. */
  browseDir: (path: string | null) => invoke<DirListing>("browse_dir", { path }),
  /** The hosts in `~/.ssh/config`, read only. */
  listSshHosts: () => invoke<SshHost[]>("list_ssh_hosts"),
  /** Logs in to a configured host with keys only, to show whether it answers. */
  testSshHost: (name: string) => invoke<SshTest>("test_ssh_host", { name }),
  /** Opens `~/.ssh/config` in the editor; fails when there is none. */
  openSshConfig: () => invoke<void>("open_ssh_config"),
  /** A worktree project also removes its worktree, refused while it has changes not committed. */
  removeProject: (id: string) => invoke<void>("remove_project", { id }),
  /** Makes a git worktree of the project on a new branch, registered as the active project. */
  createWorktree: (projectId: string) => invoke<Project>("create_worktree", { projectId }),
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
