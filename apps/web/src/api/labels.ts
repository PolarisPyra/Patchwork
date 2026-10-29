import type { CliKind } from "~/api/agents";
import type { AgentKind, ImportSource } from "~/api/types";

export const AGENT_LABELS: Record<AgentKind, string> = {
  claude: "Claude Code",
  codex: "Codex",
  antigravity: "Antigravity CLI",
  opencode: "OpenCode CLI",
  api: "API",
};

export const SOURCE_LABELS: Record<ImportSource, string> = {
  claude: AGENT_LABELS.claude,
  codex: AGENT_LABELS.codex,
  antigravity: AGENT_LABELS.antigravity,
  t3: "T3 Code",
  opencode: "OpenCode",
};

/** Import sources to offer: each connected agent's own files, plus another app and OpenCode. */
export function importSourcesFor(agents: AgentKind[]): ImportSource[] {
  const cli = agents.filter((a): a is CliKind => a !== "opencode" && a !== "api");
  return [...cli, "t3", "opencode"];
}
