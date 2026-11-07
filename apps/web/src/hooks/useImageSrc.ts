import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api, type Attachment } from "~/api";

// The paths the backend has allowed since the app started, so an image shown again appears at
// once instead of after another round trip, which also changed its row's height.
const allowed = new Set<string>();

/** Drops `path` once its image failed to load, so the next showing asks the backend again. */
export function forgetPreview(path: string): void {
  allowed.delete(path);
}

/**
 * The webview URL for an image file, once the backend has allowed it. Asks the backend first,
 * since the asset scope is not kept across restarts.
 */
export function previewSrc(path: string): Promise<string> {
  if (allowed.has(path)) return Promise.resolve(convertFileSrc(path));
  return api.allowPreview(path).then(() => {
    allowed.add(path);
    return convertFileSrc(path);
  });
}

/**
 * The webview URL for an image attachment, or null for other kinds and while it loads.
 * Asks the backend to allow the file first, since the asset scope is not kept across restarts.
 */
export function usePreviewSrc(attachment: Attachment): string | null {
  return useImageSrc(attachment.kind === "image" ? attachment.path : null);
}

/** The webview URL for an image file at `path`, or null for no path and while it loads. */
export function useImageSrc(path: string | null): string | null {
  // The URL is kept with the path it was made for: a card reused for another file must not show
  // the last one's picture, which reads as a file from another reply.
  const [shown, setShown] = useState<{ path: string; src: string } | null>(() =>
    path && allowed.has(path) ? { path, src: convertFileSrc(path) } : null,
  );
  useEffect(() => {
    if (!path) {
      setShown(null);
      return;
    }
    if (allowed.has(path)) {
      setShown({ path, src: convertFileSrc(path) });
      return;
    }
    let cancelled = false;
    previewSrc(path).then(
      (src) => {
        if (!cancelled) setShown({ path, src });
      },
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, [path]);
  return shown && shown.path === path ? shown.src : null;
}
