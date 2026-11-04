import { useEffect, useMemo, useState } from "react";
import { MagnifyingGlassIcon, PlusIcon } from "@phosphor-icons/react";
import { api, AGENT_LABELS, type CliMcpGroup, type McpServerView } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import { INPUT, chip } from "~/features/settings/controls";
import { errorText } from "~/lib/util";
import {
  type Card,
  type Client,
  type Draft,
  merge,
  onTools,
  ownConfig,
} from "~/features/settings/providers/mcp/servers";
import ServerDetail from "~/features/settings/providers/mcp/ServerDetail";
import ServerForm from "~/features/settings/providers/mcp/ServerForm";
import ServerRow from "~/features/settings/providers/mcp/ServerRow";

export default function McpServersPane({ appIcon }: { appIcon: string }) {
  const [servers, setServers] = useState<McpServerView[] | null>(null);
  const [clis, setClis] = useState<CliMcpGroup[]>([]);
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

  const cards = useMemo(() => merge(servers ?? [], clis), [servers, clis]);
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

  // Nothing to show until the servers are listed; if that failed, say why and offer to try again.
  if (servers === null) {
    return (
      error && (
        <div
          role="alert"
          className="flex items-center gap-2 text-[length:var(--fs-sm)] text-danger"
        >
          {error}
          <button
            className="btn"
            onClick={() => {
              setError(null);
              void reload();
            }}
          >
            Retry
          </button>
        </div>
      )
    );
  }

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
    <div role="alert" className="mt-2 text-[length:var(--fs-sm)] text-danger">
      {error}
    </div>
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
        <label className="relative ml-auto min-w-0 max-w-52 flex-1">
          <MagnifyingGlassIcon
            size={13}
            className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-fg-muted"
          />
          <input
            className={`${INPUT} pl-7`}
            value={query}
            placeholder="Search"
            aria-label="Search MCP servers"
            spellCheck={false}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <button
          className="btn flex-none"
          onClick={() => setEditing({ previous: null, name: "", server: {} })}
        >
          <PlusIcon size={13} />
          Add server
        </button>
      </div>
      {over && (
        <div className="mb-3 rounded-[var(--radius-md)] border border-warning/40 bg-warning/10 px-3 py-2 text-[length:var(--fs-sm)]">
          {onCount} tools are on, and an API request takes {budget}. The last {onCount - budget} are
          left out; turn some off on a server's page.
        </div>
      )}
      {shown.length > 0 && (
        <div className="divide-y divide-border overflow-hidden rounded-[var(--radius-md)] border border-border">
          {shown.map((c) => (
            <ServerRow
              key={c.id}
              card={c}
              logo={logo}
              matches={matches(c)}
              over={over}
              onOpen={() => setOpenId(c.id)}
            />
          ))}
        </div>
      )}
      {shown.length === 0 && (
        <div className="py-6 text-center text-fg-muted">
          {q ? `Nothing matches “${query.trim()}”` : "No servers"}
        </div>
      )}
      {errorLine}
    </>
  );
}
