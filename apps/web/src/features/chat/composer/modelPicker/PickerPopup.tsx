import { useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { CaretRightIcon, MagnifyingGlassIcon } from "@phosphor-icons/react";
import { AGENT_LABELS, apiProviderName, type AgentKind, type ModelOption } from "~/api";
import { readFavorites, saveFavorites } from "~/features/chat/composer/modelPicker/favorites";
import ModelRow, { type ModelRowData } from "~/features/chat/composer/modelPicker/ModelRow";
import ProviderRail, {
  type ProviderFilter,
} from "~/features/chat/composer/modelPicker/ProviderRail";
import { usePopupPlacement } from "~/features/chat/composer/modelPicker/usePopupPlacement";

const rowKey = (kind: AgentKind, item: ModelOption) => `${kind}:${item.id ?? "default"}`;

/**
 * The model picker's panel: the providers on the left, a search field over the list of models,
 * and the Legacy models panel beside it.
 */
export default function PickerPopup({
  agent,
  model,
  agents,
  models,
  onPick,
  anchor,
  close,
}: {
  agent: AgentKind;
  model: string | null;
  agents: AgentKind[];
  models: Partial<Record<AgentKind, ModelOption[]>>;
  onPick: (agent: AgentKind, model: string | null) => void;
  anchor: HTMLButtonElement;
  close: () => void;
}) {
  const [favorites, setFavorites] = useState(readFavorites);
  // Open on Favorites only if a favorite is still in the model list.
  const [provider, setProvider] = useState<ProviderFilter>(() => {
    const hasFavorite = agents.some((kind) =>
      (models[kind] ?? []).some((m) => favorites.includes(rowKey(kind, m))),
    );
    return agents.length > 1 && hasFavorite ? "favorites" : agent;
  });
  // The provider whose Legacy models panel is open; the active legacy model's provider starts open.
  const [legacyOpen, setLegacyOpen] = useState<AgentKind | null>(() => {
    const active = (models[agent] ?? []).find((m) => m.id === model);
    return active?.isLegacy && !favorites.includes(`${agent}:${model}`) ? agent : null;
  });
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState(0);
  const panel = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const searching = query.trim().length > 0;
  const isLegacyRow = (item: ModelOption, key: string) => item.isLegacy && !favorites.includes(key);
  const rank = (item: ModelOption, key: string) =>
    favorites.includes(key) ? 2 : item.isLegacy ? 0 : 1;
  const rows: ModelRowData[] = agents
    .flatMap((kind) =>
      (models[kind] ?? []).map((item) => ({ kind, item, key: rowKey(kind, item) })),
    )
    .filter(({ kind, item, key }) =>
      searching
        ? `${item.label} ${item.defaultModel ?? ""} ${AGENT_LABELS[kind]} ${apiProviderName(item.id) ?? ""}`
            .toLowerCase()
            .includes(query.trim().toLowerCase())
        : provider === "favorites"
          ? favorites.includes(key)
          : provider === kind && !isLegacyRow(item, key),
    )
    // Favorites first, then current models; search results put legacy models last.
    .sort((a, b) => rank(b.item, b.key) - rank(a.item, a.key));
  const rowsKey = rows.map((row) => row.key).join("|");
  const legacyToggle = !searching && provider !== "favorites" ? provider : null;
  const legacyModels: ModelRowData[] = legacyToggle
    ? (models[legacyToggle] ?? [])
        .map((item) => ({ kind: legacyToggle, item, key: rowKey(legacyToggle, item) }))
        .filter(({ item, key }) => isLegacyRow(item, key))
    : [];
  const legacyPanelOpen = legacyToggle !== null && legacyOpen === legacyToggle;
  const toggleLegacy = (kind: AgentKind) => {
    setLegacyOpen((open) => (open === kind ? null : kind));
    input.current?.focus({ preventScroll: true });
  };
  const legacyToggleRow =
    legacyToggle && legacyModels.length > 0 ? (
      <button
        type="button"
        className="mb-0.5 flex min-h-11 w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-[4px] border-0 bg-transparent px-2 py-1 text-left text-fg hover:bg-hover focus-visible:bg-hover"
        aria-expanded={legacyPanelOpen}
        onClick={() => toggleLegacy(legacyToggle)}
      >
        <span className="flex min-w-0 flex-1 flex-col gap-1">
          <span className="text-[12px] leading-[16.5px] font-medium">Legacy models</span>
          <span className="text-[12px] leading-[16.5px] text-fg-subtle">{legacyModels.length}</span>
        </span>
        <CaretRightIcon size={14} className="flex-none text-fg-subtle" />
      </button>
    ) : null;

  const flyoutLeft = usePopupPlacement(anchor, panel, input, close);

  useLayoutEffect(() => {
    const current = rows.findIndex(({ kind, item }) => kind === agent && item.id === model);
    setHighlight(searching || current < 0 ? 0 : current);
  }, [rowsKey]);
  // A new provider or search starts at the top; expanding Legacy models keeps its place.
  useLayoutEffect(() => {
    if (list.current) list.current.scrollTop = 0;
  }, [provider, query]);

  const pick = ({ kind, item }: ModelRowData) => {
    onPick(kind, item.id);
    close();
  };
  const choose = (index: number) => {
    const row = rows[index];
    if (row) pick(row);
  };
  const selectProvider = (next: ProviderFilter) => {
    setProvider(next);
    input.current?.focus({ preventScroll: true });
  };
  const toggleFavorite = (key: string) => {
    const next = favorites.includes(key)
      ? favorites.filter((value) => value !== key)
      : [...favorites, key];
    setFavorites(next);
    saveFavorites(next);
  };
  const keyDown = (event: KeyboardEvent) => {
    event.stopPropagation();
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if ((event.ctrlKey || event.metaKey) && /^[1-9]$/.test(event.key)) {
      event.preventDefault();
      choose(Number(event.key) - 1);
    } else if (
      event.target === input.current &&
      (event.key === "ArrowDown" || event.key === "ArrowUp")
    ) {
      event.preventDefault();
      const next = rows.length
        ? (highlight + (event.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length
        : 0;
      setHighlight(next);
      list.current
        ?.querySelectorAll("[data-model-row]")
        [next]?.scrollIntoView({ block: "nearest" });
    } else if (event.target === input.current && event.key === "Enter") {
      event.preventDefault();
      choose(highlight);
    } else if (
      event.target === input.current &&
      !searching &&
      (event.key === "ArrowLeft" || (event.key === "Tab" && event.shiftKey))
    ) {
      event.preventDefault();
      panel.current?.querySelector<HTMLButtonElement>('[aria-pressed="true"]')?.focus();
    }
  };
  const renderRow = (row: ModelRowData, index: number | null) => (
    <ModelRow
      key={row.key}
      row={row}
      index={index}
      selected={row.kind === agent && row.item.id === model}
      highlighted={index !== null && highlight === index}
      favorite={favorites.includes(row.key)}
      onHighlight={() => index !== null && setHighlight(index)}
      onPick={() => (index === null ? pick(row) : choose(index))}
      onToggleFavorite={() => toggleFavorite(row.key)}
    />
  );
  return (
    <div ref={panel} className="fixed z-[1000] flex h-[346px]" onKeyDown={keyDown}>
      <div
        className="model-picker-panel flex h-full w-[360px] max-w-[calc(100vw_-_16px)] overflow-hidden rounded-[8px] border border-border-strong bg-elevated text-fg shadow-[var(--shadow-popup)]"
        role="dialog"
        aria-label="Choose model"
      >
        {!searching && (
          <ProviderRail
            agents={agents}
            provider={provider}
            onSelect={selectProvider}
            onMove={setProvider}
            onLeave={() => input.current?.focus()}
          />
        )}
        <div
          className={`flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden ${!searching ? "border-l border-border" : ""}`}
        >
          <label className="mx-3 mt-[9px] flex h-[34px] flex-none items-center gap-1 border-b border-border pb-1.5 text-fg-subtle focus-within:border-accent [&_svg]:flex-none">
            <MagnifyingGlassIcon size={16} />
            <input
              ref={input}
              role="combobox"
              aria-expanded="true"
              aria-controls="ws-model-options"
              aria-activedescendant={rows[highlight] ? `ws-model-option-${highlight}` : undefined}
              className="h-[26px] w-full min-w-0 border-0 bg-transparent text-fg text-[14px] placeholder:text-fg-subtle outline-none"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search models..."
              aria-label="Search models"
            />
          </label>
          <div
            ref={list}
            id="ws-model-options"
            className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain py-1.5 px-2 [scrollbar-gutter:stable]"
            role="listbox"
            aria-label="Models"
          >
            {rows.map((row, index) => renderRow(row, index))}
            {legacyToggleRow}
            {!rows.length && !legacyToggleRow && (
              <div className="p-2 text-center text-[14px] text-fg-subtle">No models found</div>
            )}
          </div>
        </div>
      </div>
      {legacyPanelOpen && (
        <div
          className={`model-picker-panel absolute top-0 flex h-full w-[300px] flex-col overflow-hidden rounded-[8px] border border-border-strong bg-elevated text-fg shadow-[var(--shadow-popup)] ${flyoutLeft ? "right-full mr-1.5" : "left-full ml-1.5"}`}
          role="listbox"
          aria-label="Legacy models"
        >
          <div className="px-3 pt-3 pb-1.5 text-[12px] font-medium text-fg-subtle">
            Legacy models
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-1.5">
            {legacyModels.map((row) => renderRow(row, null))}
          </div>
        </div>
      )}
    </div>
  );
}
