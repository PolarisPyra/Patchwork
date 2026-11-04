import { useRef, useState, type ReactNode } from "react";
import { ArrowClockwiseIcon, CaretLeftIcon } from "@phosphor-icons/react";
import { api, AGENT_LABELS, type CliKind, type McpServerView } from "~/api";
import { Badge, INPUT } from "~/features/settings/controls";
import { tildify } from "~/lib/util";
import {
  type Card,
  type Client,
  DOT,
  onTools,
  ownConfig,
} from "~/features/settings/providers/mcp/servers";

export default function ServerDetail({
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
  // Each change is worked out from the saved list, so the next waits until it is read back.
  const saving = useRef(false);
  const setOff = (names: Set<string>) => {
    if (!own || saving.current) return;
    saving.current = true;
    void run(
      api.saveMcpServer(own.name, { ...ownConfig(own), disabledTools: [...names] }, own.name),
    ).finally(() => (saving.current = false));
  };
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
                      {tildify(u.file)}
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
                      aria-label={`Restart ${u.name} for ${u.client === "patchwork" ? "Patchwork" : AGENT_LABELS[u.client]}`}
                      onClick={() => void run(api.restartMcpServer(u.name))}
                    >
                      <ArrowClockwiseIcon size={15} />
                    </button>
                  )}
                  <button
                    className="btn"
                    aria-label={`Edit ${u.name}`}
                    onClick={() => onEdit(u.own!)}
                  >
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
                aria-label="Filter tools"
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
