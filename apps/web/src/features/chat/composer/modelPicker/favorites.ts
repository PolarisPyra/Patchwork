/** The models starred in the picker, as `agent:model` keys, kept across restarts. */

const favoriteKey = "patchwork-model-favorites";

/** Favorites saved before model ids came from the model manifest used CLI aliases. */
const LEGACY_FAVORITES: Record<string, string> = {
  "claude:fable": "claude:claude-fable-5-1",
  "claude:opus": "claude:claude-opus-5-5",
  "claude:sonnet": "claude:claude-sonnet-5-5",
  "claude:haiku": "claude:claude-haiku-5-5",
  "gemini:pro": "antigravity:gemini-3.1-pro-preview",
  "gemini:flash": "antigravity:gemini-3.8-flash",
  "gemini:flash-lite": "antigravity:gemini-3.5-flash-lite",
};

export function readFavorites(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(favoriteKey) ?? "[]");
    if (!Array.isArray(value)) return [];
    const keys = value
      .filter((v): v is string => typeof v === "string")
      // Favorites saved under the agent's old "gemini" name move to "antigravity".
      .map((k) => LEGACY_FAVORITES[k] ?? k.replace(/^gemini:/, "antigravity:"));
    return [...new Set(keys)];
  } catch {
    return [];
  }
}

export function saveFavorites(favorites: string[]) {
  try {
    localStorage.setItem(favoriteKey, JSON.stringify(favorites));
  } catch {
    /* Favorites still work for this session. */
  }
}
