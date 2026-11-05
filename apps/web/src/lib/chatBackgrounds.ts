import { useSyncExternalStore, type CSSProperties } from "react";
import { api, type BackgroundImage, type BackgroundStrength, type ChatBackground } from "~/api";

/** A background the app comes with. `css` is the CSS `background` it paints. */
export interface BackgroundPreset {
  id: string;
  name: string;
  css: string;
}

/** A dot pattern laid over a gradient, in the window's own text color. */
const dots = (size: number) =>
  `radial-gradient(color-mix(in srgb, var(--text) 22%, transparent) 1px, transparent 1.6px) 0 0 / ${size}px ${size}px`;

export const BACKGROUND_PRESETS: BackgroundPreset[] = [
  { id: "sky", name: "Sky", css: `${dots(18)}, linear-gradient(160deg, #a1c4fd, #c2e9fb)` },
  { id: "peach", name: "Peach", css: `${dots(18)}, linear-gradient(160deg, #ffecd2, #fcb69f)` },
  { id: "mint", name: "Mint", css: `${dots(18)}, linear-gradient(160deg, #d4fc79, #96e6a1)` },
  {
    id: "lavender",
    name: "Lavender",
    css: `${dots(18)}, linear-gradient(160deg, #e0c3fc, #8ec5fc)`,
  },
  { id: "dusk", name: "Dusk", css: `${dots(22)}, linear-gradient(160deg, #2b5876, #4e4376)` },
  { id: "forest", name: "Forest", css: `${dots(22)}, linear-gradient(160deg, #134e5e, #71b280)` },
  {
    id: "grid",
    name: "Grid",
    css: "linear-gradient(color-mix(in srgb, var(--text) 10%, transparent) 1px, transparent 1px) 0 0 / 24px 24px, linear-gradient(90deg, color-mix(in srgb, var(--text) 10%, transparent) 1px, transparent 1px) 0 0 / 24px 24px, var(--bg-editor)",
  },
  { id: "dots", name: "Dots", css: `${dots(14)}, var(--bg-editor)` },
];

/** How much of the window color is laid over a background, in percent, at each strength. */
export const STRENGTH_VEIL: Record<BackgroundStrength, number> = {
  subtle: 88,
  medium: 78,
  strong: 60,
};

/** The background `chat` shows: its own when it has one, else `shared`. */
export function effectiveBackground(
  own: ChatBackground | null | undefined,
  shared: ChatBackground,
): ChatBackground {
  return own || shared;
}

/**
 * The style that paints `background`, given the webview URL of its picture when it is one. The
 * window color is laid over it, so the text on top reads the same in either theme.
 */
export function backgroundStyle(
  background: ChatBackground,
  imageSrc: string | null,
  veil = 78,
): CSSProperties | null {
  const veilCss = `linear-gradient(color-mix(in srgb, var(--bg-editor) ${veil}%, transparent), color-mix(in srgb, var(--bg-editor) ${veil}%, transparent))`;
  if (background.kind === "preset") {
    const preset = BACKGROUND_PRESETS.find((p) => p.id === background.id);
    return preset ? { background: `${veilCss}, ${preset.css}` } : null;
  }
  if (background.kind === "image" && imageSrc) {
    return { background: `${veilCss}, center / cover no-repeat url("${imageSrc}")` };
  }
  return null;
}

/** The pictures in Patchwork's backgrounds folder, read once for the whole window. */
const library = {
  images: [] as BackgroundImage[],
  listeners: new Set<() => void>(),
  started: false,
};

/** Reads the pictures again, after one was added or removed. */
export function refreshBackgroundImages(): Promise<void> {
  library.started = true;
  return api.listBackgroundImages().then(
    (images) => {
      library.images = images;
      library.listeners.forEach((listener) => listener());
    },
    () => void (library.started = false),
  );
}

const subscribe = (listener: () => void) => {
  if (!library.started) void refreshBackgroundImages();
  library.listeners.add(listener);
  return () => void library.listeners.delete(listener);
};

/** The pictures picked as backgrounds, oldest first; re-renders when one is added or removed. */
export function useBackgroundImages(): BackgroundImage[] {
  return useSyncExternalStore(subscribe, () => library.images);
}

/** Where the picture `background` shows is, or null when it is no picture or it is gone. */
export function backgroundImagePath(
  images: BackgroundImage[],
  background: ChatBackground | null | undefined,
): string | null {
  if (background?.kind !== "image") return null;
  return images.find((image) => image.file === background.file)?.path ?? null;
}
