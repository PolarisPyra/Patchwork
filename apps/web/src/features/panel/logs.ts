import { AGENT_LABELS, type AgentKind, type LoggingSettings } from "~/api";

/** A log the Output panel can show: its name, and the setting that sets its level. */
export interface Channel {
  /** Its path in the launch's log folder, such as "agent-claude.log". */
  id: string;
  name: string;
  setting: Exclude<keyof LoggingSettings, "threadMenu">;
}

const NAMED: Record<string, Omit<Channel, "id">> = {
  "main.log": { name: "Main", setting: "app" },
  "window.log": { name: "Window", setting: "window" },
  "imports.log": { name: "Imports", setting: "imports" },
};

export function channel(id: string): Channel {
  const named = NAMED[id];
  if (named) return { id, ...named };
  const agent = /^agent-(\w+)\.log$/.exec(id)?.[1];
  const label = agent && AGENT_LABELS[agent as AgentKind];
  return { id, name: label ?? id, setting: "agents" };
}

/** Main and Window first, then the agents, then the rest. */
export function sortChannels(ids: string[]): Channel[] {
  const rank = (c: Channel) =>
    c.id === "main.log" ? 0 : c.id === "window.log" ? 1 : c.setting === "agents" ? 2 : 3;
  return ids.map(channel).sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}
