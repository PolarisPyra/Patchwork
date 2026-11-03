import { api, type FileChange } from "~/api";
import { cachePerChanges, useAsync } from "~/hooks/useAsync";

const NONE: ReadonlySet<string> = new Set();

// Kept per `changes`, so a chat opened again shows its replies' files without asking the backend.
const cacheFor = cachePerChanges<ReadonlySet<string>>();

/**
 * Which of `paths` no longer exist in the project, such as a deleted file git never tracked.
 * Paths with uncommitted changes are left out, since their status says whether they were deleted.
 * Checked again whenever `changes` updates, which follows every settled burst of file changes.
 * With a `subject`, such as a reply's id, its last result shows while the next one loads.
 */
export function useMissingFiles(
  projectId: string | null,
  paths: string[],
  changes: FileChange[],
  subject?: string,
): ReadonlySet<string> {
  const unchanged = paths.filter((p) => !changes.some((c) => c.path === p)).join("\0");
  const missing = useAsync(
    projectId && unchanged
      ? () => api.missingFiles(projectId, unchanged.split("\0")).then((found) => new Set(found))
      : null,
    [projectId, unchanged, changes],
    { store: cacheFor(changes), key: `${projectId}\0${unchanged}`, subject },
  );
  return missing ?? NONE;
}
