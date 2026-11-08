import { StarIcon } from "@phosphor-icons/react";
import { apiModelPlace, apiProviderName, isMac, type AgentKind, type ModelOption } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";

const PROVIDER_NAMES: Record<AgentKind, string> = {
  claude: "Claude",
  codex: "Codex",
  antigravity: "Antigravity",
  opencode: "OpenCode",
  api: "API",
};

/** A model of one agent, with its `agent:model` key. */
export interface ModelRowData {
  kind: AgentKind;
  item: ModelOption;
  key: string;
}

/**
 * A model in the picker, with a star that adds it to or removes it from the favorites. `index` is
 * the row's keyboard position in the list, or null in the Legacy models panel, whose rows take
 * focus themselves.
 */
export default function ModelRow({
  row: { kind, item },
  index,
  selected,
  highlighted,
  favorite,
  onHighlight,
  onPick,
  onToggleFavorite,
}: {
  row: ModelRowData;
  index: number | null;
  /** The thread's model. */
  selected: boolean;
  /** Under the list's keyboard highlight. */
  highlighted: boolean;
  favorite: boolean;
  onHighlight: () => void;
  onPick: () => void;
  onToggleFavorite: () => void;
}) {
  return (
    // The option is the row's text: an option's children are left out of the accessibility tree,
    // and the favorite button beside it has to stay in.
    <div
      data-model-row={index === null ? undefined : true}
      className={`mb-0.5 flex min-h-11 min-w-0 cursor-pointer items-center gap-1.5 rounded-[4px] px-2 py-1 hover:bg-hover ${highlighted ? "bg-hover" : ""}`}
      onMouseMove={index === null ? undefined : onHighlight}
      onClick={onPick}
      tabIndex={index === null ? 0 : undefined}
      onKeyDown={
        index === null
          ? (e) => {
              if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
              e.preventDefault();
              onPick();
            }
          : undefined
      }
    >
      <span
        id={index === null ? undefined : `ws-model-option-${index}`}
        role="option"
        aria-selected={selected}
        className="flex min-w-0 flex-1 flex-col gap-1"
      >
        <span className="overflow-hidden text-ellipsis whitespace-nowrap text-[12px] leading-[16.5px] font-medium">
          {item.label}
        </span>
        <span
          className="flex items-center gap-1.5 text-[12px] leading-[16.5px] text-fg-subtle [&_svg]:flex-none"
          title={
            item.id === null
              ? "Configured CLI default; session or project settings may override it"
              : undefined
          }
        >
          <ProviderLogos kind={kind} model={item.id ?? item.defaultModel} size={12} />
          <span className="overflow-hidden text-ellipsis whitespace-nowrap">
            {item.id === null
              ? (item.defaultModel ?? "CLI default")
              : kind === "api"
                ? [apiProviderName(item.id) ?? PROVIDER_NAMES[kind], apiModelPlace(item.id)]
                    .filter(Boolean)
                    .join(" · ")
                : PROVIDER_NAMES[kind]}
          </span>
        </span>
      </span>
      {index !== null && index < 9 && (
        <kbd className="flex-none rounded-[4px] bg-[color-mix(in_srgb,var(--text)_6%,transparent)] px-1 py-0.5 text-[12px] leading-4 text-fg-muted [font-family:inherit]">
          {isMac ? "Cmd" : "Ctrl"}+{index + 1}
        </kbd>
      )}
      <button
        className={`grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[4px] border-0 bg-transparent p-0 -mr-1 hover:bg-hover focus-visible:bg-hover ${favorite ? "text-accent-text" : "text-fg-subtle hover:text-fg focus-visible:text-fg"}`}
        aria-label={
          favorite ? `Remove ${item.label} from favorites` : `Add ${item.label} to favorites`
        }
        onKeyDown={(event) => event.stopPropagation()}
        onClick={(event) => {
          event.stopPropagation();
          onToggleFavorite();
        }}
      >
        <StarIcon size={12} fill={favorite ? "currentColor" : "none"} />
      </button>
    </div>
  );
}
