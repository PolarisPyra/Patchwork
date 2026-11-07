import { useMemo, useRef, useState, type ReactNode } from "react";
import {
  ArrowClockwiseIcon,
  CaretDownIcon,
  CheckIcon,
  EyeIcon,
  EyeSlashIcon,
  ImageIcon,
  MagnifyingGlassIcon,
  PlusIcon,
  XIcon,
} from "@phosphor-icons/react";
import {
  api,
  type ApiModel,
  type ApiProvider,
  type ApiProviderView,
  type KeyChange,
  type Settings,
} from "~/api";
import type { UpdateSettings } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import Spinner from "~/components/Spinner";
import { errorText } from "~/lib/util";
import { INPUT } from "~/features/settings/controls";

/** Endpoints a provider can start from. */
interface Preset {
  id: string;
  name: string;
  baseUrl: string;
  models: string[];
}

const PRESETS: Preset[] = [
  {
    id: "deepseek",
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com",
    models: [],
  },
  { id: "custom", name: "Custom", baseUrl: "", models: [] },
];

export const blank = (preset: Preset): ApiProvider => ({
  id: "",
  name: preset.id === "custom" ? "" : preset.name,
  preset: preset.id,
  baseUrl: preset.baseUrl,
  enabled: true,
  models: preset.models.map((id) => ({ id, name: null })),
  headers: [],
  keyEnv: null,
  keyStore: null,
  logo: null,
  systemPrompt: "",
});

/** Puts the API agent in `settings.agents` while a provider can run a task, and takes it out
 * when none can, so the model picker offers API models exactly when there are some. */
export function syncAgents(providers: ApiProviderView[], updateSettings: UpdateSettings) {
  const usable = providers.some((p) => p.enabled && p.models.length > 0);
  void updateSettings((s: Settings) => {
    const has = s.agents.includes("api");
    if (usable === has) return s;
    if (usable) return { ...s, agents: [...s.agents, "api"] };
    return { ...s, agents: s.agents.filter((k) => k !== "api") };
  });
}

/** A saved provider in the Providers list: click or Edit to change it, Prompt for its system
 * prompt, the switch to use it. */
export function ApiProviderRow({
  provider: p,
  prompting,
  onEdit,
  onPrompt,
  onToggle,
}: {
  provider: ApiProviderView;
  prompting: boolean;
  onEdit: () => void;
  onPrompt: () => void;
  onToggle: () => void;
}) {
  return (
    <div className="flex items-center gap-3 border-b border-border py-2.5">
      <ProviderLogos kind="api" preset={p.preset} logo={p.logo} size={20} />
      <button
        className="min-w-0 flex-1 cursor-pointer border-none bg-transparent p-0 text-left text-fg"
        onClick={onEdit}
      >
        <div className="font-semibold">{p.name}</div>
        <div className="truncate text-[length:var(--fs-sm)] text-fg-muted">{describe(p)}</div>
      </button>
      <button className="btn" aria-label={`Edit ${p.name}`} onClick={onEdit}>
        Edit
      </button>
      <button
        className={`btn${p.systemPrompt ? " text-accent" : ""}`}
        aria-label={`${p.name} system prompt`}
        aria-expanded={prompting}
        onClick={onPrompt}
      >
        System Prompt
      </button>
      <button
        className="toggle"
        role="switch"
        aria-label={`Use ${p.name}`}
        aria-checked={p.enabled}
        onClick={onToggle}
      />
    </div>
  );
}

function describe(p: ApiProviderView): string {
  const models =
    p.models.length === 0
      ? "No models"
      : p.models.length <= 2
        ? p.models.map((m) => m.name ?? m.id).join(", ")
        : `${p.models.length} models`;
  const key = p.keyEnv
    ? `key from $${p.keyEnv}`
    : p.keyStore === "keychain"
      ? "key in system keychain"
      : p.keyStore === "file"
        ? "key in Patchwork's config folder"
        : "no key";
  return [models, key].filter(Boolean).join(" · ");
}

/** One labeled field of the editor. */
function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[length:var(--fs-sm)] font-medium text-fg-muted">{label}</span>
      {children}
    </label>
  );
}

export function PresetGrid({ onPick }: { onPick: (p: Preset) => void }) {
  return (
    <div className="grid grid-cols-2 gap-2 py-2.5">
      {PRESETS.map((p) => (
        <button
          key={p.id}
          className="flex h-16 cursor-pointer flex-col items-center justify-center gap-1.5 rounded-[var(--radius-md)] border border-border bg-transparent text-fg hover:border-border-strong hover:bg-hover"
          onClick={() => onPick(p)}
        >
          {p.id === "custom" ? (
            <PlusIcon size={20} />
          ) : (
            <ProviderLogos kind="api" preset={p.id} size={20} />
          )}
          <span className="text-[length:var(--fs-sm)]">{p.name}</span>
        </button>
      ))}
    </div>
  );
}

