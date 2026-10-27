import { useCallback, useEffect, useState } from "react";
import { api, onProjectChanged, type FileChange } from "~/api";
import { errorText } from "~/lib/util";

interface GitState {
  projectId: string;
  branch: string | null;
  changes: FileChange[];
}

// One shared empty list, so the chat's memoized rows see the same `changes` on every render.
const NO_CHANGES: FileChange[] = [];

/** The open project's branch and uncommitted changes, kept current as anything changes its
 * files, and a way to read them again. */
export function useProjectGit(projectId: string | null, onError: (message: string) => void) {
  const [git, setGit] = useState<GitState | null>(null);
  const current = git && git.projectId === projectId ? git : null;

  const refreshGit = useCallback(() => {
    if (!projectId) return Promise.resolve();
    return Promise.all([
      api.gitBranch(projectId).catch(() => null),
      api.gitChanges(projectId).catch(() => []),
    ]).then(([branch, changes]) => setGit({ projectId, branch, changes }));
  }, [projectId]);

  useEffect(() => {
    void refreshGit();
  }, [refreshGit]);

  // The backend watches the active project and sends git's state after each settled burst of
  // changes, whoever made them: an agent, an editor, or git in a terminal.
  useEffect(() => {
    if (!projectId) return;
    api.watchProject(projectId).catch((e: unknown) => onError(errorText(e)));
    const unlisten = onProjectChanged((e) => {
      if (e.projectId === projectId && e.changes) {
        setGit({ projectId, branch: e.branch, changes: e.changes });
      }
    });
    return () => void unlisten.then((stop) => stop());
  }, [projectId, onError]);

  return {
    changes: current?.changes ?? NO_CHANGES,
    branch: current?.branch ?? null,
    refreshGit,
  };
}
