import logoManifest from "../../../../../../assets/logos/manifest.json";

/** Preview image of each app icon color way, keyed by id. */
export const LOGO_URLS: Record<string, string> = Object.fromEntries(
  Object.entries(
    import.meta.glob<string>("../../../../../../assets/logos/*.svg", {
      eager: true,
      query: "?url",
      import: "default",
    }),
  ).map(([path, url]) => [path.slice(path.lastIndexOf("/") + 1, -".svg".length), url]),
);

/** The icon Patchwork draws for a saved `logo` id, falling back to the default one. */
export function logoUrl(id: string | null | undefined): string {
  return LOGO_URLS[id ?? logoManifest.default];
}