/** The size a chosen logo is scaled to fit, so the saved image stays small. */
const LOGO_PX = 128;

/** An image file scaled to fit `LOGO_PX` square, as a PNG `data:` URL. */
function logoDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      const w = img.naturalWidth || LOGO_PX;
      const h = img.naturalHeight || LOGO_PX;
      const scale = Math.min(1, LOGO_PX / Math.max(w, h));
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(w * scale));
      canvas.height = Math.max(1, Math.round(h * scale));
      canvas.getContext("2d")?.drawImage(img, 0, 0, canvas.width, canvas.height);
      resolve(canvas.toDataURL("image/png"));
    };
    img.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error("That file is not an image Patchwork can read"));
    };
    img.src = url;
  });
}

/** A custom provider's logo: click to choose an image, or clear it back to the generic mark. */
function LogoPicker({
  logo,
  onChange,
  onError,
}: {
  logo: string | null;
  onChange: (logo: string | null) => void;
  onError: (error: string) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  return (
    <div className="group relative flex-none">
      <button
        className={`flex size-8 cursor-pointer items-center justify-center rounded-[var(--radius-sm)] border border-dashed bg-transparent p-0 text-fg-muted hover:border-accent hover:text-fg ${logo ? "border-transparent" : "border-border-strong"}`}
        aria-label={logo ? "Change logo" : "Choose logo"}
        title={logo ? "Change logo" : "Choose logo"}
        onClick={() => input.current?.click()}
      >
        {logo ? (
          <ProviderLogos kind="api" preset="custom" logo={logo} size={20} />
        ) : (
          <ImageIcon size={16} />
        )}
      </button>
      {logo && (
        <button
          className="hover-reveal absolute -top-1.5 -right-1.5 flex size-4 cursor-pointer items-center justify-center rounded-full border border-border-strong bg-elevated p-0 text-fg-muted hover:text-fg"
          aria-label="Remove logo"
          title="Remove logo"
          onClick={() => onChange(null)}
        >
          <XIcon size={9} />
        </button>
      )}
      <input
        ref={input}
        type="file"
        accept="image/*"
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (file) logoDataUrl(file).then(onChange, (err) => onError(errorText(err)));
        }}
      />
    </div>
  );
}

