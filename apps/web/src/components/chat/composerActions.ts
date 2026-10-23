/**
 * When a slash command takes effect: as soon as it is picked from the menu, when it is sent on
 * its own, or never in Patchwork, since the agent reads it.
 */
type Runs = "pick" | "send" | "agent";

/** Slash commands offered while the draft is a single "/word". */
export const SLASH_COMMANDS = [
  { name: "/model", runs: "pick", description: "Switch the response model for this chat" },
  { name: "/plan", runs: "pick", description: "Switch this chat into plan mode" },
  { name: "/default", runs: "pick", description: "Switch this chat back to normal mode" },
  { name: "/goal", runs: "agent", description: "Keep working until a condition is met" },
  { name: "/new", runs: "send", description: "Start a fresh chat in this project" },
  {
    name: "/clear",
    runs: "send",
    description: "Delete this chat's messages and start the agent fresh",
  },
  {
    name: "/compact",
    runs: "send",
    description: "Ask Claude to summarise the conversation to free context",
  },
] as const satisfies readonly { name: string; runs: Runs; description: string }[];

type Command = (typeof SLASH_COMMANDS)[number];
/** A command Patchwork carries out itself instead of passing it to the agent. */
export type LocalCommand = Exclude<Command, { runs: "agent" }>;

/** The local command a sent draft is, or null when it goes to the agent. */
export function localCommand(text: string): LocalCommand | null {
  return (
    SLASH_COMMANDS.find((c): c is LocalCommand => c.runs !== "agent" && c.name === text) ?? null
  );
}

/**
 * How well a command name matches the typed query (without the "/"): lower is better, and null
 * drops the command. Prefixes rank above matches that start a word, which rank above any match.
 */
export function commandScore(name: string, query: string): number | null {
  const bare = name.slice(1);
  if (bare === query) return 0;
  if (bare.startsWith(query)) return 2;
  for (let i = bare.indexOf(query, 1); i > 0; i = bare.indexOf(query, i + 1)) {
    if ("-_/".includes(bare[i - 1])) return 4;
  }
  return bare.includes(query) ? 6 : null;
}

/** A message that is `/compact`, which Claude Code answers by compacting its context. */
export function isCompactCommand(text: string): boolean {
  return text.trim() === "/compact";
}

/** The objective of a message that starts with `/goal`, or null for any other message. */
export function goalObjective(text: string): string | null {
  const trimmed = text.trim();
  return /^\/goal(\s|$)/.test(trimmed) ? trimmed.slice("/goal".length).trim() : null;
}

/** `/goal clear` or one of its aliases, as the backend reads them. */
export function isGoalClearCommand(text: string): boolean {
  return /^\/goal\s+(clear|stop|off|reset|none|cancel)\s*$/i.test(text.trim());
}
