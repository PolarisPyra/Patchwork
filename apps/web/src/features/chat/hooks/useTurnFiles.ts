import { api, type FileChange, type TurnFile, type TurnTrees } from "~/api";
import { useAsync } from "~/hooks/useAsync";

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
  );
}
