import { useEffect, useState } from "react";
import { MagnifyingGlassIcon, StethoscopeIcon } from "@phosphor-icons/react";
import { api, type ClaudeInfo, type Settings } from "~/api";
import type { UpdateSettings } from "~/app/App";
import { INPUT, chip, home } from "~/components/settings/McpServersPane";
import { TokenRow } from "~/components/settings/SettingsDialog";
import { errorText } from "~/lib/util";

/**
 * Settings → Claude CLI: what the installed Claude Code offers. Claude Code prints neither its
 * feature gates nor its model catalog, so the pane shows the gates the CLI itself has cached in
 * `.claude.json` and the catalog it has fetched into `~/.claude/cache`, so a newer Claude Code
 * shows up as it is rather than as a copy of a list from an older release.
 */
export default function ClaudePane({
  settings,
}: {
  settings: Settings;
  updateSettings: UpdateSettings;
}) {
  const [info, setInfo] = useState<ClaudeInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [onOnly, setOnOnly] = useState(false);
  const [output, setOutput] = useState<{ title: string; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .claudeInfo()
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
        text: (await api.claudeCommand(kind)) || "Nothing to report.",
      }),
    );
  }

  const q = query.trim().toLowerCase();
  const all = info?.features ?? [];
  const features = all.filter(
    (feature) =>
      (!onOnly || feature.enabled === true) &&
      (!q || `${feature.name} ${feature.value}`.toLowerCase().includes(q)),
  );
  const enabled = all.filter((feature) => feature.enabled === true).length;

  return (
    <>
      {info === null && (
        <div className="py-2.5 text-fg-muted">{error ?? "Reading Claude Code…"}</div>
      )}
      {info && (
        <>
          <div className="flex flex-wrap items-center gap-1.5 pb-3">
            <span className="font-semibold">{info.version}</span>
            <span className="text-[length:var(--fs-sm)] text-fg-subtle">{home(info.path)}</span>
            <span className="ml-auto text-[length:var(--fs-sm)] text-fg-muted">
              {enabled} of {all.length} flags on
            </span>
          </div>

          <div className="flex flex-wrap items-center gap-1.5 pb-2">
            <button
              className={chip(onOnly)}
              aria-pressed={onOnly}
              onClick={() => setOnOnly(!onOnly)}
            >
              On only
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
                onClick={() => command("doctor", "claude doctor")}
              >
                <StethoscopeIcon size={13} />
                Doctor
              </button>
              <button
                className="btn"
                disabled={busy}
                onClick={() => command("mcp", "claude mcp list")}
              >
                MCP servers
              </button>
              <button
                className="btn"
                disabled={busy}
                onClick={() => command("plugins", "claude plugin list")}
              >
                Plugins
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
                      title={feature.value}
                    >
                      {feature.value}
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
                {all.length === 0
                  ? "Claude Code has not cached its feature gates yet. Start it once, then read again."
                  : "Nothing matches."}
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
                  {model.fastMode && (
                    <span className="flex-none rounded-[var(--radius-sm)] bg-selected-muted px-1.5 text-[length:var(--fs-xs)] text-fg-muted">
                      fast
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
            settings.agentPaths.claude
              ? "The Claude Code you located under Providers."
              : "Claude Code on PATH. Set a path under Providers to use another copy."
          }
        >
          <button className="btn" onClick={() => run(async () => setInfo(await api.claudeInfo()))}>
            Read again
          </button>
        </TokenRow>
      </div>
    </>
  );
}
