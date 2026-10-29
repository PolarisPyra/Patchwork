// Typed bridge to the Rust backend. Mirrors apps/server/src/types/ and the
// commands registered in apps/desktop/src-tauri/src/lib.rs — keep all three in sync.
//
// One module per area, each with its commands, the events it listens to, and the shapes they carry
// that are not generated into `~/bindings`. `api` joins the commands into the one object the app
// calls, so a caller never needs to know which module a command lives in.
import { agentsApi } from "~/api/agents";
import { attachmentsApi } from "~/api/attachments";
import { computerApi } from "~/api/computer";
import { filesApi } from "~/api/files";
import { gitApi } from "~/api/git";
import { importsApi } from "~/api/imports";
import { logsApi } from "~/api/logs";
import { mcpApi } from "~/api/mcp";
import { modelsApi } from "~/api/models";
import { projectsApi } from "~/api/projects";
import { scheduledApi } from "~/api/scheduled";
import { settingsApi } from "~/api/settings";
import { skillsApi } from "~/api/skills";
import { terminalApi } from "~/api/terminal";
import { threadsApi } from "~/api/threads";
import { turnsApi } from "~/api/turns";
import { updatesApi } from "~/api/updates";
import { usageApi } from "~/api/usage";

export type * from "~/api/types";
export { DEFAULT_SETTINGS, defaultAgentOf, defaultTerminalSettings } from "~/api/defaults";
export { AGENT_LABELS, SOURCE_LABELS, importSourcesFor } from "~/api/labels";
export type { AgentsView, CliKind, CustomAgent } from "~/api/agents";
export type { ComputerAction } from "~/api/computer";
export type { UpdateSettings } from "~/api/settings";
export type { CliMcpGroup, CliMcpServer, McpServerView, McpTool } from "~/api/mcp";
export {
  apiProviderName,
  apiProviderOf,
  onApiProvidersChanged,
  type ApiProviderView,
  type KeyChange,
} from "~/api/models";
export { onProjectChanged } from "~/api/projects";
export type { SkillsView } from "~/api/skills";
export { onTerminalExit, onTerminalOutput, type StartedTerminal } from "~/api/terminal";
export { onAgentEvent } from "~/api/threads";
export {
  onUpdateStatus,
  type RestartResult,
  type UpdateState,
  type UpdateStatus,
} from "~/api/updates";

export const api = {
  ...settingsApi,
  ...agentsApi,
  ...modelsApi,
  ...mcpApi,
  ...skillsApi,
  ...scheduledApi,
  ...computerApi,
  ...logsApi,
  ...terminalApi,
  ...usageApi,
  ...importsApi,
  ...projectsApi,
  ...threadsApi,
  ...turnsApi,
  ...attachmentsApi,
  ...filesApi,
  ...gitApi,
  ...updatesApi,
};
