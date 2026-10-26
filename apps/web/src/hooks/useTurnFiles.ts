import { useEffect, useState } from "react";
import { api, type FileChange, type TurnFile, type TurnTrees } from "~/api";

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
  const [files, setFiles] = useState<TurnFile[] | null>(null);
  const key = paths.join("\0");
  const before = turn?.before;
  const after = turn?.after;
  useEffect(() => {
    if (!projectId || !before || !after || !key) {
      setFiles(null);
      return;
    }
    let live = true;
    api.turnFiles(projectId, { before, after }, key.split("\0")).then(
      (found) => {
        if (live) setFiles(found);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [projectId, before, after, key, changes]);
  return files;
}
