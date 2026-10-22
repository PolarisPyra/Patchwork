import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { CaretDownIcon, MagnifyingGlassIcon, StarIcon } from "@phosphor-icons/react";
import { AGENT_LABELS, type AgentKind, type ModelOption } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";

interface Props {
  agent: AgentKind;
  model: string | null;
  agents: AgentKind[];
  models: Partial<Record<AgentKind, ModelOption[]>>;
  onPick: (agent: AgentKind, model: string | null) => void;
  openRequest?: number;
}
const PROVIDER_NAMES: Record<AgentKind, string> = {
  claude: "Claude",
  codex: "Codex",
  antigravity: "Antigravity",
};
export const CHIP_CLASS =
  "inline-flex h-7 cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0 text-[length:var(--fs-md)] font-medium text-fg-muted hover:bg-hover hover:text-fg [&_span]:max-w-[180px] [&_span]:overflow-hidden [&_span]:text-ellipsis [&_span]:whitespace-nowrap";
const favoriteKey = "patchwork-model-favorites";
/** Favorites saved before model ids came from the model manifest used CLI aliases. */
const LEGACY_FAVORITES: Record<string, string> = {
  "claude:fable": "claude:claude-fable-5-1",
  "claude:opus": "claude:claude-opus-5-5",
  "claude:sonnet": "claude:claude-sonnet-5-5",
  "claude:haiku": "claude:claude-haiku-5-5",
  "gemini:pro": "antigravity:gemini-3.1-pro-preview",
  "gemini:flash": "antigravity:gemini-3.8-flash",
  "gemini:flash-lite": "antigravity:gemini-3.5-flash-lite",
};
function readFavorites(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(favoriteKey) ?? "[]");
    if (!Array.isArray(value)) return [];
    const keys = value
      .filter((v): v is string => typeof v === "string")
      // Favorites saved under the agent's old "gemini" name move to "antigravity".
      .map((k) => LEGACY_FAVORITES[k] ?? k.replace(/^gemini:/, "antigravity:"));
    return [...new Set(keys)];
  } catch {
    return [];
  }
}

export default function ModelPicker(props: Props) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const handledOpenRequest = useRef(props.openRequest ?? 0);
  useEffect(() => {
    if (props.openRequest === handledOpenRequest.current) return;
    handledOpenRequest.current = props.openRequest ?? 0;
    if (props.openRequest) setOpen(true);
  }, [props.openRequest]);
  const label =
    props.models[props.agent]?.find((m) => m.id === props.model)?.label ?? props.model ?? "Default";
  const close = () => {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
  };
  return (
    <>
      <button
        ref={trigger}
        className={CHIP_CLASS}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`Choose model: ${label}`}
        onClick={() => setOpen(!open)}
      >
        <ProviderLogos kind={props.agent} size={16} />
        <span>{label}</span>
        <CaretDownIcon size={14} />
      </button>
      {open &&
        createPortal(
          <PickerPopup {...props} anchor={trigger.current!} close={close} />,
          document.body,
        )}
    </>
  );
}

