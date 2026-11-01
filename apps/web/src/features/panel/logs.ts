import { AGENT_LABELS, type AgentKind, type LoggingSettings } from "~/api";

/** A log the Output panel can show: its name, and the setting that sets its level. */
export interface Channel {
  /** Its path in the launch's log folder, such as "agents/claude.log". */
  id: string;
  name: string;
  setting: keyof LoggingSettings;
}

const NAMED: Record<string, Omit<Channel, "id">> = {
  "main.log": { name: "Main", setting: "app" },
  "window/renderer.log": { name: "Window", setting: "window" },
  "imports.log": { name: "Imports", setting: "imports" },
};

export function channel(id: string): Channel {
  const named = NAMED[id];
  if (named) return { id, ...named };
  const agent = /^agents\/(\w+)\.log$/.exec(id)?.[1];
  const label = agent && AGENT_LABELS[agent as AgentKind];
  return { id, name: label ?? id, setting: "agents" };
}

/** Main and Window first, then the agents, then the rest, as VS Code lists core logs. */
export function sortChannels(ids: string[]): Channel[] {
  const rank = (c: Channel) =>
    c.id === "main.log" ? 0 : c.id === "window/renderer.log" ? 1 : c.setting === "agents" ? 2 : 3;
  return ids.map(channel).sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}
