import { type ToolCall } from "~/api";
import { titleOf } from "~/features/chat/lib/toolCalls";
import type { Entry } from "~/features/chat/response/model";

/**
 * What a call is, in the terms a reader thinks in — changed a file, ran a command, searched — and
 * how that is written in a row. Pure and icon-free, so the wording can be tested on its own; the
 * glyph for each action lives in `ToolIcon.tsx`.
 */

/** The kinds of work a call can be, which decide both its icon and its wording. */
export type Action = "edit" | "command" | "read" | "search" | "web" | "agent" | "skill" | "other";

/** The action of each tool name, across Claude Code, Codex, OpenCode and Antigravity. */
const ACTION_OF_NAME: Record<string, Action> = {
  Bash: "command",
  BashOutput: "command",
  Shell: "command",
  run_command: "command",
  Read: "read",
  NotebookRead: "read",
  view_file: "read",
  Edit: "edit",
  Write: "edit",
  MultiEdit: "edit",
  NotebookEdit: "edit",
  write_to_file: "edit",
  replace_file_content: "edit",
  multi_replace_file_content: "edit",
  Grep: "search",
  Glob: "search",
  LS: "search",
  grep_search: "search",
  find_by_name: "search",
  list_dir: "search",
  WebSearch: "web",
  WebFetch: "web",
  search_web: "web",
  read_url_content: "web",
  Skill: "skill",
};

/** The action a call does. */
export function actionOf(tool: ToolCall): Action {
  if (tool.subagent) return "agent";
  const action = ACTION_OF_NAME[tool.name] ?? "other";
  // Agents without a skill tool load a skill by reading its SKILL.md.
  return action === "read" && /(^|\/)SKILL\.md$/.test(tool.summary) ? "skill" : action;
}

/** Whether the call is still going. */
export function isRunning(tool: ToolCall): boolean {
  return tool.status === "running" || tool.status === "pending";
}

/** The skill a call loads: Claude names Patchwork's as the "claude" plugin's, and a read names
 * its folder. */
function skillName(tool: ToolCall): string {
  if (tool.name !== "Skill") return tool.summary.split("/").slice(-2, -1)[0] ?? "";
  return tool.summary.replace(/^claude:/, "");
}

/** The program a command runs: its first word, without a path. */
function programOf(command: string): string {
  const word = command.trim().split(/\s+/)[0] ?? "";
  return word.split("/").pop() || "command";
}

/** `mcp__server__tool` and Codex's `server.tool` name the tool last. */
function toolName(tool: ToolCall): string {
  if (tool.name.startsWith("mcp__")) return tool.name.split("__").pop() ?? tool.name;
  if (tool.name === "MCP") return tool.summary.split(".").pop() ?? tool.summary;
  return tool.name;
}

/** A row's label: what the call does while it runs, and what it did once done. */
export function toolLabel(tool: ToolCall): string {
  const { summary } = tool;
  const running = isRunning(tool);
  const verb = (doing: string, done: string) => (running ? doing : done);
  switch (actionOf(tool)) {
    case "command":
      if (running) return `Running ${programOf(summary)}`;
      if (tool.status === "error") return summary ? `Failed: ${summary}` : "Failed command";
      return summary || "Ran command";
    case "read":
      return summary ? `${verb("Reading", "Read")} ${summary}` : "Read file";
    case "edit":
      if (!summary) return "Changed file";
      return `${verb("Editing", created(tool) ? "Created" : "Changed")} ${summary}`;
    case "search":
      return summary ? `${verb("Searching", "Searched")} ${summary}` : "Searched files";
    case "web":
      if (tool.name === "WebFetch") return `${verb("Fetching", "Fetched")} ${summary}`;
      return summary ? `${verb("Searching", "Searched")} ${summary}` : "Searched web";
    case "agent":
      return titleOf(tool);
    case "skill": {
      const name = skillName(tool);
      if (!name) return verb("Loading skill", "Used skill");
      return `${verb("Loading", "Used")} the ${name} skill`;
    }
    case "other":
      return `${verb("Using", "Used")} ${toolName(tool)}`;
  }
}

