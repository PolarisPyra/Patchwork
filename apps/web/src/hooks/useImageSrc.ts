import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api, type Attachment } from "~/api";

// The paths the backend has allowed since the app started, so an image shown again appears at
// once instead of after another round trip, which also changed its row's height.
const allowed = new Set<string>();

/**
 * The webview URL for an image attachment, or null for other kinds and while it loads.
 * Asks the backend to allow the file first, since the asset scope is not kept across restarts.
 */
export function usePreviewSrc(attachment: Attachment): string | null {
  return useImageSrc(attachment.kind === "image" ? attachment.path : null);
}

/** The webview URL for an image file at `path`, or null for no path and while it loads. */
export function useImageSrc(path: string | null): string | null {
  const [src, setSrc] = useState<string | null>(() =>
    path && allowed.has(path) ? convertFileSrc(path) : null,
  );
  useEffect(() => {
    if (!path) return;
    if (allowed.has(path)) {
      setSrc(convertFileSrc(path));
      return;
    }
    let cancelled = false;
    api.allowPreview(path).then(
      () => {
        allowed.add(path);
        if (!cancelled) setSrc(convertFileSrc(path));
      },
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, [path]);
  return src;
}
