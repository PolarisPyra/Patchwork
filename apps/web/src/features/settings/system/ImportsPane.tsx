import { useCallback, useEffect, useState } from "react";
import { api, importSourcesFor, SOURCE_LABELS, type ImportScan, type ImportSource } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import type { PaneProps } from "~/features/settings/types";
import { AGENT_KINDS, errorText } from "~/lib/util";

/** Conversations saved by the other agents Patchwork can read, and how much of each is left to
 * import. */
export default function ImportsPane({
  settings,
  updateSettings,
  onImported,
}: PaneProps & { onImported: () => void }) {
  const [scans, setScans] = useState<Partial<Record<ImportSource, ImportScan | null>>>({});
  const [busy, setBusy] = useState<ImportSource | null>(null);
  const [error, setError] = useState<string | null>(null);
  const sources = importSourcesFor(AGENT_KINDS.filter((k) => settings.agents.includes(k)));
  const key = sources.join(",");

  const scan = useCallback((kind: ImportSource) => {
    api.scanImports(kind).then(
      (s) => setScans((prev) => ({ ...prev, [kind]: s })),
      () => setScans((prev) => ({ ...prev, [kind]: null })),
    );
  }, []);

  // `key` stands in for `sources`, which is a new array on every render.
  useEffect(() => {
    for (const kind of sources) scan(kind);
  }, [key, scan]);

  const importFrom = async (kind: ImportSource) => {
    setBusy(kind);
    setError(null);
    try {
      await api.importThreads([kind]);
      // Importing adds projects on the backend; pick them up before saving settings again.
      await updateSettings(await api.getSettings());
      onImported();
      scan(kind);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      {sources.map((kind) => {
        const s = scans[kind];
        // T3 Code is only offered to people who use it.
        if (kind === "t3" && !(s && s.found > 0)) return null;
        const desc =
          s === undefined
            ? "Looking for saved conversations…"
            : s === null
              ? "Couldn't read saved conversations"
              : s.new > 0
                ? `${s.new} of ${s.found} conversations not imported yet`
                : s.found > 0
                  ? `All ${s.found} conversations imported`
                  : "No saved conversations";
        return (
          <div key={kind} className="flex items-center gap-3 border-b border-border py-2.5">
            <ProviderLogos kind={kind} size={20} />
            <div className="min-w-0 flex-1">
              <div className="font-semibold">{SOURCE_LABELS[kind]}</div>
              <div className="text-[length:var(--fs-sm)] text-fg-muted">{desc}</div>
              {s && s.subagents > 0 && (
                <div className="text-[length:var(--fs-sm)] text-fg-muted">
                  {s.subagents} sub-agent {s.subagents === 1 ? "conversation" : "conversations"}{" "}
                  hidden
                </div>
              )}
            </div>
            <button
              className="btn"
              disabled={busy !== null || !s || s.new === 0}
              aria-label={`Import from ${SOURCE_LABELS[kind]}`}
              onClick={() => void importFrom(kind)}
            >
              {busy === kind ? "Importing…" : "Import"}
            </button>
          </div>
        );
      })}
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}