export function ProviderForm({
  initial,
  hasKey,
  onDone,
}: {
  initial: ApiProvider;
  hasKey: boolean;
  onDone: () => void;
}) {
  const [draft, setDraft] = useState(initial);
  const saved = draft.id !== "";
  const [key, setKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  // The stored key, fetched when the eye first shows it. It is shown, not edited: Save keeps it.
  const [savedKey, setSavedKey] = useState<string | null>(null);
  const [clearKey, setClearKey] = useState(false);
  const [useEnv, setUseEnv] = useState(draft.keyEnv !== null);
  const [advanced, setAdvanced] = useState(draft.headers.length > 0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const set = (patch: Partial<ApiProvider>) => setDraft({ ...draft, ...patch });

  const toggleShowKey = async () => {
    const show = !showKey;
    setShowKey(show);
    if (!show || !hasKey || key || savedKey !== null) return;
    try {
      setSavedKey((await api.revealApiKey(draft.id)) ?? "");
    } catch (e) {
      setError(errorText(e));
    }
  };

  // An environment variable replaces a saved key, so the saved one is removed.
  const keyChange = (): KeyChange =>
    useEnv
      ? { action: draft.keyStore ? "clear" : "keep" }
      : clearKey
        ? { action: "clear" }
        : key.trim()
          ? { action: "set", key }
          : { action: "keep" };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const provider = { ...draft, keyEnv: useEnv ? (draft.keyEnv ?? "") : null };
      await api.saveApiProvider(provider, keyChange());
      onDone();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      await api.removeApiProvider(draft.id);
      onDone();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  const keyPlaceholder = clearKey
    ? "Key will be removed"
    : hasKey && draft.keyStore
      ? draft.keyStore === "keychain"
        ? "Saved in the system keychain"
        : "Saved in Patchwork's config folder"
      : "sk-…";

  return (
    <div className="flex flex-col gap-3 py-2.5">
      <div className="flex items-center gap-2">
        {draft.preset === "custom" ? (
          <LogoPicker logo={draft.logo} onChange={(logo) => set({ logo })} onError={setError} />
        ) : (
          <ProviderLogos kind="api" preset={draft.preset} size={20} />
        )}
        <span className="font-semibold">{draft.name || "New provider"}</span>
      </div>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Name">
          <input
            className={INPUT}
            value={draft.name}
            autoFocus={!draft.name}
            onChange={(e) => set({ name: e.target.value })}
          />
        </Field>
        <Field label="Base URL">
          <input
            className={INPUT}
            value={draft.baseUrl}
            placeholder="https://example.com/v1"
            spellCheck={false}
            onChange={(e) => set({ baseUrl: e.target.value })}
          />
        </Field>
      </div>
      <div className="flex flex-col gap-1">
        <div className="flex items-center justify-between">
          <span className="text-[length:var(--fs-sm)] font-medium text-fg-muted">
            {useEnv ? "Environment variable" : "API key"}
          </span>
          <button
            className="cursor-pointer border-none bg-transparent p-0 text-[length:var(--fs-sm)] text-accent hover:underline"
            onClick={() => setUseEnv(!useEnv)}
          >
            {useEnv ? "Enter a key instead" : "Use an environment variable"}
          </button>
        </div>
        {useEnv ? (
          <input
            className={`${INPUT} font-mono`}
            aria-label="API key environment variable"
            value={draft.keyEnv ?? ""}
            placeholder={`${(draft.preset === "custom" ? "API" : draft.preset).toUpperCase().replace(/-/g, "_")}_API_KEY`}
            spellCheck={false}
            onChange={(e) => set({ keyEnv: e.target.value })}
          />
        ) : (
          <div className="flex items-center gap-1">
            <input
              className={`${INPUT} font-mono`}
              type={showKey ? "text" : "password"}
              aria-label="API key"
              value={key || savedKey || ""}
              placeholder={keyPlaceholder}
              autoFocus={!!draft.name && !hasKey}
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => {
                setKey(e.target.value);
                setSavedKey(null);
                setClearKey(false);
              }}
            />
            <button
              className="btn-icon"
              title={showKey ? "Hide key" : "Show key"}
              aria-label="Show key"
              aria-pressed={showKey}
              onClick={toggleShowKey}
            >
              {showKey ? <EyeSlashIcon size={15} /> : <EyeIcon size={15} />}
            </button>
            {hasKey && draft.keyStore && !key && (
              <button
                className="btn-icon"
                title="Remove the saved key"
                aria-label="Remove the saved key"
                aria-pressed={clearKey}
                onClick={() => setClearKey(!clearKey)}
              >
                <XIcon size={15} />
              </button>
            )}
          </div>
        )}
      </div>
      <Models
        draft={draft}
        set={set}
        typedKey={useEnv ? null : key}
        keyEnv={useEnv ? draft.keyEnv : null}
      />
      <button
        className="flex cursor-pointer items-center gap-1 self-start border-none bg-transparent p-0 text-[length:var(--fs-sm)] text-fg-muted hover:text-fg"
        aria-expanded={advanced}
        onClick={() => setAdvanced(!advanced)}
      >
        <CaretDownIcon size={12} className={advanced ? "" : "-rotate-90"} />
        Headers
      </button>
      {advanced && <Headers draft={draft} set={set} />}
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
      <div className="flex items-center gap-2 border-t border-border pt-3">
        <button
          className="btn btn-primary"
          disabled={busy || !draft.name.trim() || !draft.baseUrl.trim()}
          onClick={() => void save()}
        >
          {busy ? "Saving…" : "Save"}
        </button>
        <button className="btn" disabled={busy} onClick={onDone}>
          Cancel
        </button>
        {saved && (
          <button
            className={`btn ml-auto ${confirmRemove ? "text-danger" : ""}`}
            disabled={busy}
            onClick={() => (confirmRemove ? void remove() : setConfirmRemove(true))}
          >
            {confirmRemove ? "Remove provider and key" : "Remove"}
          </button>
        )}
      </div>
    </div>
  );
}

/** The provider's models: chips to remove, a field to add one by id, and the endpoint's own
 * list to pick from. */
function Models({
  draft,
  set,
  typedKey,
  keyEnv,
}: {
  draft: ApiProvider;
  set: (patch: Partial<ApiProvider>) => void;
  typedKey: string | null;
  keyEnv: string | null;
}) {
  const [adding, setAdding] = useState("");
  const [remote, setRemote] = useState<ApiModel[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const chosen = new Set(draft.models.map((m) => m.id));

  const add = (id: string) => {
    const trimmed = id.trim();
    if (!trimmed || chosen.has(trimmed)) return;
    set({ models: [...draft.models, { id: trimmed, name: null }] });
  };
  const removeModel = (id: string) => set({ models: draft.models.filter((m) => m.id !== id) });

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await api.fetchApiModels({
        baseUrl: draft.baseUrl,
        headers: draft.headers,
        key: typedKey?.trim() || null,
        keyEnv: keyEnv?.trim() || null,
        providerId: draft.id || null,
      });
      setRemote(list);
    } catch (e) {
      setError(errorText(e));
      setRemote(null);
    } finally {
      setLoading(false);
    }
  };

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (remote ?? []).filter((m) => !q || m.id.toLowerCase().includes(q));
  }, [remote, query]);

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <span className="text-[length:var(--fs-sm)] font-medium text-fg-muted">Models</span>
        <button
          className="btn"
          disabled={loading || !draft.baseUrl.trim()}
          onClick={() => void load()}
        >
          {loading ? <Spinner size={13} /> : <ArrowClockwiseIcon size={13} />}
          {remote ? "Refresh list" : "Fetch models"}
        </button>
      </div>
      <div className="flex flex-wrap items-center gap-1.5 rounded-[var(--radius-sm)] border border-border-strong bg-editor p-1.5">
        {draft.models.map((m) => (
          <span
            key={m.id}
            className="inline-flex h-6 items-center gap-1 rounded-[var(--radius-sm)] bg-selected pr-0.5 pl-2 font-mono text-[length:var(--fs-sm)]"
          >
            {m.id}
            <button
              className="grid h-5 w-5 cursor-pointer place-items-center rounded-[var(--radius-sm)] border-none bg-transparent p-0 text-fg-muted hover:bg-hover hover:text-fg"
              aria-label={`Remove ${m.id}`}
              onClick={() => removeModel(m.id)}
            >
              <XIcon size={11} />
            </button>
          </span>
        ))}
        <input
          className="h-6 min-w-[140px] flex-1 border-none bg-transparent px-1 font-mono text-[length:var(--fs-sm)] text-fg outline-none"
          value={adding}
          aria-label="Add model id"
          placeholder={draft.models.length === 0 ? "Add a model id, then Enter" : "Add a model"}
          spellCheck={false}
          onChange={(e) => setAdding(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === ",") {
              e.preventDefault();
              add(adding);
              setAdding("");
            } else if (e.key === "Backspace" && !adding && draft.models.length > 0) {
              removeModel(draft.models[draft.models.length - 1].id);
            }
          }}
          onBlur={() => {
            add(adding);
            setAdding("");
          }}
        />
      </div>
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
      {remote && (
        <div className="flex flex-col overflow-hidden rounded-[var(--radius-sm)] border border-border">
          {remote.length > 8 && (
            <label className="flex h-8 items-center gap-1.5 border-b border-border px-2 text-fg-subtle">
              <MagnifyingGlassIcon size={14} />
              <input
                className="h-full flex-1 border-none bg-transparent text-[length:var(--fs-sm)] text-fg outline-none"
                value={query}
                placeholder={`Search ${remote.length} models`}
                aria-label="Search models"
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
          )}
          <div className="max-h-[180px] overflow-y-auto">
            {shown.length === 0 && (
              <div className="px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted">No models</div>
            )}
            {shown.map((m) => {
              const on = chosen.has(m.id);
              return (
                <button
                  key={m.id}
                  type="button"
                  role="checkbox"
                  aria-checked={on}
                  className="flex w-full cursor-pointer items-center gap-2 border-none bg-transparent px-2 py-1 text-left font-mono text-[length:var(--fs-sm)] text-fg hover:bg-hover"
                  onClick={() => (on ? removeModel(m.id) : add(m.id))}
                >
                  <span
                    className={`inline-flex size-4 flex-none items-center justify-center rounded-[var(--radius-sm)] border text-on-accent ${on ? "border-accent bg-accent" : "border-border-strong"}`}
                  >
                    {on && <CheckIcon size={11} weight="bold" />}
                  </span>
                  <span className="min-w-0 truncate">{m.id}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

function Headers({
  draft,
  set,
}: {
  draft: ApiProvider;
  set: (patch: Partial<ApiProvider>) => void;
}) {
  const update = (i: number, patch: Partial<{ name: string; value: string }>) =>
    set({ headers: draft.headers.map((h, j) => (j === i ? { ...h, ...patch } : h)) });
  return (
    <div className="flex flex-col gap-1.5">
      {draft.headers.map((h, i) => (
        <div key={i} className="flex items-center gap-1.5">
          <input
            className={`${INPUT} font-mono`}
            value={h.name}
            placeholder="Header"
            aria-label={`Header ${i + 1} name`}
            spellCheck={false}
            onChange={(e) => update(i, { name: e.target.value })}
          />
          <input
            className={`${INPUT} font-mono`}
            value={h.value}
            placeholder="Value"
            aria-label={`Header ${i + 1} value`}
            spellCheck={false}
            onChange={(e) => update(i, { value: e.target.value })}
          />
          <button
            className="btn-icon"
            aria-label={`Remove header ${h.name || i + 1}`}
            onClick={() => set({ headers: draft.headers.filter((_, j) => j !== i) })}
          >
            <XIcon size={14} />
          </button>
        </div>
      ))}
      <button
        className="btn self-start"
        onClick={() => set({ headers: [...draft.headers, { name: "", value: "" }] })}
      >
        <PlusIcon size={13} />
        Add header
      </button>
    </div>
  );
}
