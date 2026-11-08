import { Channel } from "@tauri-apps/api/core";
import { invoke } from "~/api/invoke";
import type { AgentKind, AgentStatus, ClaudeInfo, CliInfo, CodexInfo } from "~/api/types";

/** The agents that run an installed CLI. */
export type CliKind = Exclude<AgentKind, "api">;

/** A custom agent in Patchwork's agents folder; "&name" in a message summons it. */
export interface CustomAgent {
  /** The file's name without `.md`. */
  name: string;
  /** The frontmatter's `name`, when it differs from the file's. */
  title: string | null;
  description: string | null;
  /** The model the chat switches to when the agent is summoned, if the chat's CLI has it. */
  model: string | null;
  /** Summoned, it stays the chat's agent until another replaces it. */
  primary: boolean;
  path: string;
}

export interface AgentsView {
  /** The agents folder. */
  dir: string;
  agents: CustomAgent[];
}

const agentStatusCache: Partial<Record<AgentKind, AgentStatus>> = {};
const pendingDetect: Partial<Record<AgentKind, Promise<AgentStatus>>> = {};

/** Drops the remembered status, so the next `detectAgent` asks the backend again. */
export function forgetAgentStatus(kind: AgentKind) {
  delete agentStatusCache[kind];
}

export const agentsApi = {
  detectAgent: async (kind: AgentKind, forceRefresh = false): Promise<AgentStatus> => {
    if (!forceRefresh && agentStatusCache[kind]) {
      return agentStatusCache[kind]!;
    }
    if (!pendingDetect[kind]) {
      pendingDetect[kind] = invoke<AgentStatus>("detect_agent", { kind })
        .then((status) => {
          agentStatusCache[kind] = status;
          delete pendingDetect[kind];
          return status;
        })
        .catch((err) => {
          delete pendingDetect[kind];
          throw err;
        });
    }
    return pendingDetect[kind]!;
  },
  /** Downloads and runs an agent CLI's official installer. */
  installCli: (kind: CliKind) => invoke<void>("install_cli", { kind }),
  /** What the installed Codex CLI offers: its version, flags, and models. */
  codexInfo: () => invoke<CodexInfo>("codex_info"),
  /** The output of `codex doctor`, `codex mcp list`, or `codex plugin list`. */
  codexCommand: (kind: "doctor" | "mcp" | "plugins") => invoke<string>("codex_command", { kind }),
  /** The path and version of the installed Claude Code. */
  claudeInfo: () => invoke<ClaudeInfo>("claude_info"),
  /** The output of `claude doctor`, `claude mcp list`, or `claude plugin list`. */
  claudeCommand: (kind: "doctor" | "mcp" | "plugins") => invoke<string>("claude_command", { kind }),
  /** The path, version, models and config settings of the installed OpenCode. */
  opencodeInfo: () => invoke<CliInfo>("opencode_info"),
  /** The output of `opencode mcp list`, `plugin list`, `debug paths`, or its agents as a list. */
  opencodeCommand: (kind: "mcp" | "plugins" | "paths" | "agents") =>
    invoke<string>("opencode_command", { kind }),
  /** The path, version and models of the installed Antigravity CLI. */
  antigravityInfo: () => invoke<CliInfo>("antigravity_info"),
  /** The output of `agy mcp list`, `agy plugin list`, or `agy changelog`. */
  antigravityCommand: (kind: "mcp" | "plugins" | "changelog") =>
    invoke<string>("antigravity_command", { kind }),
  /** The agents now, and on `onChange` after each change to them, until another watch. */
  watchAgents: (onChange: (view: AgentsView) => void) => {
    const channel = new Channel<AgentsView>();
    channel.onmessage = onChange;
    return invoke<AgentsView>("watch_agents", { onChange: channel });
  },
  /** Opens the agent's file in the editor; `projectId` is the project the agent was listed for. */
  openAgent: (path: string, projectId: string | null) =>
    invoke<void>("open_agent", { path, projectId }),
  /** The agents a chat in the project can summon, with the project's own. */
  listAgents: (projectId: string | null) => invoke<CustomAgent[]>("list_agents", { projectId }),
  createAgent: (name: string) => invoke<void>("create_agent", { name }),
};
