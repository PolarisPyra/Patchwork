import { invoke } from "~/api/invoke";
import type { FileChange } from "~/api/types";

export const gitApi = {
  gitBranch: (projectId: string) => invoke<string | null>("git_branch", { projectId }),
  gitChanges: (projectId: string) => invoke<FileChange[]>("git_changes", { projectId }),
  /** Unified diff text for one file (empty string if unchanged). */
  gitFileDiff: (projectId: string, path: string) =>
    invoke<string>("git_file_diff", { projectId, path }),
  /** "Apply changes": stages the file (git add). */
  acceptFile: (projectId: string, path: string) => invoke<void>("accept_file", { projectId, path }),
  /** "Reject": restores the file to HEAD (deletes it if untracked). */
  revertFile: (projectId: string, path: string) => invoke<void>("revert_file", { projectId, path }),
};
