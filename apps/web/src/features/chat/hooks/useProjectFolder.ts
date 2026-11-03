import { useEffect, useState } from "react";
import { api } from "~/api";

// A fallback that keeps one identity, so the memoized composer sees no change.
const NO_FILES: string[] = [];

/**
 * What the chat knows of its project's folder: the files an @ mention can offer, and whether the
 * folder is missing, after it was moved or deleted, so the agent cannot run until it is pointed
 * elsewhere. Each answer is kept with the folder it was read for, so a chat never shows what was
 * read for another.
 */
export function useProjectFolder(projectId: string | null, projectPath: string | null) {
  const [listed, setListed] = useState<{
    projectId: string;
    path: string | null;
    files: string[];
  } | null>(null);
  useEffect(() => {
    if (!projectId) return;
    let cancelled = false;
    const keep = (files: string[]) => {
      if (!cancelled) setListed({ projectId, path: projectPath, files });
    };
    api.listFiles(projectId).then(keep, () => keep([]));
    return () => {
      cancelled = true;
    };
  }, [projectId, projectPath]);
  const files =
    listed && listed.projectId === projectId && listed.path === projectPath
      ? listed.files
      : NO_FILES;

  const [checked, setChecked] = useState<{ path: string; found: boolean } | null>(null);
  useEffect(() => {
    if (!projectPath) return;
    let cancelled = false;
    api.isFolder(projectPath).then(
      (found) => {
        if (!cancelled) setChecked({ path: projectPath, found });
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  }, [projectPath]);
  const folderMissing = checked !== null && checked.path === projectPath && !checked.found;

  return { files, folderMissing };
}
