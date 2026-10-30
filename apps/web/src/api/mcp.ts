import { invoke } from "~/api/invoke";
import type { AgentKind, McpServer } from "~/api/types";

export interface McpTool {
  name: string;
  description: string;
}

export interface McpServerView extends McpServer {
  name: string;
  status: "off" | "starting" | "ready" | "error";
  tools: McpTool[];
  error: string | null;
  /** Equal for the same server wherever it is configured; a CLI server's `key`. */
  identity: string;
  /** The file it is configured in. */
  file: string;
}

/** A server Claude Code or Codex runs, read from its own config. Mirrors `CliServer` in
 * apps/server/src/provider/mcp_clis.rs; environment and header values stay out. */
export interface CliMcpServer {
  /** Names it for a refresh; equal for the same server wherever it is configured. */
  key: string;
  name: string;
  scope: "user" | "project" | "local";
  /** The Patchwork project a project or local server belongs to. */
  project: string | null;
  file: string;
  command: string | null;
  args: string[];
  url: string | null;
  /** Names of its environment variables, or of its headers for a URL. */
  env: string[];
  /** `unapproved`: a project's server the CLI has not approved, so it is not started. */
  status: "off" | "unapproved" | "listing" | "ready" | "signin" | "error";
  /** Its turns in Patchwork use Patchwork's copy of it. */
  relayed: boolean;
  tools: McpTool[];
  error: string | null;
}

export interface CliMcpGroup {
  agent: AgentKind;
  file: string;
  servers: CliMcpServer[];
}

export const mcpApi = {
  listMcpServers: () => invoke<McpServerView[]>("list_mcp_servers"),
  /** Adds or replaces a server; `previous` is its old name when renamed. */
  saveMcpServer: (name: string, server: McpServer, previous: string | null = null) =>
    invoke<void>("save_mcp_server", { name, previous, server }),
  removeMcpServer: (name: string) => invoke<void>("remove_mcp_server", { name }),
  restartMcpServer: (name: string) => invoke<void>("restart_mcp_server", { name }),
  /** How many MCP tools an API turn may offer. */
  mcpToolBudget: () => invoke<number>("mcp_tool_budget"),
  listCliMcpServers: () => invoke<CliMcpGroup[]>("list_cli_mcp_servers"),
  /** Reads the server's tools again. */
  refreshCliMcpServer: (key: string) => invoke<void>("refresh_cli_mcp_server", { key }),
  /** Adds a CLI's server to Patchwork's own, for API tasks. */
  enableCliMcpServer: (key: string) => invoke<void>("enable_cli_mcp_server", { key }),
  openMcpConfig: (path: string) => invoke<void>("open_mcp_config", { path }),
};
