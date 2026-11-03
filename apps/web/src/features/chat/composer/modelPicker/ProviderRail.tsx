import { useRef } from "react";
import { StarIcon } from "@phosphor-icons/react";
import { AGENT_LABELS, type AgentKind } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";

/** Which models the list shows: the favorites, or one agent's. */
export type ProviderFilter = AgentKind | "favorites";

const RAIL_BUTTON_CLASS =
  "relative grid h-9 w-9 flex-none cursor-pointer place-items-center rounded-[6px] border-0 bg-transparent p-0 text-fg hover:bg-hover focus-visible:bg-hover aria-pressed:after:absolute aria-pressed:after:top-2 aria-pressed:after:-right-1 aria-pressed:after:h-5 aria-pressed:after:w-[3px] aria-pressed:after:rounded-[3px_0_0_3px] aria-pressed:after:bg-accent aria-pressed:after:content-['']";

/**
 * The picker's column of providers, with Favorites on top. Up and Down move through it and show
 * that provider's models; Right goes back to the search field.
 */
export default function ProviderRail({
  agents,
  provider,
  onSelect,
  onMove,
  onLeave,
}: {
  agents: AgentKind[];
  provider: ProviderFilter;
  /** A provider was clicked. */
  onSelect: (provider: ProviderFilter) => void;
  /** The arrow keys moved to a provider. */
  onMove: (provider: ProviderFilter) => void;
  onLeave: () => void;
}) {
  const nav = useRef<HTMLElement>(null);
  return (
    <nav
      ref={nav}
      className="flex min-h-0 flex-[0_0_44px] flex-col gap-1 overflow-y-auto bg-[color-mix(in_srgb,var(--bg-app)_30%,transparent)] p-1 [scrollbar-width:none]"
      aria-label="Providers"
      onKeyDown={(event) => {
        const filters: ProviderFilter[] = ["favorites", ...agents];
        if (event.key === "ArrowRight") {
          event.preventDefault();
          onLeave();
        }
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const next =
            (filters.indexOf(provider) + (event.key === "ArrowDown" ? 1 : -1) + filters.length) %
            filters.length;
          onMove(filters[next]);
          nav.current?.querySelectorAll<HTMLButtonElement>("[data-model-rail-btn]")[next]?.focus();
        }
      }}
    >
      <button
        data-model-rail-btn
        className={RAIL_BUTTON_CLASS}
        aria-label="Favorites"
        title="Favorites"
        aria-pressed={provider === "favorites"}
        onClick={() => onSelect("favorites")}
      >
        <StarIcon size={20} weight="fill" />
      </button>
      <span className="h-px flex-none bg-border" />
      {agents.map((kind) => (
        <button
          key={kind}
          data-model-rail-btn
          className={RAIL_BUTTON_CLASS}
          title={AGENT_LABELS[kind]}
          aria-label={AGENT_LABELS[kind]}
          aria-pressed={provider === kind}
          onClick={() => onSelect(kind)}
        >
          <ProviderLogos kind={kind} size={20} />
        </button>
      ))}
    </nav>
  );
}
