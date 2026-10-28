import { useEffect, useState } from "react";
import { FolderSimpleIcon, PuzzlePieceIcon } from "@phosphor-icons/react";
import { api, type Plugin, type PluginHookHandler, type Settings } from "~/api";
import type { UpdateSettings } from "~/app/App";
import { home } from "~/components/settings/McpServersPane";
import { TokenRow, TokenSwitch } from "~/components/settings/SettingsDialog";
import { errorText } from "~/lib/util";

/**
 * Settings → Hooks: plugin bundles of hooks that run on turn events and the apps they ask for. A bundle
 * is a folder with a `plugin.json`; a Codex plugin bundle works as it is because the manifest is the same
 * shape.
 */
export default function PluginsPane({
  settings,
  updateSettings,
}: {
  settings: Settings;
  updateSettings: UpdateSettings;
}) {
  const [plugins, setPlugins] = useState<Plugin[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .listPlugins()
      .then(setPlugins)
      .catch((e: unknown) => setError(errorText(e)));
  }, []);

  function run(action: () => Promise<unknown>) {
    setError(null);
    action().catch((e: unknown) => setError(errorText(e)));
  }

  async function toggle(plugin: Plugin, on: boolean) {
    setPlugins(
      (list) => list?.map((p) => (p.id === plugin.id ? { ...p, enabled: on } : p)) ?? null,
    );
    await updateSettings((s) => ({
      ...s,
      disabledPlugins: on
        ? s.disabledPlugins.filter((id) => id !== plugin.id)
        : [...s.disabledPlugins.filter((id) => id !== plugin.id), plugin.id],
    }));
    setPlugins(await api.listPlugins());
  }

  const on = plugins?.filter((p) => p.enabled).length ?? 0;
  const hooks = plugins?.filter((p) => p.enabled).flatMap((p) => p.hooks).length ?? 0;

  return (
    <>
      <div className="flex items-center gap-1.5 pb-3">
        <span className="text-[length:var(--fs-sm)] text-fg-muted">
          {plugins === null ? "Reading plugins…" : `${on} on · ${hooks} hooks`}
        </span>
        <button
          className="btn ml-auto flex-none"
          onClick={() => run(async () => setPlugins(await api.listPlugins()))}
        >
          Refresh
        </button>
        <button className="btn flex-none" onClick={() => run(api.openPluginsFolder)}>
          <FolderSimpleIcon size={13} />
          Open folder
        </button>
      </div>

      {plugins && plugins.length > 0 && (
        <div className="flex flex-col gap-2">
          {plugins.map((plugin) => (
            <div
              key={plugin.id}
              className={`rounded-[var(--radius-md)] border border-border p-3 ${plugin.enabled ? "" : "opacity-60"}`}
            >
              <div className="flex min-w-0 items-start gap-2.5">
                <PuzzlePieceIcon size={18} className="mt-0.5 flex-none text-fg-muted" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    <span className="truncate font-semibold">{plugin.name}</span>
                    {plugin.version && (
                      <span className="text-[length:var(--fs-xs)] text-fg-subtle">
                        {plugin.version}
                      </span>
                    )}
                  </div>
                  {plugin.description && (
                    <div className="mt-0.5 text-[length:var(--fs-sm)] text-fg-muted">
                      {plugin.description}
                    </div>
                  )}
                  {plugin.hooks.length > 0 && (
                    <div className="mt-1.5 flex flex-col gap-0.5">
                      {plugin.hooks.map((hook, i) => (
                        <div
                          key={i}
                          className="flex min-w-0 items-baseline gap-1.5 text-[length:var(--fs-xs)]"
                        >
                          <span className="flex-none text-fg">{hook.event}</span>
                          <span className="flex-none text-fg-subtle">·</span>
                          <code
                            className="truncate text-fg-muted"
                            title={handlerText(hook.handler)}
                          >
                            {handlerText(hook.handler)}
                          </code>
                        </div>
                      ))}
                    </div>
                  )}
                  {plugin.apps.length > 0 && (
                    <div className="mt-1.5 flex flex-wrap gap-1">
                      {plugin.apps.map((app) => (
                        <span
                          key={app.name}
                          className="flex items-center gap-1 rounded-[var(--radius-sm)] bg-selected-muted px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-muted"
                          title={app.id}
                        >
                          {app.name}
                          <span className="text-fg-subtle">
                            {app.required ? "required" : "optional"}
                          </span>
                        </span>
                      ))}
                    </div>
                  )}
                </div>
                <TokenSwitch
                  label={`${plugin.name} is on`}
                  on={plugin.enabled}
                  onChange={(value) => run(() => toggle(plugin, value))}
                />
              </div>
            </div>
          ))}
        </div>
      )}

      {plugins && plugins.length === 0 && (
        <div className="py-6 text-center text-fg-muted">
          No plugins yet. A plugin is a folder with a <code>plugin.json</code>; drop one in the
          plugins folder.
        </div>
      )}

      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}

      <div className="mt-3">
        <TokenRow title="Folder" desc={settings.pluginsDir ?? home("")}>
          <button className="btn" onClick={() => run(api.openPluginsFolder)}>
            Open
          </button>
        </TokenRow>
      </div>
    </>
  );
}

/** A hook's handler as one line: the command it runs, or what it would do. */
function handlerText(handler: PluginHookHandler): string {
  switch (handler.type) {
    case "command":
      return handler.command;
    case "mcpTool":
      return `${handler.server} → ${handler.tool}`;
    case "prompt":
    case "agent":
      return `${handler.type}, not run yet`;
    case "unsupported":
      return handler.kind;
  }
}
