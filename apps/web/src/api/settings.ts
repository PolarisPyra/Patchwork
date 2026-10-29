import { invoke } from "~/api/invoke";
import type { PermissionRule, Settings, ShellChoices } from "~/api/types";

/** Saves the settings: the next value, or a change to the current one. */
export type UpdateSettings = (next: Settings | ((s: Settings) => Settings)) => Promise<void>;

export const settingsApi = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  /** The rules Settings → Permissions starts with, which Restore defaults puts back. */
  defaultPermissionRules: () => invoke<PermissionRule[]>("default_permission_rules"),
  /** Installed login shells, as paths from /etc/shells, and the default shell's path. */
  listShells: () => invoke<ShellChoices>("list_shells"),
};
