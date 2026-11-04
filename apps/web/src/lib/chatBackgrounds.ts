import type { CSSProperties } from "react";
import type { ChatBackground } from "~/api";

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

/** The background `chat` shows: its own when it has one and the plan allows it, else `shared`. */
export function effectiveBackground(
  own: ChatBackground | null | undefined,
  shared: ChatBackground,
  perChat: boolean,
): ChatBackground {
  return (perChat && own) || shared;
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
