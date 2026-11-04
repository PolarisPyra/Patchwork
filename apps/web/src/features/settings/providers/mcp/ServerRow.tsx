import { type ReactNode } from "react";
import { CaretRightIcon } from "@phosphor-icons/react";
import { AGENT_LABELS, type McpTool } from "~/api";
import { Badge } from "~/features/settings/controls";
import { type Card, type Client, DOT, onTools } from "~/features/settings/providers/mcp/servers";

export default function ServerRow({
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
      <span className="text-fg-muted">Not approved</span>
    ) : card.status === "signin" && card.tools.length === 0 ? (
      <span className="text-fg-muted">Signs in</span>
    ) : (
      <span className={warn ? "text-warning" : ""}>
        {on}
        <span className="ml-1 text-fg-muted">
          {on < card.tools.length ? `of ${card.tools.length} tools` : on === 1 ? "tool" : "tools"}
        </span>
      </span>
    );
  return (
    <button
      className={`flex w-full min-w-0 cursor-pointer items-center gap-2.5 border-x-0 border-t-0 border-b border-solid border-border bg-transparent last:border-b-0 px-3 py-2 text-left text-fg hover:bg-hover ${card.status === "off" || card.status === "unapproved" ? "opacity-55" : ""}`}
      title={card.status === "error" ? (card.uses.find((u) => u.error)?.error ?? "") : card.target}
      onClick={onOpen}
    >
      <Badge name={card.name} size={24} />
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="truncate font-semibold">{card.name}</span>
          <span className={`size-1.5 flex-none rounded-full ${DOT[card.status]}`} />
        </div>
        {matches.length > 0 ? (
          <div className="flex min-w-0 gap-1">
            {matches.slice(0, 3).map((t) => (
              <span
                key={t.name}
                className="min-w-0 truncate rounded-[var(--radius-sm)] bg-accent-soft px-1.5 font-mono text-[length:var(--fs-xs)]"
              >
                {t.name}
              </span>
            ))}
            {matches.length > 3 && (
              <span className="flex-none text-[length:var(--fs-xs)] text-fg-muted">
                +{matches.length - 3}
              </span>
            )}
          </div>
        ) : (
          <div className="truncate font-mono text-[length:var(--fs-xs)] text-fg-muted">
            {card.target}
          </div>
        )}
      </div>
      <span className="flex flex-none items-center gap-1">
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
      <span className="w-28 flex-none text-right text-[length:var(--fs-sm)] font-semibold">
        {tally}
      </span>
      <CaretRightIcon size={13} className="flex-none text-fg-muted" />
    </button>
  );
}
