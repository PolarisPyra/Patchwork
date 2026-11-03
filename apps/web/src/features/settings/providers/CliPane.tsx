import { useEffect, useState } from "react";
import { MagnifyingGlassIcon, StethoscopeIcon } from "@phosphor-icons/react";
import { api } from "~/api";
import { INPUT, Segmented, chip } from "~/features/settings/controls";
import { errorText, tildify } from "~/lib/util";

/** The CLIs whose flags and model catalog Patchwork can read. */
type DetailsKind = "claude" | "codex";

/** Claude Code's and Codex's readings in one shape, so one view draws both. */
interface Details {
  path: string;
  version: string;
  /** `kept` is whether the filter chip keeps the flag. */
  features: { name: string; detail: string; enabled: boolean | null; kept: boolean }[];
  models: {
    id: string;
    name: string;
    contextWindow: number | null;
    efforts: string[];
    tag: string | null;
  }[];
}

/**
 * Claude Code prints neither its feature gates nor its model catalog, so its reading is the gates
 * the CLI itself has cached in `.claude.json` and the catalog it has fetched into
 * `~/.claude/cache`. Codex's flags and catalog are read from the binary itself. Either way a newer
 * CLI shows up as it is rather than as a copy of a list from an older release.
 */
const CLIS: Record<
  DetailsKind,
  {
    name: string;
    read: () => Promise<Details>;
    command: (kind: "doctor" | "mcp" | "plugins") => Promise<string>;
    filter: string;
    empty: string;
  }
> = {
  claude: {
    name: "claude",
    read: async () => {
      const info = await api.claudeInfo();
      return {
        ...info,
        features: info.features.map((f) => ({ ...f, detail: f.value, kept: f.enabled === true })),
        models: info.models.map((m) => ({ ...m, tag: m.fastMode ? "fast" : null })),
      };
    },
    command: api.claudeCommand,
    filter: "On only",
    empty: "Claude Code has not cached its feature gates yet. Start it once, then read again.",
  },
  codex: {
    name: "codex",
    read: async () => {
      const info = await api.codexInfo();
      return {
        ...info,
        features: info.features.map((f) => ({ ...f, detail: f.stage, kept: f.stage === "stable" })),
        models: info.models.map((m) => ({
          ...m,
          id: m.slug,
          name: m.displayName,
          tag: m.multiAgent,
        })),
      };
    },
    command: api.codexCommand,
    filter: "Stable only",
    empty: "Nothing matches.",
  },
};

