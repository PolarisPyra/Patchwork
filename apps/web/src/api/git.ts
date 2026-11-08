import { invoke } from "~/api/invoke";
import type {
  FileChange,
  GitCommit,
  GitSyncState,
  HunkAction,
  PullRequest,
  TurnTrees,
} from "~/api/types";

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
  /** Stages, unstages, or discards one change of a file's diff, given as its text from its `@@` line
   * on: the reply's diff when `turn` is given. */
  applyHunk: (
    projectId: string,
    path: string,
    turn: TurnTrees | null,
    hunk: string,
    action: HunkAction,
  ) => invoke<void>("apply_hunk", { projectId, path, turn, hunk, action }),
  /** Which changes of a file's diff are staged, in its order. */
  stagedHunks: (projectId: string, path: string, turn: TurnTrees | null) =>
    invoke<boolean[]>("staged_hunks", { projectId, path, turn }),
  /** Where the branch stands against its remote, and its open pull request. */
  gitSyncState: (projectId: string) => invoke<GitSyncState>("git_sync_state", { projectId }),
  /** The branch's and its upstream's commits, newest first in graph order. */
  gitLog: (projectId: string, skip: number, limit: number) =>
    invoke<GitCommit[]>("git_log", { projectId, skip, limit }),
  /** Stages everything and commits it; a blank message is written by a model. Returns the short hash and the subject. */
  commitChanges: (projectId: string, threadId: string | null, message: string) =>
    invoke<[string, string]>("commit_changes", { projectId, threadId, message }),
  /** Stages everything and has a model write a commit message for it, to edit before committing. */
  writeCommitMessage: (projectId: string, threadId: string | null) =>
    invoke<string>("write_commit_message", { projectId, threadId }),
  /** Pushes the branch, setting its upstream on the first push. Returns where it went, such as "origin/main". */
  gitPush: (projectId: string) => invoke<string>("git_push", { projectId }),
  /** Opens a pull request into the default branch, or returns the open one. */
  openPullRequest: (projectId: string, threadId: string | null) =>
    invoke<PullRequest>("open_pull_request", { projectId, threadId }),
  /** A prompt with the open pull request's failed checks and comments, and the thread that made the
   * branch's newest commit, to send it to. */
  pullRequestFeedback: (projectId: string) =>
    invoke<[string | null, string]>("pull_request_feedback", { projectId }),
  /** A prompt for an agent to resolve the project's merge conflicts. */
  conflictPrompt: (projectId: string) => invoke<string>("conflict_prompt", { projectId }),
};
