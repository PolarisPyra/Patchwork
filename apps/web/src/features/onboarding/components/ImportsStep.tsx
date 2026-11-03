import { useEffect, useState } from "react";
import { CheckIcon } from "@phosphor-icons/react";
import Spinner from "~/components/Spinner";
import { api, SOURCE_LABELS, type ImportScan, type ImportSource } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import { cx } from "~/features/onboarding/classNames";

export type ImportScans = Partial<Record<ImportSource, ImportScan | null>>;

/** Scans each agent's saved conversations; `null` marks a failed scan. */
export function useImportScans(sources: ImportSource[], enabled: boolean) {
  const [scans, setScans] = useState<ImportScans>({});
  const key = sources.join(",");
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    setScans({});
    for (const kind of sources) {
      api.scanImports(kind).then(
        (scan) => !cancelled && setScans((s) => ({ ...s, [kind]: scan })),
        () => !cancelled && setScans((s) => ({ ...s, [kind]: null })),
      );
    }
    return () => {
      cancelled = true;
    };
    // `key` stands in for `sources`, which is a new array on every render.
  }, [key, enabled]);
  const scanning = sources.some((k) => scans[k] === undefined);
  return { scans, scanning };
}

/** The provider being imported now, and where it falls in the run. */
export interface ImportProgress {
  kind: ImportSource;
  index: number;
  total: number;
}

interface ImportsStepProps {
  sources: ImportSource[];
  scans: ImportScans;
  selected: ImportSource[];
  onToggle: (kind: ImportSource) => void;
  importing: ImportProgress | null;
  error: string | null;
}

export default function ImportsStep({
  sources,
  scans,
  selected,
  onToggle,
  importing,
  error,
}: ImportsStepProps) {
  return (
    <>
      <div className="flex items-center justify-between">
        <span className="text-[length:var(--fs-md)] font-semibold">
          Conversations found on this computer
        </span>
      </div>
      <div className="flex flex-col gap-2" role="group" aria-label="Providers to import from">
        {sources.map((kind) => {
          const scan = scans[kind];
          // T3 Code is only offered to people who use it.
          if (kind === "t3" && !(scan && scan.found > 0)) return null;
          const importable = (scan?.new ?? 0) > 0;
          const isSelected = importable && selected.includes(kind);
          return (
            <button
              key={kind}
              type="button"
              role="checkbox"
              aria-checked={isSelected}
              disabled={!importable || !!importing}
              className={cx(
                "flex w-full items-center gap-4 rounded-[var(--radius)] border p-4 text-left text-fg transition-[background-color,border-color] duration-[0.12s] ease-[ease] not-disabled:cursor-pointer",
                isSelected
                  ? "choice-selected"
                  : "border-border bg-transparent not-disabled:hover:bg-hover",
              )}
              onClick={() => onToggle(kind)}
            >
              <span className="inline-flex h-11 w-11 flex-none items-center justify-center rounded-[var(--radius)] bg-selected-muted text-[length:var(--fs-lg)] font-semibold text-accent-text">
                <ProviderLogos kind={kind} size={26} />
              </span>
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="text-[length:var(--fs-lg)] font-semibold">
                  {SOURCE_LABELS[kind]}
                </span>
                <span className="text-[length:var(--fs-sm)] text-fg-muted">
                  {describeScan(scan)}
                </span>
              </span>
              <span className="flex flex-none items-center gap-3">
                {scan === undefined && (
                  <span className="badge">
                    <Spinner />
                    Scanning…
                  </span>
                )}
                {importable && (
                  <span
                    className={cx(
                      "inline-flex h-[18px] w-[18px] flex-none items-center justify-center rounded-[var(--radius-sm)] border text-on-accent",
                      isSelected ? "border-accent bg-accent" : "border-border-strong",
                    )}
                  >
                    {isSelected && <CheckIcon size={12} weight="bold" />}
                  </span>
                )}
              </span>
            </button>
          );
        })}
      </div>
      {/* Always present, so screen readers announce each provider as it starts. */}
      <p
        role="status"
        className="m-0 flex items-center gap-2 text-[length:var(--fs-sm)] text-fg-muted"
      >
        {importing && (
          <>
            <Spinner />
            {describeProgress(importing, scans[importing.kind])}
          </>
        )}
      </p>
      {error && (
        <p role="alert" className="m-0 text-[length:var(--fs-sm)] text-danger">
          {error}
        </p>
      )}
    </>
  );
}

function describeProgress({ kind, index, total }: ImportProgress, scan?: ImportScan | null) {
  const count = scan?.new ? `${scan.new} ` : "";
  const of = total > 1 ? ` (${index + 1} of ${total})` : "";
  return `Importing ${count}${SOURCE_LABELS[kind]} conversations${of}…`;
}

function describeScan(scan: ImportScan | null | undefined): string {
  if (scan === undefined) return "Looking for saved conversations…";
  if (scan === null) return "Couldn't read saved conversations";
  if (scan.found === 0) return "No saved conversations";
  if (scan.new === 0) return `All ${scan.found} conversations are already imported`;
  const plural = scan.new === 1 ? "conversation" : "conversations";
  return `${scan.new} ${plural} to import, grouped into projects by folder`;
}