function PickerPopup({
  agent,
  model,
  agents,
  models,
  onPick,
  anchor,
  close,
}: Props & { anchor: HTMLButtonElement; close: () => void }) {
  const [favorites, setFavorites] = useState(readFavorites);
  // Open on Favorites only if a favorite is still in the model list.
  const [provider, setProvider] = useState<AgentKind | "favorites">(() => {
    const hasFavorite = agents.some((kind) =>
      (models[kind] ?? []).some((m) => favorites.includes(`${kind}:${m.id ?? "default"}`)),
    );
    return agents.length > 1 && hasFavorite ? "favorites" : agent;
  });
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState(0);
  const panel = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const searching = query.trim().length > 0;
  const rows = agents
    .flatMap((kind) =>
      (models[kind] ?? []).map((item) => ({ kind, item, key: `${kind}:${item.id ?? "default"}` })),
    )
    .filter(({ kind, item, key }) =>
      searching
        ? `${item.label} ${item.defaultModel ?? ""} ${AGENT_LABELS[kind]}`
            .toLowerCase()
            .includes(query.trim().toLowerCase())
        : provider === "favorites"
          ? favorites.includes(key)
          : provider === kind,
    )
    .sort((a, b) => Number(favorites.includes(b.key)) - Number(favorites.includes(a.key)));
  const rowsKey = rows.map((row) => row.key).join("|");

  useLayoutEffect(() => {
    const position = () => {
      if (!panel.current) return;
      const rect = anchor.getBoundingClientRect();
      const above = rect.top - 12;
      const below = window.innerHeight - rect.bottom - 12;
      const up = above >= 346 || above >= below;
      Object.assign(panel.current.style, {
        left: `${Math.max(8, Math.min(rect.left, window.innerWidth - 368))}px`,
        top: up ? "auto" : `${rect.bottom + 4}px`,
        bottom: up ? `${window.innerHeight - rect.top + 4}px` : "auto",
        maxHeight: `${Math.max(0, Math.min(346, up ? above : below))}px`,
      });
    };
    position();
    input.current?.focus({ preventScroll: true });
    const observer = new ResizeObserver(position);
    observer.observe(anchor);
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
    };
  }, [anchor]);

  useLayoutEffect(() => {
    const current = rows.findIndex(({ kind, item }) => kind === agent && item.id === model);
    setHighlight(searching || current < 0 ? 0 : current);
    if (list.current) list.current.scrollTop = 0;
  }, [rowsKey]);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (
        event.target instanceof Node &&
        !panel.current?.contains(event.target) &&
        !anchor.contains(event.target)
      )
        close();
    };
    const wheel = (event: WheelEvent | TouchEvent) => {
      if (event.target instanceof Node && !panel.current?.contains(event.target))
        event.preventDefault();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("wheel", wheel, { passive: false });
    document.addEventListener("touchmove", wheel, { passive: false });
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("wheel", wheel);
      document.removeEventListener("touchmove", wheel);
    };
  }, [anchor, close]);

  const choose = (index: number) => {
    const row = rows[index];
    if (row) {
      onPick(row.kind, row.item.id);
      close();
    }
  };
  const selectProvider = (next: AgentKind | "favorites") => {
    setProvider(next);
    input.current?.focus({ preventScroll: true });
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
  return (
    <div
      ref={panel}
      className="fixed z-[1000] flex h-[346px] max-h-[346px] w-[360px] max-w-[calc(100vw_-_16px)] overflow-hidden rounded-[8px] border border-border-strong bg-elevated text-fg shadow-[var(--shadow-popup)]"
      role="dialog"
      aria-label="Choose model"
      onKeyDown={keyDown}
    >
      {!searching && (
        <nav
          className="flex min-h-0 flex-[0_0_44px] flex-col gap-1 overflow-y-auto bg-[color-mix(in_srgb,var(--bg-app)_30%,transparent)] p-1 [scrollbar-width:none]"
          aria-label="Providers"
          onKeyDown={(event) => {
            const filters: (AgentKind | "favorites")[] = ["favorites", ...agents];
            if (event.key === "ArrowRight") {
              event.preventDefault();
              input.current?.focus();
            }
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              const next =
                (filters.indexOf(provider) +
                  (event.key === "ArrowDown" ? 1 : -1) +
                  filters.length) %
                filters.length;
              setProvider(filters[next]);
              panel.current
                ?.querySelectorAll<HTMLButtonElement>("[data-model-rail-btn]")
                [next]?.focus();
            }
          }}
        >
          <button
            data-model-rail-btn
            className="relative grid h-9 w-9 flex-none cursor-pointer place-items-center rounded-[6px] border-0 bg-transparent p-0 text-fg hover:bg-hover focus-visible:bg-hover aria-pressed:after:absolute aria-pressed:after:top-2 aria-pressed:after:-right-1 aria-pressed:after:h-5 aria-pressed:after:w-[3px] aria-pressed:after:rounded-[3px_0_0_3px] aria-pressed:after:bg-accent aria-pressed:after:content-['']"
            aria-label="Favorites"
            title="Favorites"
            aria-pressed={provider === "favorites"}
            onClick={() => selectProvider("favorites")}
          >
            <StarIcon size={20} weight="fill" />
          </button>
          <span className="h-px flex-none bg-border" />
          {agents.map((kind) => (
            <button
              key={kind}
              data-model-rail-btn
              className="relative grid h-9 w-9 flex-none cursor-pointer place-items-center rounded-[6px] border-0 bg-transparent p-0 text-fg hover:bg-hover focus-visible:bg-hover aria-pressed:after:absolute aria-pressed:after:top-2 aria-pressed:after:-right-1 aria-pressed:after:h-5 aria-pressed:after:w-[3px] aria-pressed:after:rounded-[3px_0_0_3px] aria-pressed:after:bg-accent aria-pressed:after:content-['']"
              title={AGENT_LABELS[kind]}
              aria-label={AGENT_LABELS[kind]}
              aria-pressed={provider === kind}
              onClick={() => selectProvider(kind)}
            >
              <ProviderLogos kind={kind} size={20} />
            </button>
          ))}
        </nav>
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
          {rows.map(({ kind, item, key }, index) => (
            <div
              key={key}
              id={`ws-model-option-${index}`}
              role="option"
              aria-selected={kind === agent && item.id === model}
              data-model-row
              className={`mb-0.5 flex min-h-11 min-w-0 cursor-pointer items-center gap-1.5 rounded-[4px] px-2 py-1 ${highlight === index ? "bg-hover" : ""}`}
              onMouseMove={() => setHighlight(index)}
              onClick={() => choose(index)}
            >
              <span className="flex min-w-0 flex-1 flex-col gap-1">
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
                  <ProviderLogos kind={kind} size={12} />
                  <span className="overflow-hidden text-ellipsis whitespace-nowrap">
                    {item.id === null ? (item.defaultModel ?? "CLI default") : PROVIDER_NAMES[kind]}
                  </span>
                </span>
              </span>
              {index < 9 && (
                <kbd className="flex-none rounded-[4px] bg-[color-mix(in_srgb,var(--text)_6%,transparent)] px-1 py-0.5 text-[12px] leading-4 text-fg-muted [font-family:inherit]">
                  {navigator.platform.includes("Mac") ? "Cmd" : "Ctrl"}+{index + 1}
                </kbd>
              )}
              <button
                className={`grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[4px] border-0 bg-transparent p-0 -mr-1 hover:bg-hover focus-visible:bg-hover ${favorites.includes(key) ? "text-accent-text" : "text-fg-subtle hover:text-fg focus-visible:text-fg"}`}
                aria-label={
                  favorites.includes(key)
                    ? `Remove ${item.label} from favorites`
                    : `Add ${item.label} to favorites`
                }
                onKeyDown={(event) => event.stopPropagation()}
                onClick={(event) => {
                  event.stopPropagation();
                  const next = favorites.includes(key)
                    ? favorites.filter((value) => value !== key)
                    : [...favorites, key];
                  setFavorites(next);
                  try {
                    localStorage.setItem(favoriteKey, JSON.stringify(next));
                  } catch {
                    /* Favorites still work for this session. */
                  }
                }}
              >
                <StarIcon size={12} fill={favorites.includes(key) ? "currentColor" : "none"} />
              </button>
            </div>
          ))}
          {!rows.length && (
            <div className="p-2 text-center text-[14px] text-fg-subtle">No models found</div>
          )}
        </div>
      </div>
    </div>
  );
}
