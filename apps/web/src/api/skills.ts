import { Channel } from "@tauri-apps/api/core";
import { invoke } from "~/api/invoke";
import type { Plugin, Skill } from "~/api/types";

export interface SkillsView {
  /** The skills folder in use. */
  dir: string;
  skills: Skill[];
}

export const skillsApi = {
  listSkills: () => invoke<SkillsView>("list_skills"),
  openSkillsFolder: () => invoke<void>("open_skills_folder"),
  /** The skills now, and on `onChange` after each change to the folder until `unwatchSkills`. */
  watchSkills: (onChange: (view: SkillsView) => void) => {
    const channel = new Channel<SkillsView>();
    channel.onmessage = onChange;
    return invoke<SkillsView>("watch_skills", { onChange: channel });
  },
  unwatchSkills: () => invoke<void>("unwatch_skills"),
  openSkill: (path: string) => invoke<void>("open_skill", { path }),
  /** The plugin bundles in the plugins folder, with their skills and MCP servers. */
  listPlugins: () => invoke<Plugin[]>("list_plugins"),
  openPluginsFolder: () => invoke<void>("open_plugins_folder"),
};
