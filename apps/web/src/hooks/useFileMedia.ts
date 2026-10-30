import { api, type FileChange, type FileMedia } from "~/api";
import { cachePerChanges, useAsync } from "~/hooks/useAsync";

const cacheFor = cachePerChanges<FileMedia[]>();

/**
 * What each of `paths` is shown as, for the cards of a reply's produced files. Null until loaded.
 * The backend decides what may be previewed, so the UI only ever shows what it returns. Given
 * `changes`, it is kept until they update, so a chat opened again shows its cards without asking.
 */
export function useFileMedia(
  projectId: string | null,
  paths: string[],
  changes?: FileChange[],
): FileMedia[] | null {
  const key = paths.join("\0");
  return useAsync<FileMedia[]>(
    projectId && key ? () => api.fileMedia(projectId, key.split("\0")) : null,
    [projectId, key, changes],
    changes && { store: cacheFor(changes), key: `${projectId}\0${key}` },
  );
}
