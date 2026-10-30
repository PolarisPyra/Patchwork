import { api, type FileChange, type TurnFile, type TurnTrees } from "~/api";
import { cachePerChanges, useAsync } from "~/hooks/useAsync";

// Kept per `changes`, so a chat opened again shows its replies' files without asking the backend.
const cacheFor = cachePerChanges<TurnFile[]>();

/**
 * What a reply's turn did to each of `paths`, from the snapshots around the turn, and where each
 * file stands now. Null until loaded, and for replies without snapshots. Checked again whenever
 * `changes` updates, which follows every settled burst of file changes, commits included.
 */
export function useTurnFiles(
  projectId: string | null,
  turn: TurnTrees | null | undefined,
  paths: string[],
  changes: FileChange[],
): TurnFile[] | null {
  const key = paths.join("\0");
  const before = turn?.before;
  const after = turn?.after;
  return useAsync<TurnFile[]>(
    projectId && before && after && key
      ? () => api.turnFiles(projectId, { before, after }, key.split("\0"))
      : null,
    [projectId, before, after, key, changes],
    { store: cacheFor(changes), key: `${projectId}\0${before}\0${after}\0${key}` },
  );
}