/** What the installed CLI offers: its version, its flags, its models, and its own diagnostics. */
function CliDetails({ kind }: { kind: DetailsKind }) {
  const cli = CLIS[kind];
  const [info, setInfo] = useState<Details | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [filtered, setFiltered] = useState(false);
  const [output, setOutput] = useState<{ title: string; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    cli
      .read()
      .then(setInfo)
      .catch((e: unknown) => setError(errorText(e)));
  }, [cli]);

  function run(action: () => Promise<unknown>) {
    setError(null);
    setBusy(true);
    action()
      .catch((e: unknown) => setError(errorText(e)))
      .finally(() => setBusy(false));
  }

  function command(which: "doctor" | "mcp" | "plugins", title: string) {
    setOutput(null);
    run(async () =>
      setOutput({
        title: `${cli.name} ${title}`,
        text: (await cli.command(which)) || "Nothing to report.",
      }),
    );
  }

  const q = query.trim().toLowerCase();
  const all = info?.features ?? [];
  const features = all.filter(
    (feature) =>
      (!filtered || feature.kept) &&
      (!q || `${feature.name} ${feature.detail}`.toLowerCase().includes(q)),
  );
  const enabled = all.filter((feature) => feature.enabled === true).length;

  return (
    <>
      {info === null && (
        <div className="flex items-center gap-2 text-fg-muted">
          {error ?? "Reading…"}
          {error && (
            <button
              className="btn"
              disabled={busy}
              onClick={() => run(async () => setInfo(await cli.read()))}
            >
              Read again
            </button>
          )}
        </div>
      )}
      {info && (
        <>
          <div className="flex flex-wrap items-center gap-1.5 pb-3">
            <span className="font-semibold">{info.version}</span>
            <span className="text-[length:var(--fs-sm)] text-fg-subtle">{tildify(info.path)}</span>
            <span className="ml-auto text-[length:var(--fs-sm)] text-fg-muted">
              {enabled} of {all.length} flags on
            </span>
          </div>

          <div className="flex flex-wrap items-center gap-1.5 pb-2">
            <button
              className={chip(filtered)}
              aria-pressed={filtered}
              onClick={() => setFiltered(!filtered)}
            >
              {cli.filter}
            </button>
            <label className="relative w-52">
              <MagnifyingGlassIcon
                size={13}
                className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-fg-muted"
              />
              <input
                className={`${INPUT} pl-7`}
                value={query}
                placeholder="Search flags"
                aria-label="Search flags"
                spellCheck={false}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <span className="ml-auto flex gap-1.5">
              <button className="btn" disabled={busy} onClick={() => command("doctor", "doctor")}>
                <StethoscopeIcon size={13} />
                Doctor
              </button>
              <button className="btn" disabled={busy} onClick={() => command("mcp", "mcp list")}>
                MCP servers
              </button>
              <button
                className="btn"
                disabled={busy}
                onClick={() => command("plugins", "plugin list")}
              >
                Plugins
              </button>
              <button
                className="btn"
                disabled={busy}
                onClick={() => run(async () => setInfo(await cli.read()))}
              >
                Read again
              </button>
            </span>
          </div>

          {output && (
            <div className="mb-3 overflow-hidden rounded-[var(--radius-md)] border border-border">
              <div className="flex items-center justify-between gap-2 border-b border-border py-1 pr-1 pl-3 text-[length:var(--fs-xs)] text-fg-subtle">
                <span>{output.title}</span>
                <button className="btn" onClick={() => setOutput(null)}>
                  Close
                </button>
              </div>
              <pre className="m-0 max-h-[220px] overflow-auto px-3 py-2 font-[family-name:var(--font-mono)] text-[12px] leading-[1.5] whitespace-pre-wrap">
                {output.text}
              </pre>
            </div>
          )}

          <div className="max-h-[300px] overflow-auto rounded-[var(--radius-md)] border border-border">
            <table className="w-full border-collapse text-[length:var(--fs-sm)]">
              <tbody>
                {features.map((feature) => (
                  <tr key={feature.name} className="border-b border-border last:border-0">
                    <td className="px-2 py-1 font-[family-name:var(--font-mono)] text-[12px]">
                      {feature.name}
                    </td>
                    <td
                      className="max-w-[260px] truncate px-2 py-1 text-right font-[family-name:var(--font-mono)] text-[11px] text-fg-muted"
                      title={feature.detail}
                    >
                      {feature.detail}
                    </td>
                    <td className="w-8 px-2 py-1 text-right whitespace-nowrap">
                      {feature.enabled === true && <span className="text-accent-text">on</span>}
                      {feature.enabled === false && <span className="text-fg-subtle">off</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {features.length === 0 && (
              <div className="px-3 py-4 text-center text-fg-muted">
                {all.length === 0 ? cli.empty : "Nothing matches."}
              </div>
            )}
          </div>

          {info.models.length > 0 && (
            <div className="mt-3 divide-y divide-border overflow-hidden rounded-[var(--radius-md)] border border-border">
              {info.models.map((model) => (
                <div key={model.id} className="flex min-w-0 items-center gap-2 px-3 py-1.5">
                  <span className="min-w-0 flex-1 truncate font-semibold">{model.name}</span>
                  <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle">
                    {model.contextWindow ? `${Math.round(model.contextWindow / 1000)}k` : ""}
                  </span>
                  <span className="flex-none text-[length:var(--fs-xs)] text-fg-muted">
                    {model.efforts.join(" · ")}
                  </span>
                  {model.tag && (
                    <span className="flex-none rounded-[var(--radius-sm)] bg-selected-muted px-1.5 text-[length:var(--fs-xs)] text-fg-muted">
                      {model.tag}
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {error && info !== null && (
        <div role="alert" className="mt-2 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}

const KINDS: { value: DetailsKind; label: string }[] = [
  { value: "claude", label: "Claude Code" },
  { value: "codex", label: "Codex" },
];

/** Settings → CLIs: Claude Code's or Codex's reading, one at a time. */
export default function CliPane() {
  const [kind, setKind] = useState<DetailsKind>("claude");
  return (
    <>
      <div className="pb-3">
        <Segmented label="CLI" value={kind} options={KINDS} onChange={setKind} />
      </div>
      <CliDetails key={kind} kind={kind} />
    </>
  );
}
