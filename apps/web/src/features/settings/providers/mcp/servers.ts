import {
  type CliMcpGroup,
  type CliMcpServer,
  type McpServer,
  type McpServerView,
  type McpTool,
} from "~/api";

export type Status = McpServerView["status"] | CliMcpServer["status"];
export type Client = "patchwork" | "claude" | "codex";

export const DOT: Record<Status, string> = {
  ready: "bg-success",
  starting: "bg-warning animate-pulse",
  listing: "bg-warning animate-pulse",
  unapproved: "bg-fg-subtle",
  signin: "bg-success",
  error: "bg-danger",
  off: "bg-fg-subtle",
};

/** Which status a server shared by several clients shows: the best one. */
export const RANK: Status[] = [
  "ready",
  "signin",
  "starting",
  "listing",
  "error",
  "unapproved",
  "off",
];

export const SCOPES: Record<CliMcpServer["scope"], string> = {
  user: "User",
  project: "Project",
  local: "Local",
};

/** A server being added (`previous` null) or changed. */
export interface Draft {
  previous: string | null;
  name: string;
  server: McpServer;
}

/** One client's entry for a server. */
export interface Use {
  client: Client;
  name: string;
  scope: string | null;
  file: string;
  status: Status;
  error: string | null;
  own?: McpServerView;
  cli?: CliMcpServer;
}

/** A server, once for every client that runs the same command or URL. */
export interface Card {
  id: string;
  name: string;
  target: string;
  env: string[];
  tools: McpTool[];
  status: Status;
  uses: Use[];
  /** Patchwork's own entry for it, whose tools can be turned off. */
  own?: McpServerView;
}

/** Stands in for a stored value in the editor; saving it keeps the value. */
export const STORED = "••••••";

/** The tools of `card` that API turns get. */
export function onTools(card: Card): McpTool[] {
  const off = new Set(card.own?.disabledTools ?? []);
  return card.tools.filter((t) => !off.has(t.name));
}

/** Patchwork's servers and the CLIs', one card per server: the same command, arguments and
 * environment, or URL and headers, wherever it is configured. */
export function merge(own: McpServerView[], clis: CliMcpGroup[]): Card[] {
  const cards = new Map<string, Card>();
  const add = (id: string, target: string, env: string[], tools: McpTool[], use: Use) => {
    const card = cards.get(id);
    if (!card) {
      cards.set(id, {
        id,
        name: use.name,
        target,
        env,
        tools,
        status: use.status,
        uses: [use],
        own: use.own,
      });
      return;
    }
    card.uses.push(use);
    card.own ??= use.own;
    if (card.tools.length === 0) card.tools = tools;
    if (RANK.indexOf(use.status) < RANK.indexOf(card.status)) card.status = use.status;
  };
  for (const s of own) {
    const config = ownConfig(s);
    const env = Object.keys((config.url ? config.headers : config.env) ?? {});
    add(s.identity, target(config), env, s.tools, {
      client: "patchwork",
      name: s.name,
      scope: null,
      file: s.file,
      status: s.status,
      error: s.error,
      own: s,
    });
  }
  for (const g of clis) {
    for (const s of g.servers) {
      const scope = s.project ? `${SCOPES[s.scope]} · ${s.project}` : SCOPES[s.scope];
      add(s.key, cliTarget(s), s.env, s.tools, {
        client: g.agent as Client,
        name: s.name,
        scope,
        file: s.file,
        status: s.status,
        error: s.error,
        cli: s,
      });
    }
  }
  return [...cards.values()];
}

export function ownConfig(s: McpServerView): McpServer {
  const {
    name: _name,
    status: _status,
    tools: _tools,
    error: _error,
    identity: _identity,
    ...server
  } = s;
  return server;
}

export function cliTarget(s: CliMcpServer): string {
  return target({ command: s.command ?? undefined, args: s.args, url: s.url ?? undefined });
}

/** The command line or URL a server runs at. */
export function target(server: McpServer): string {
  if (server.url) return server.url;
  return [server.command ?? "", ...(server.args ?? [])].map(quote).join(" ");
}

export function quote(word: string): string {
  return /^[\w@%+=:,./~-]+$/.test(word) ? word : `"${word.replace(/(["\\])/g, "\\$1")}"`;
}

/** Splits a command line into words, honoring quotes and backslashes. */
export function words(line: string): string[] {
  const out: string[] = [];
  let word: string | null = null;
  let quoteChar: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (c === "\\" && i + 1 < line.length && quoteChar !== "'") {
      word = (word ?? "") + line[++i];
    } else if (quoteChar) {
      if (c === quoteChar) quoteChar = null;
      else word = (word ?? "") + c;
    } else if (c === '"' || c === "'") {
      quoteChar = c;
      word ??= "";
    } else if (/\s/.test(c)) {
      if (word !== null) out.push(word);
      word = null;
    } else {
      word = (word ?? "") + c;
    }
  }
  if (word !== null) out.push(word);
  return out;
}

/** `KEY=value` lines for environment variables, `Name: value` lines for headers. */
export function pairsText(pairs: Record<string, string> | undefined, sep: string): string {
  return Object.entries(pairs ?? {})
    .map(([k, v]) => `${k}${sep}${v || STORED}`)
    .join("\n");
}

export function parsePairs(text: string, sep: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const at = line.indexOf(sep);
    if (at <= 0) continue;
    const value = line.slice(at + 1).trim();
    // An empty value keeps the stored one.
    out[line.slice(0, at).trim()] = value === STORED ? "" : value;
  }
  return out;
}

/** The servers in pasted JSON: an `mcpServers` or `servers` map, a bare map of servers, or one
 * server, which takes `name`. */
export function parseServers(text: string, name: string): [string, McpServer][] {
  const json: unknown = JSON.parse(text);
  if (typeof json !== "object" || json === null) throw new Error("That JSON is not an object");
  const obj = json as Record<string, unknown>;
  const map = (obj.mcpServers ?? obj.servers ?? obj) as Record<string, unknown>;
  const isServer = (v: unknown): v is McpServer =>
    typeof v === "object" && v !== null && ("command" in v || "url" in v);
  if (isServer(map)) {
    if (!name.trim()) throw new Error("Give the server a name");
    return [[name.trim(), clean(map)]];
  }
  const entries = Object.entries(map).filter((e): e is [string, McpServer] => isServer(e[1]));
  if (entries.length === 0) throw new Error("No servers with a command or URL in that JSON");
  return entries.map(([n, s]) => [n, clean(s)]);
}

/** Only the fields Patchwork reads; other clients add their own, such as `type`. */
export function clean(s: McpServer): McpServer {
  return s.url
    ? { url: s.url, headers: s.headers, disabled: s.disabled }
    : { command: s.command, args: s.args, env: s.env, disabled: s.disabled };
}
