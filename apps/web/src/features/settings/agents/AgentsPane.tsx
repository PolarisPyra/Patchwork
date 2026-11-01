import { useState } from "react";
import { MagnifyingGlassIcon, PlusIcon, XIcon } from "@phosphor-icons/react";
import { api } from "~/api";
import { Badge, INPUT, chip, TokenRow } from "~/features/settings/controls";
import { useAgents } from "~/lib/agents";
import { useSigils } from "~/lib/sigils";
import { errorText } from "~/lib/util";

/** How the agents folder is laid out: one Markdown file per agent, named like the agent. */
const AGENTS_EXAMPLE = `agents/
├── dev-engineer.md
└── doc-scout.md

dev-engineer.md
---
name: DevEngineer
description: Senior developer: plans, implements features and fixes with explicit risk control
mode: primary
model: claude-sonnet-5-5
---
You are DevEngineer. Plan before you change anything…`;

/** The custom agents that "&name" summons, kept up to date as their folder changes. */
export default function AgentsPane() {
  const view = useAgents();
  const sigil = useSigils().agents;
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [primaryOnly, setPrimaryOnly] = useState(false);
  // The name being typed for a new agent, while its row is open.
  const [draft, setDraft] = useState<string | null>(null);

  const run = (action: Promise<void>) => {
    setError(null);
    action.catch((e) => setError(errorText(e)));
  };
  const create = (name: string) => {
    setError(null);
    api.createAgent(name).then(
      () => setDraft(null),
      (e) => setError(errorText(e)),
    );
  };
  const q = query.trim().toLowerCase();
  const primaries = view.agents.filter((a) => a.primary).length;
  const shown = view.agents.filter(
    (a) =>
      (!primaryOnly || a.primary) &&
      (!q || `${a.name} ${a.title ?? ""} ${a.description ?? ""}`.toLowerCase().includes(q)),
  );

  return (
    <>
      <div className="flex items-center gap-1.5 pb-3">
        <button
          className={chip(!primaryOnly)}
          aria-pressed={!primaryOnly}
          onClick={() => setPrimaryOnly(false)}
        >
          All <span className="text-fg-muted">{view.agents.length}</span>
        </button>
        <button
          className={chip(primaryOnly)}
          aria-pressed={primaryOnly}
          onClick={() => setPrimaryOnly(!primaryOnly)}
        >
          Primary <span className="text-fg-muted">{primaries}</span>
        </button>
        <label className="relative ml-auto w-52">
          <MagnifyingGlassIcon
            size={13}
            className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-fg-muted"
          />
          <input
            className={`${INPUT} pl-7`}
            value={query}
            placeholder="Search agents"
            aria-label="Search agents"
            spellCheck={false}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <button className="btn flex-none" onClick={() => setDraft("")}>
          <PlusIcon size={13} />
          New agent
        </button>
      </div>
      {(draft !== null || shown.length > 0) && (
        <div className="divide-y divide-border overflow-hidden rounded-[var(--radius-md)] border border-border">
          {draft !== null && (
            <form
              className="flex items-center gap-2.5 px-3 py-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (draft.trim()) create(draft.trim());
              }}
            >
              <Badge name={draft.trim() || "?"} size={24} />
              <input
                className={`${INPUT} flex-1 font-mono`}
                value={draft}
                placeholder="agent-name"
                aria-label="New agent's name"
                spellCheck={false}
                autoFocus
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key !== "Escape") return;
                  e.stopPropagation();
                  setDraft(null);
                }}
              />
              <button type="submit" className="btn flex-none" disabled={!draft.trim()}>
                Create
              </button>
              <button
                type="button"
                className="btn-icon flex-none"
                aria-label="Cancel"
                onClick={() => setDraft(null)}
              >
                <XIcon size={14} />
              </button>
            </form>
          )}
          {shown.map((agent) => (
            <div key={agent.path} className="flex min-w-0 items-center gap-2.5 px-3 py-2">
              <Badge name={agent.name} size={24} />
              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 items-center gap-1.5">
                  <span className="truncate font-semibold" title={agent.title ?? undefined}>
                    {sigil}
                    {agent.name}
                  </span>
                  {agent.primary && (
                    <span
                      className="flex-none rounded-[4px] bg-accent-soft px-1.5 text-[length:var(--fs-xs)] text-accent-text"
                      title="Stays the chat's agent once summoned"
                    >
                      Primary
                    </span>
                  )}
                </div>
                <div
                  className="truncate text-[length:var(--fs-sm)] text-fg-muted"
                  title={agent.description ?? undefined}
                >
                  {agent.description}
                </div>
              </div>
              {agent.model && (
                <span
                  className="max-w-[180px] flex-none truncate font-mono text-[length:var(--fs-xs)] text-fg-muted"
                  title={agent.model}
                >
                  {agent.model}
                </span>
              )}
              <button
                className="btn flex-none"
                aria-label={`Edit ${agent.name}`}
                onClick={() => run(api.openAgent(agent.path))}
              >
                Edit
              </button>
            </div>
          ))}
        </div>
      )}
      {draft === null && shown.length === 0 && (
        <div className="py-6 text-center text-fg-muted">
          {q
            ? `Nothing matches “${query.trim()}”`
            : primaryOnly
              ? "No primary agents"
              : "No agents"}
        </div>
      )}
      {error && (
        <div role="alert" className="mt-2 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
      <div className="mt-3">
        <TokenRow title="Folder" desc={view.dir}>
          {null}
        </TokenRow>
        <TokenRow title="In a project" desc=".patchwork/agents">
          {null}
        </TokenRow>
      </div>
      <div className="mt-5 mb-1.5 font-semibold">Example</div>
      <pre className="m-0 overflow-x-auto rounded-[var(--radius-sm)] border border-border-strong bg-editor px-3 py-2.5 font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] leading-relaxed text-fg-muted">
        {AGENTS_EXAMPLE}
      </pre>
    </>
  );
}
