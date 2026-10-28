import { api, type FileMedia } from "~/api";
import { useAsync } from "~/hooks/useAsync";

/**
 * What each of `paths` is shown as, for the cards of a reply's produced files. Null until loaded.
 * The backend decides what may be previewed, so the UI only ever shows what it returns.
 */
export function useFileMedia(projectId: string | null, paths: string[]): FileMedia[] | null {
  const key = paths.join("\0");
  return useAsync<FileMedia[]>(
    projectId && key ? () => api.fileMedia(projectId, key.split("\0")) : null,
    [projectId, key],
  );
}
