import type { FileChange, FileMedia, TurnFile, TurnTrees } from "~/api";
import { useFileMedia } from "~/hooks/useFileMedia";
import { useMissingFiles } from "~/hooks/useMissingFiles";
import { useTurnFiles } from "~/hooks/useTurnFiles";

/** What a reply's files are now. The reply's summary and its cards share one load of it. */
export interface ReplyFiles {
  /** What the reply's turn did to each file; null until loaded, and for replies without snapshots. */
  turnFiles: TurnFile[] | null;
  /** The files that no longer exist in the project. */
  missing: ReadonlySet<string>;
  /** How each file is shown; null until loaded. */
  media: FileMedia[] | null;
}

/** The state of a reply's `paths`, loaded once for the whole reply. */
export function useReplyFiles(
  projectId: string | null,
  turn: TurnTrees | null | undefined,
  paths: string[],
  changes: FileChange[],
): ReplyFiles {
  return {
    turnFiles: useTurnFiles(projectId, turn, paths, changes),
    missing: useMissingFiles(projectId, paths, changes),
    media: useFileMedia(projectId, paths),
  };
}
