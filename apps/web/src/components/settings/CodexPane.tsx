import { useEffect, useState } from "react";
import { MagnifyingGlassIcon, StethoscopeIcon } from "@phosphor-icons/react";
import { api, type CodexInfo, type Settings } from "~/api";
import type { UpdateSettings } from "~/app/App";
import { INPUT, chip, home } from "~/components/settings/McpServersPane";
import { TokenRow } from "~/components/settings/SettingsDialog";
import { errorText } from "~/lib/util";

/**
 * Settings → Codex: what the installed Codex CLI offers. The feature flags and the model catalog
 * are read from the binary itself, so a newer Codex shows up as it is rather than as a copy of
 * a list from an older release.
 */
export default function CodexPane({
  settings,
}: {
  settings: Settings;
  updateSettings: UpdateSettings;
}) {
  const [info, setInfo] = useState<CodexInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [stableOnly, setStableOnly] = useState(false);
  const [output, setOutput] = useState<{ title: string; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .codexInfo()
      .then(setInfo)
      .catch((e: unknown) => setError(errorText(e)));
  }, []);

  function run(action: () => Promise<unknown>) {
    setError(null);
    setBusy(true);
    action()
      .catch((e: unknown) => setError(errorText(e)))
      .finally(() => setBusy(false));
  }

  function command(kind: "doctor" | "mcp" | "plugins", title: string) {
    setOutput(null);
    run(async () =>
      setOutput({
        title,
        text: (await api.codexCommand(kind)) || "Nothing to report.",
      }),
    );
  }

  const q = query.trim().toLowerCase();
  const features =
    info?.features.filter(
      (feature) =>
        (!stableOnly || feature.stage === "stable") &&
        (!q || `${feature.name} ${feature.stage}`.toLowerCase().includes(q)),
    ) ?? [];
  const enabled = info?.features.filter((feature) => feature.enabled).length ?? 0;

  return (
    <>
      {info === null && (
        <div className="py-2.5 text-fg-muted">{error ?? "Reading the Codex CLI…"}</div>
      )}
      {info && (
        <>
          <div className="flex flex-wrap items-center gap-1.5 pb-3">
            <span className="font-semibold">{info.version}</span>
            <span className="text-[length:var(--fs-sm)] text-fg-subtle">{home(info.path)}</span>
            <span className="ml-auto text-[length:var(--fs-sm)] text-fg-muted">
              {enabled} of {info.features.length} flags on
            </span>
          </div>

          <div className="flex flex-wrap items-center gap-1.5 pb-2">
            <button
              className={chip(stableOnly)}
              aria-pressed={stableOnly}
              onClick={() => setStableOnly(!stableOnly)}
            >
              Stable only
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
                spellCheck={false}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <span className="ml-auto flex gap-1.5">
              <button
                className="btn"
                disabled={busy}
                onClick={() => command("doctor", "codex doctor")}
              >
                <StethoscopeIcon size={13} />
                Doctor
              </button>
              <button
                className="btn"
                disabled={busy}
                onClick={() => command("mcp", "codex mcp list")}
              >
                MCP servers
              </button>
              <button
                className="btn"
                disabled={busy}
                onClick={() => command("plugins", "codex plugin list")}
              >
                Plugins
              </button>
            </span>
          </div>

          {output && (
            <div className="mb-3 overflow-hidden rounded-[var(--radius-md)] border border-border">
              <div className="flex items-center justify-between border-b border-border px-3 py-1 text-[length:var(--fs-xs)] text-fg-subtle">
                <span>{output.title}</span>
                <button
                  className="text-accent-text hover:underline"
                  onClick={() => setOutput(null)}
                >
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
                    <td className="px-2 py-1 text-right whitespace-nowrap text-fg-muted">
                      {feature.stage}
                    </td>
                    <td className="w-8 px-2 py-1 text-right">
                      {feature.enabled ? (
                        <span className="text-accent-text">on</span>
                      ) : (
                        <span className="text-fg-subtle">off</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {features.length === 0 && (
              <div className="px-3 py-4 text-center text-fg-muted">Nothing matches.</div>
            )}
          </div>

          {info.models.length > 0 && (
            <div className="mt-3 divide-y divide-border overflow-hidden rounded-[var(--radius-md)] border border-border">
              {info.models.map((model) => (
                <div key={model.slug} className="flex min-w-0 items-center gap-2 px-3 py-1.5">
                  <span className="min-w-0 flex-1 truncate font-semibold">{model.displayName}</span>
                  <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle">
                    {model.contextWindow ? `${Math.round(model.contextWindow / 1000)}k` : ""}
                  </span>
                  <span className="flex-none text-[length:var(--fs-xs)] text-fg-muted">
                    {model.efforts.join(" · ")}
                  </span>
                  {model.multiAgent && (
                    <span className="flex-none rounded-[var(--radius-sm)] bg-selected-muted px-1.5 text-[length:var(--fs-xs)] text-fg-muted">
                      {model.multiAgent}
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {error && info !== null && (
        <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>
      )}

      <div className="mt-3">
        <TokenRow
          title="The CLI in use"
          desc={
            settings.agentPaths.codex
              ? "The Codex CLI you located under Providers."
              : "Codex on PATH. Set a path under Providers to use another copy."
          }
        >
          <button className="btn" onClick={() => run(async () => setInfo(await api.codexInfo()))}>
            Read again
          </button>
        </TokenRow>
      </div>
    </>
  );
}
