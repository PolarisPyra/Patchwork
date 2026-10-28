import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api, type Attachment } from "~/api";

/**
 * The webview URL for an image attachment, or null for other kinds and while it loads.
 * Asks the backend to allow the file first, since the asset scope is not kept across restarts.
 */
export function usePreviewSrc(attachment: Attachment): string | null {
  return useImageSrc(attachment.kind === "image" ? attachment.path : null);
}

/** The webview URL for an image file at `path`, or null for no path and while it loads. */
export function useImageSrc(path: string | null): string | null {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    if (!path) return;
    let cancelled = false;
    api.allowPreview(path).then(
      () => {
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
