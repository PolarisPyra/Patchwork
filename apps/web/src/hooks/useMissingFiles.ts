import { api, type FileChange } from "~/api";
import { useAsync } from "~/hooks/useAsync";

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
  const unchanged = paths.filter((p) => !changes.some((c) => c.path === p)).join("\0");
  const missing = useAsync(
    projectId && unchanged
      ? () => api.missingFiles(projectId, unchanged.split("\0")).then((found) => new Set(found))
      : null,
    [projectId, unchanged, changes],
  );
  return missing ?? NONE;
}
