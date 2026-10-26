import { useEffect, useState } from "react";
import { api, type FileChange } from "~/api";

const NONE: ReadonlySet<string> = new Set();

/**
 * Which of `paths` no longer exist in the project, such as a deleted file git never tracked.
 * Paths with uncommitted changes are left out, since their status says whether they were deleted.
 * Checked again whenever `changes` updates, which follows every settled burst of file changes.
 */
export function useMissingFiles(
  projectId: string | null,
  paths: string[],
  changes: FileChange[],
): ReadonlySet<string> {
  const [missing, setMissing] = useState(NONE);
  const unchanged = paths.filter((p) => !changes.some((c) => c.path === p)).join("\0");
  useEffect(() => {
    if (!projectId || !unchanged) {
      setMissing(NONE);
      return;
    }
    let live = true;
    api.missingFiles(projectId, unchanged.split("\0")).then(
      (found) => {
        if (live) setMissing(new Set(found));
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [projectId, unchanged, changes]);
  return missing;
}