/** How a group's calls of one kind are counted, most telling first. */
const GROUP_COUNTS: readonly { action: Action; verb: string; noun: string }[] = [
  { action: "edit", verb: "Changed", noun: "file" },
  { action: "command", verb: "Ran", noun: "command" },
  { action: "read", verb: "Read", noun: "file" },
  { action: "search", verb: "Searched code", noun: "time" },
  { action: "web", verb: "Searched web", noun: "time" },
  { action: "agent", verb: "Ran", noun: "subagent" },
  { action: "skill", verb: "Used", noun: "skill" },
  { action: "other", verb: "Used", noun: "tool" },
];

/**
 * "Changed 1 file and ran 1 command": the two kinds of call a group did most tellingly. Only the
 * first verb is capitalised. A verb that already names its object ("Searched code") takes no count,
 * since "1 times" is not a phrase anyone writes.
 */
export function groupLabel(tools: readonly ToolCall[]): string {
  const counts = GROUP_COUNTS.flatMap(({ action, verb, noun }) => {
    const n = tools.filter((tool) => actionOf(tool) === action).length;
    if (n === 0) return [];
    return [noun === "time" ? verb : `${verb} ${n} ${noun}${n === 1 ? "" : "s"}`];
  }).slice(0, 2);
  if (counts.length === 0) return "Thought";
  return counts.map((text, i) => (i === 0 ? text : text.toLowerCase())).join(" and ");
}

/** A row's label. */
export function entryLabel(entry: Entry): string {
  if (entry.kind === "thinking") return entry.part.endedAt === null ? "Thinking" : "Thought";
  return toolLabel(entry.tool);
}

/** The one file an edit call changes, or null for other calls, failed edits, and edits of several
 * files at once. */
export function editedFile(entry: Entry): string | null {
  if (entry.kind !== "tool" || actionOf(entry.tool) !== "edit") return null;
  const { summary, status } = entry.tool;
  return status === "error" || !summary || summary.includes(", ") ? null : summary;
}

/** What a row opens to show: a command and its output. */
export function detailsOf(entry: Extract<Entry, { kind: "tool" }>): string {
  const { tool } = entry;
  const input = actionOf(tool) === "command" ? `$ ${tool.summary}` : tool.summary;
  return [input, tool.output].filter(Boolean).join("\n\n") || "No output";
}

/** Whether a Claude `Write` made a new file rather than replacing one, as its result says. */
function created(tool: ToolCall): boolean {
  return tool.name === "Write" && (tool.output?.startsWith("File created successfully") ?? false);
}

/** The lines an edit added and removed. A `Write` counts all its text, which is right only for a
 * file it created, so one that replaced a file, or has not said yet, shows none. */
export function linesOf(tool: ToolCall): ToolCall["lines"] {
  return tool.name === "Write" && !created(tool) ? null : tool.lines;
}

/** What a command came to, for its row: the tests it ran, or else the exit code it failed with. */
export type CommandResult = { passed: number; failed: number } | { exit: number };

/**
 * "24 passed" or "exit 1", read from a finished command's output. Test counts are summed over the
 * summary lines test runners print ("test result: ok. 246 passed; 0 failed", "Tests: 24 passed"),
 * leaving out the per-file and per-suite totals, which count the same tests again.
 */
export function commandResult(tool: ToolCall): CommandResult | null {
  if (actionOf(tool) !== "command" || isRunning(tool) || !tool.output) return null;
  let passed = 0;
  let failed = 0;
  for (const line of tool.output.split("\n")) {
    if (/Test (Suites|Files)/.test(line)) continue;
    passed += Number(/\b(\d+) passed\b/.exec(line)?.[1] ?? 0);
    failed += Number(/\b(\d+) failed\b/.exec(line)?.[1] ?? 0);
  }
  if (passed + failed > 0) return { passed, failed };
  const code = tool.status === "error" ? /^Exit code (\d+)/.exec(tool.output) : null;
  return code ? { exit: Number(code[1]) } : null;
}
