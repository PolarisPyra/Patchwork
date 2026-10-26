import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  ArrowClockwiseIcon,
  CaretLeftIcon,
  MagnifyingGlassIcon,
  PlusIcon,
} from "@phosphor-icons/react";
import {
  api,
  AGENT_LABELS,
  type CliKind,
  type CliMcpGroup,
  type CliMcpServer,
  type McpServer,
  type McpServerView,
  type McpTool,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import { errorText } from "~/lib/util";

const INPUT =
  "h-7 w-full min-w-0 rounded-[var(--radius-sm)] border border-border-strong bg-editor px-2 text-[length:var(--fs-sm)] text-fg outline-none focus:border-accent";

type Status = McpServerView["status"] | CliMcpServer["status"];
type Client = "patchwork" | "claude" | "codex";

const DOT: Record<Status, string> = {
  ready: "bg-success",
  starting: "bg-warning animate-pulse",
  listing: "bg-warning animate-pulse",
  unapproved: "bg-fg-subtle",
  signin: "bg-success",
  error: "bg-danger",
  off: "bg-fg-subtle",
};

/** Which status a server shared by several clients shows: the best one. */
const RANK: Status[] = ["ready", "signin", "starting", "listing", "error", "unapproved", "off"];

const SCOPES: Record<CliMcpServer["scope"], string> = {
  user: "User",
  project: "Project",
  local: "Local",
};

/** A server being added (`previous` null) or changed. */
interface Draft {
  previous: string | null;
  name: string;
  server: McpServer;
}

/** One client's entry for a server. */
interface Use {
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
interface Card {
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
const STORED = "••••••";

/** The tools of `card` that API turns get. */
function onTools(card: Card): McpTool[] {
  const off = new Set(card.own?.disabledTools ?? []);
  return card.tools.filter((t) => !off.has(t.name));
}

export default function McpServersPane({ appIcon }: { appIcon: string }) {
  const [servers, setServers] = useState<McpServerView[] | null>(null);
  const [clis, setClis] = useState<CliMcpGroup[]>([]);
  const [file, setFile] = useState("");
  const [editing, setEditing] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Client | null>(null);
  const [query, setQuery] = useState("");
  const [openId, setOpenId] = useState<string | null>(null);
  const [budget, setBudget] = useState(Infinity);

  const reload = async () => {
    try {
      const [own, cli] = await Promise.all([api.listMcpServers(), api.listCliMcpServers()]);
      setServers(own);
      setClis(cli);
    } catch (e) {
      setError(errorText(e));
    }
  };

  useEffect(() => {
    void reload();
    void api.mcpConfigFile().then(setFile);
    void api.mcpToolBudget().then(setBudget);
  }, []);

  // Servers start, and CLI servers list their tools, in the background; follow them until each
  // is done.
  const busy =
    (servers?.some((s) => s.status === "starting") ?? false) ||
    clis.some((g) => g.servers.some((s) => s.status === "listing"));
  useEffect(() => {
    if (!busy) return;
    const timer = setInterval(() => void reload(), 1000);
    return () => clearInterval(timer);
  }, [busy]);

  const cards = useMemo(() => merge(servers ?? [], clis, file), [servers, clis, file]);
  // Tools API turns would get; past the budget, the last ones are left out.
  const onCount = cards
    .filter((c) => c.own && c.own.status === "ready")
    .reduce((n, c) => n + onTools(c).length, 0);
  const over = onCount > budget;

  const run = async (action: Promise<void>) => {
    setError(null);
    try {
      await action;
    } catch (e) {
      setError(errorText(e));
    }
    await reload();
  };

  if (servers === null) return null;

  if (editing !== null) {
    return (
      <ServerForm
        draft={editing}
        onDone={() => {
          setEditing(null);
          void reload();
        }}
      />
    );
  }

  const logo = (client: Client, size: number) =>
    client === "patchwork" ? (
      <img src={appIcon} width={size} height={size} alt="" className="flex-none" />
    ) : (
      <ProviderLogos kind={client} size={size} />
    );
  const open = (path: string) => void run(api.openMcpConfig(path));
  const errorLine = error && (
    <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>
  );

  const card = cards.find((c) => c.id === openId);
  if (card) {
    return (
      <>
        <ServerDetail
          card={card}
          logo={logo}
          onBack={() => setOpenId(null)}
          onOpen={open}
          onEdit={(s) => setEditing({ previous: s.name, name: s.name, server: ownConfig(s) })}
          run={run}
        />
        {errorLine}
      </>
    );
  }

  const q = query.trim().toLowerCase();
  const matches = (c: Card) =>
    q ? c.tools.filter((t) => `${t.name} ${t.description}`.toLowerCase().includes(q)) : [];
  const shown = cards.filter(
    (c) =>
      (!filter || c.uses.some((u) => u.client === filter)) &&
      (!q || c.name.toLowerCase().includes(q) || matches(c).length > 0),
  );
  const clients: Client[] = ["patchwork", "claude", "codex"];
  const label = (c: Client) => (c === "patchwork" ? "Patchwork" : AGENT_LABELS[c]);

  return (
    <>
      <div className="flex items-center gap-1.5 pb-3">
        <button
          className={chip(filter === null)}
          aria-pressed={filter === null}
          onClick={() => setFilter(null)}
        >
          All <span className="text-fg-muted">{cards.length}</span>
        </button>
        {clients.map((c) => (
          <button
            key={c}
            className={chip(filter === c)}
            aria-pressed={filter === c}
            onClick={() => setFilter(filter === c ? null : c)}
          >
            {logo(c, 14)}
            {label(c)}
            <span className="text-fg-muted">
              {cards.filter((card) => card.uses.some((u) => u.client === c)).length}
            </span>
          </button>
        ))}
        <label className="relative ml-auto w-52">
          <MagnifyingGlassIcon
            size={13}
            className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-fg-muted"
          />
          <input
            className={`${INPUT} pl-7`}
            value={query}
            placeholder="Search servers and tools"
            spellCheck={false}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
      </div>
      {over && (
        <div className="mb-3 rounded-[var(--radius-md)] border border-warning/40 bg-warning/10 px-3 py-2 text-[length:var(--fs-sm)]">
          {onCount} tools are on, and an API request takes {budget}. The last {onCount - budget} are
          left out; turn some off on a server's page.
        </div>
      )}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(196px,1fr))] gap-2">
        {shown.map((c) => (
          <ServerCard
            key={c.id}
            card={c}
            logo={logo}
            matches={matches(c)}
            over={over}
            onOpen={() => setOpenId(c.id)}
          />
        ))}
        {!q && (
          <button
            className="flex min-h-[124px] cursor-pointer flex-col items-center justify-center gap-1.5 rounded-[var(--radius-md)] border border-dashed border-border-strong bg-transparent text-fg-muted hover:border-accent hover:text-fg"
            onClick={() => setEditing({ previous: null, name: "", server: {} })}
          >
            <PlusIcon size={20} />
            <span className="text-[length:var(--fs-sm)]">Add server</span>
          </button>
        )}
      </div>
      {q && shown.length === 0 && (
        <div className="py-6 text-center text-fg-muted">Nothing matches “{query.trim()}”</div>
      )}
      {errorLine}
    </>
  );
}

function chip(on: boolean): string {
  return `flex h-7 cursor-pointer items-center gap-1.5 rounded-full border px-2.5 text-[length:var(--fs-sm)] ${on ? "border-accent bg-accent-soft text-fg" : "border-border bg-transparent text-fg hover:bg-hover"}`;
}

/** Patchwork's servers and the CLIs', one card per server: the same command, arguments and
 * environment, or URL and headers, wherever it is configured. */
function merge(own: McpServerView[], clis: CliMcpGroup[], file: string): Card[] {
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
      file,
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

function ownConfig(s: McpServerView): McpServer {
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

/** A letter on a color picked from the name, so each server is easy to spot. */
function Badge({ name, size }: { name: string; size: number }) {
  let hash = 0;
  for (const c of name) hash = (hash * 31 + c.charCodeAt(0)) | 0;
  const hue = Math.abs(hash) % 360;
  return (
    <span
      className="grid flex-none place-items-center rounded-[var(--radius-md)] font-semibold uppercase"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.45,
        background: `hsl(${hue} 70% 55% / 0.18)`,
        color: `hsl(${hue} 75% 65%)`,
      }}
    >
      {name.replace(/[^a-z0-9]/gi, "").charAt(0) || "?"}
    </span>
  );
}

function ServerCard({
  card,
  logo,
  matches,
  over,
  onOpen,
}: {
  card: Card;
  logo: (client: Client, size: number) => ReactNode;
  matches: McpTool[];
  /** API turns have more tools on than a request takes. */
  over: boolean;
  onOpen: () => void;
}) {
  const clients = [...new Set(card.uses.map((u) => u.client))];
  const on = onTools(card).length;
  const warn = over && card.own?.status === "ready" && on > 0;
  const tally =
    card.status === "error" ? (
      <span className="text-danger">Error</span>
    ) : card.status === "starting" || card.status === "listing" ? (
      <span className="animate-pulse text-fg-muted">…</span>
    ) : card.status === "unapproved" ? (
      <span className="text-[length:var(--fs-sm)] font-normal text-fg-muted">Not approved</span>
    ) : card.status === "signin" && card.tools.length === 0 ? (
      <span className="text-[length:var(--fs-sm)] font-normal text-fg-muted">Signs in</span>
    ) : (
      <span className={warn ? "text-warning" : ""}>
        {on}
        <span className="ml-1 text-[length:var(--fs-sm)] font-normal text-fg-muted">
          {on < card.tools.length ? `of ${card.tools.length} tools` : on === 1 ? "tool" : "tools"}
        </span>
      </span>
    );
  return (
    <button
      className={`flex min-h-[124px] min-w-0 cursor-pointer flex-col gap-2.5 rounded-[var(--radius-md)] border border-border bg-transparent p-3 text-left text-fg hover:border-border-strong hover:bg-hover ${card.status === "off" || card.status === "unapproved" ? "opacity-55" : ""}`}
      title={card.status === "error" ? (card.uses.find((u) => u.error)?.error ?? "") : card.target}
      onClick={onOpen}
    >
      <div className="flex w-full min-w-0 items-center gap-2.5">
        <Badge name={card.name} size={32} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="truncate font-semibold">{card.name}</span>
            <span className={`size-1.5 flex-none rounded-full ${DOT[card.status]}`} />
          </div>
          <div className="truncate font-mono text-[length:var(--fs-xs)] text-fg-muted">
            {card.target}
          </div>
        </div>
      </div>
      {matches.length > 0 ? (
        <div className="flex flex-wrap gap-1">
          {matches.slice(0, 3).map((t) => (
            <span
              key={t.name}
              className="max-w-full truncate rounded-[var(--radius-sm)] bg-accent-soft px-1.5 font-mono text-[length:var(--fs-xs)]"
            >
              {t.name}
            </span>
          ))}
          {matches.length > 3 && (
            <span className="text-[length:var(--fs-xs)] text-fg-muted">+{matches.length - 3}</span>
          )}
        </div>
      ) : (
        <div className="flex-1" />
      )}
      <div className="flex w-full items-center justify-between">
        <span className="text-[length:var(--fs-xl)] leading-none font-semibold">{tally}</span>
        <span className="flex items-center gap-1">
          {clients.map((c) => (
            <span
              key={c}
              className="grid size-6 place-items-center rounded-full border border-border bg-white"
              title={c === "patchwork" ? "Patchwork" : AGENT_LABELS[c]}
            >
              {logo(c, 14)}
            </span>
          ))}
        </span>
      </div>
    </button>
  );
}

function ServerDetail({
  card,
  logo,
  onBack,
  onOpen,
  onEdit,
  run,
}: {
  card: Card;
  logo: (client: Client, size: number) => ReactNode;
  onBack: () => void;
  onOpen: (path: string) => void;
  onEdit: (s: McpServerView) => void;
  run: (action: Promise<void>) => Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const tools = card.tools.filter(
    (t) => !q || `${t.name} ${t.description}`.toLowerCase().includes(q),
  );
  // Patchwork's own server can leave tools out of API turns; the buttons act on the tools shown.
  const own = card.own;
  const off = new Set(own?.disabledTools ?? []);
  const setOff = (names: Set<string>) =>
    own &&
    void run(
      api.saveMcpServer(own.name, { ...ownConfig(own), disabledTools: [...names] }, own.name),
    );
  const turn = (on: boolean) => {
    const next = new Set(off);
    for (const t of tools)
      if (on) next.delete(t.name);
      else next.add(t.name);
    setOff(next);
  };
  // A CLI's server Patchwork can run too, unless it signs in through the CLI.
  const enable = card.uses.some((u) => u.client === "patchwork")
    ? undefined
    : card.uses.find((u) => u.cli && u.status !== "signin" && u.status !== "unapproved")?.cli;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-3">
        <button className="btn-icon" title="Back" aria-label="Back" onClick={onBack}>
          <CaretLeftIcon size={16} />
        </button>
        <Badge name={card.name} size={40} />
        <div className="min-w-0 flex-1">
          <div className="text-[length:var(--fs-lg)] font-semibold">{card.name}</div>
          <div className="font-mono text-[length:var(--fs-sm)] break-all text-fg-muted">
            {card.target}
          </div>
        </div>
        {enable && (
          <button
            className="btn btn-primary flex-none"
            onClick={() => void run(api.enableCliMcpServer(enable.key))}
          >
            Enable in Patchwork
          </button>
        )}
      </div>
      <div className="flex flex-col gap-1.5">
        {card.uses.map((u) => {
          return (
            <div
              key={`${u.client}:${u.file}:${u.name}`}
              className="flex items-center gap-3 rounded-[var(--radius-md)] border border-border px-3 py-2.5"
            >
              <span className="relative grid size-8 flex-none place-items-center rounded-full border border-border bg-white">
                {logo(u.client, 16)}
                <span
                  className={`absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full ring-2 ring-elevated ${DOT[u.status]}`}
                />
              </span>
              <div className="min-w-0 flex-1">
                <div className="font-medium">
                  {u.client === "patchwork" ? "Patchwork" : AGENT_LABELS[u.client]}
                </div>
                {u.error ? (
                  <div className="truncate text-[length:var(--fs-sm)] text-danger" title={u.error}>
                    {u.error}
                  </div>
                ) : (
                  <div className="flex min-w-0 items-center gap-1.5 text-[length:var(--fs-xs)] text-fg-muted">
                    <button
                      className="min-w-0 cursor-pointer truncate border-none bg-transparent p-0 font-mono text-fg-muted hover:text-fg hover:underline"
                      title="Open in editor"
                      onClick={() => onOpen(u.file)}
                    >
                      {home(u.file)}
                    </button>
                    {u.cli?.relayed && <span className="flex-none">· runs Patchwork's copy</span>}
                    {u.status === "unapproved" && (
                      <span className="flex-none">
                        ·{" "}
                        {u.client === "codex"
                          ? "project not trusted in Codex"
                          : "not approved in Claude Code"}
                      </span>
                    )}
                  </div>
                )}
              </div>
              {u.scope && (
                <span className="flex-none rounded-[var(--radius-sm)] bg-hover px-1.5 text-[length:var(--fs-xs)] text-fg-muted">
                  {u.scope}
                </span>
              )}
              {u.own && (
                <>
                  {(u.status === "error" || u.status === "ready") && (
                    <button
                      className="btn-icon"
                      title="Restart"
                      aria-label="Restart"
                      onClick={() => void run(api.restartMcpServer(u.name))}
                    >
                      <ArrowClockwiseIcon size={15} />
                    </button>
                  )}
                  <button className="btn" onClick={() => onEdit(u.own!)}>
                    Edit
                  </button>
                  <button
                    className="toggle"
                    role="switch"
                    aria-label={`Use ${u.name}`}
                    aria-checked={u.status !== "off"}
                    onClick={() => {
                      const server = ownConfig(u.own!);
                      void run(
                        api.saveMcpServer(
                          u.name,
                          { ...server, disabled: !server.disabled },
                          u.name,
                        ),
                      );
                    }}
                  />
                </>
              )}
              {u.cli &&
                (u.status !== "off" && u.status !== "listing" && u.status !== "unapproved" ? (
                  <button
                    className="btn-icon"
                    title="List tools again"
                    aria-label="List tools again"
                    onClick={() => void run(api.refreshCliMcpServer(u.cli!.key))}
                  >
                    <ArrowClockwiseIcon size={15} />
                  </button>
                ) : (
                  // Keeps the scope in line with the rows that have the button.
                  <span className="size-7 flex-none" />
                ))}
            </div>
          );
        })}
      </div>
      {own?.command && card.uses.some((u) => u.cli) && (
        <label className="flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-border px-3 py-2.5">
          <span>
            {[
              ...new Set(
                card.uses.filter((u) => u.cli).map((u) => AGENT_LABELS[u.client as CliKind]),
              ),
            ].join(" and ")}{" "}
            use Patchwork's copy
          </span>
          <button
            className="toggle"
            role="switch"
            aria-checked={!!own.share}
            onClick={() =>
              void run(
                api.saveMcpServer(own.name, { ...ownConfig(own), share: !own.share }, own.name),
              )
            }
          />
        </label>
      )}
      {card.env.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          {card.env.map((e) => (
            <span
              key={e}
              className="rounded-[var(--radius-sm)] bg-hover px-1.5 font-mono text-[length:var(--fs-xs)] text-fg-muted"
            >
              {e}
            </span>
          ))}
        </div>
      )}
      {card.tools.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <span className="flex-none font-semibold">
              {own
                ? `${onTools(card).length} of ${card.tools.length} tools on`
                : `${card.tools.length} tools`}
            </span>
            {own && (
              <>
                <button className="btn" onClick={() => turn(true)}>
                  All on
                </button>
                <button className="btn" onClick={() => turn(false)}>
                  All off
                </button>
              </>
            )}
            {card.tools.length > 8 && (
              <input
                className={`${INPUT} ml-auto w-52!`}
                value={query}
                placeholder="Filter tools"
                spellCheck={false}
                onChange={(e) => setQuery(e.target.value)}
              />
            )}
          </div>
          <div className="grid grid-cols-2 gap-1.5">
            {tools.map((t) => {
              const body = (
                <>
                  <div className="truncate font-mono text-[length:var(--fs-sm)]">{t.name}</div>
                  <div className="line-clamp-2 text-[length:var(--fs-xs)] text-fg-muted">
                    {t.description}
                  </div>
                </>
              );
              const tile = "min-w-0 rounded-[var(--radius-sm)] border px-2.5 py-1.5 text-left";
              if (!own) {
                return (
                  <div key={t.name} className={`${tile} border-border`} title={t.description}>
                    {body}
                  </div>
                );
              }
              const isOn = !off.has(t.name);
              return (
                <button
                  key={t.name}
                  className={`${tile} cursor-pointer bg-transparent text-fg ${isOn ? "border-accent/60 hover:border-accent" : "border-border opacity-50 hover:opacity-80"}`}
                  role="switch"
                  aria-checked={isOn}
                  title={t.description}
                  onClick={() => {
                    const next = new Set(off);
                    if (isOn) next.add(t.name);
                    else next.delete(t.name);
                    setOff(next);
                  }}
                >
                  {body}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

/** A path with the home folder shown as `~`. */
function home(path: string): string {
  return path.replace(/^(\/home\/[^/]+|\/Users\/[^/]+|[A-Za-z]:\\Users\\[^\\]+)/, "~");
}

function cliTarget(s: CliMcpServer): string {
  return target({ command: s.command ?? undefined, args: s.args, url: s.url ?? undefined });
}

/** The command line or URL a server runs at. */
function target(server: McpServer): string {
  if (server.url) return server.url;
  return [server.command ?? "", ...(server.args ?? [])].map(quote).join(" ");
}

function quote(word: string): string {
  return /^[\w@%+=:,./~-]+$/.test(word) ? word : `"${word.replace(/(["\\])/g, "\\$1")}"`;
}

/** Splits a command line into words, honoring quotes and backslashes. */
function words(line: string): string[] {
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
function pairsText(pairs: Record<string, string> | undefined, sep: string): string {
  return Object.entries(pairs ?? {})
    .map(([k, v]) => `${k}${sep}${v || STORED}`)
    .join("\n");
}

function parsePairs(text: string, sep: string): Record<string, string> {
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
function parseServers(text: string, name: string): [string, McpServer][] {
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
function clean(s: McpServer): McpServer {
  return s.url
    ? { url: s.url, headers: s.headers, disabled: s.disabled }
    : { command: s.command, args: s.args, env: s.env, disabled: s.disabled };
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[length:var(--fs-sm)] font-medium text-fg-muted">{label}</span>
      {children}
    </label>
  );
}

function ServerForm({ draft, onDone }: { draft: Draft; onDone: () => void }) {
  const [name, setName] = useState(draft.name);
  const [line, setLine] = useState(draft.previous === null ? "" : target(draft.server));
  const isUrl = /^https?:\/\//i.test(line.trim());
  const isJson = line.trim().startsWith("{");
  const sep = isUrl ? ":" : "=";
  const [pairs, setPairs] = useState(
    draft.server.url ? pairsText(draft.server.headers, ": ") : pairsText(draft.server.env, "="),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      if (isJson) {
        for (const [n, s] of parseServers(line, name)) await api.saveMcpServer(n, s);
      } else {
        const server: McpServer = isUrl
          ? { url: line.trim(), headers: parsePairs(pairs, sep) }
          : (() => {
              const [command, ...args] = words(line);
              return { command, args, env: parsePairs(pairs, sep) };
            })();
        server.disabled = draft.server.disabled;
        server.disabledTools = draft.server.disabledTools;
        server.share = draft.server.share;
        await api.saveMcpServer(name, server, draft.previous);
      }
      onDone();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      await api.removeMcpServer(draft.previous ?? name);
      onDone();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 py-2.5">
      <Field label="Name">
        <input
          className={INPUT}
          value={name}
          autoFocus={!name}
          spellCheck={false}
          onChange={(e) => setName(e.target.value)}
        />
      </Field>
      <Field label={isJson ? "JSON" : "Command or URL"}>
        {isJson ? (
          <textarea
            className={`${INPUT} h-40 resize-y py-1.5 font-mono`}
            value={line}
            autoFocus
            spellCheck={false}
            onChange={(e) => setLine(e.target.value)}
          />
        ) : (
          <input
            className={`${INPUT} font-mono`}
            value={line}
            placeholder='npx -y @playwright/mcp@latest, https://…, or {"mcpServers": …}'
            spellCheck={false}
            onChange={(e) => setLine(e.target.value)}
          />
        )}
      </Field>
      {!isJson && (
        <Field label={isUrl ? "Headers" : "Environment"}>
          <textarea
            className={`${INPUT} h-20 resize-y py-1.5 font-mono`}
            value={pairs}
            placeholder={isUrl ? "Authorization: Bearer …" : "API_KEY=…"}
            spellCheck={false}
            onChange={(e) => setPairs(e.target.value)}
          />
        </Field>
      )}
      {error && <div className="text-[length:var(--fs-sm)] text-danger">{error}</div>}
      <div className="flex items-center gap-2 border-t border-border pt-3">
        <button
          className="btn btn-primary"
          disabled={busy || !line.trim() || (!isJson && !name.trim())}
          onClick={() => void save()}
        >
          {busy ? "Saving…" : isJson ? "Import" : "Save"}
        </button>
        <button className="btn" disabled={busy} onClick={onDone}>
          Cancel
        </button>
        {draft.previous !== null && (
          <button
            className={`btn ml-auto ${confirmRemove ? "text-danger" : ""}`}
            disabled={busy}
            onClick={() => (confirmRemove ? void remove() : setConfirmRemove(true))}
          >
            {confirmRemove ? "Remove server" : "Remove"}
          </button>
        )}
      </div>
    </div>
  );
}
