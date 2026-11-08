import { useEffect, useState, type ReactNode } from "react";
import { MagnifyingGlassIcon, WarningIcon } from "@phosphor-icons/react";
import {
  api,
  apiProviderOf,
  onApiProvidersChanged,
  type AgentKind,
  type ProviderHealth,
} from "~/api";
import Spinner from "~/components/Spinner";
import { ComposerStripPresence } from "~/features/chat/ComposerStrip";
import { errorText } from "~/lib/util";

/** The most other models offered when the server stopped serving the chat's. */
const MAX_OFFERED = 3;

/**
 * Above the composer when the chat's local model cannot answer as asked: its server is not
 * answering, the server no longer serves the model, or the message has images the model does not
 * read. Checked when the model changes, when a reply ends, and when a provider is saved.
 */
export function ModelNotice({
  agent,
  model,
  images,
  visionSet,
  working,
  onPick,
  onChoose,
}: {
  agent: AgentKind;
  /** The chat's `provider/model`. */
  model: string | null;
  /** The message being written has images. */
  images: boolean;
  /** Settings → Providers has a model that reads images for every chat. */
  visionSet: boolean;
  working: boolean;
  onPick: (model: string) => void;
  /** Opens the model picker. */
  onChoose: () => void;
}) {
  const [checked, setChecked] = useState<{ model: string; health: ProviderHealth } | null>(null);
  const [saved, setSaved] = useState(0);
  const [finding, setFinding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => onApiProvidersChanged(() => setSaved((n) => n + 1)), []);

  useEffect(() => {
    if (agent !== "api" || !model || working) return;
    let cancelled = false;
    void (async () => {
      if (!apiProviderOf(model)) await api.listApiProviders().catch(() => []);
      const health = apiProviderOf(model)?.local
        ? await api.probeApiProvider(model.split("/")[0]).catch(() => null)
        : null;
      if (!cancelled) setChecked(health && { model, health });
    })();
    return () => {
      cancelled = true;
    };
  }, [agent, model, working, saved]);

  const health = agent === "api" && checked?.model === model ? checked.health : null;
  const providerId = model?.split("/")[0] ?? "";
  const modelId = model?.slice(providerId.length + 1) ?? "";
  const name = apiProviderOf(model)?.name ?? providerId;

  const find = async () => {
    setFinding(true);
    setError(null);
    try {
      await api.relocateApiProvider(providerId);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setFinding(false);
    }
  };

  let notice: { text: string; actions: ReactNode } | null = null;
  if (health && !health.reachable) {
    notice = {
      text: `${name} isn't answering`,
      actions: (
        <button className="btn flex-none" disabled={finding} onClick={() => void find()}>
          {finding ? <Spinner size={13} /> : <MagnifyingGlassIcon size={13} />}
          Find server
        </button>
      ),
    };
  } else if (health && !health.servedModels.includes(modelId)) {
    notice = {
      text: `${name} no longer serves ${modelId}`,
      actions: health.servedModels
        .filter((m) => !/embed/i.test(m))
        .slice(0, MAX_OFFERED)
        .map((m) => (
          <button key={m} className="btn flex-none" onClick={() => onPick(`${providerId}/${m}`)}>
            Use {m}
          </button>
        )),
    };
  } else if (health && images && health.images[modelId] === false && !visionSet) {
    notice = { text: `${modelId} doesn't read images`, actions: null };
  }

  return (
    <ComposerStripPresence present={notice !== null}>
      {notice && (
        <div
          role="status"
          className="mb-1.5 flex flex-wrap items-center gap-2 rounded-[var(--radius)] border border-border bg-panel px-2.5 py-1.5 text-[length:var(--fs-sm)]"
        >
          <WarningIcon size={14} className="flex-none text-warning" />
          <span className="min-w-0 flex-1 truncate text-fg">{notice.text}</span>
          {notice.actions}
          <button className="btn flex-none" onClick={onChoose}>
            Choose model
          </button>
          {error && (
            <span role="alert" className="basis-full text-danger">
              {error}
            </span>
          )}
        </div>
      )}
    </ComposerStripPresence>
  );
}
