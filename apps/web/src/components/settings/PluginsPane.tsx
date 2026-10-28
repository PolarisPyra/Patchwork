import { useEffect, useState } from "react";
import { FolderSimpleIcon, PuzzlePieceIcon } from "@phosphor-icons/react";
import { api, type McpServer, type Plugin, type PluginMcpServer, type Settings } from "~/api";
import type { UpdateSettings } from "~/app/App";
import { Badge, home } from "~/components/settings/McpServersPane";
import { TokenRow, TokenSwitch } from "~/components/settings/SettingsDialog";
import { errorText } from "~/lib/util";

/**
 * Settings → Plugins: bundles of skills and MCP servers. A bundle is a folder with a
 * `plugin.json`; the two starter skills ship with the app, and a Codex plugin bundle works as it
 * is because the manifest is the same shape.
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
  const [added, setAdded] = useState<string[]>([]);

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
    // Turning a plugin on or off changes which skills the agents load.
    setPlugins(await api.listPlugins());
  }

  /** Adds a plugin's server to Patchwork's own, so API tasks can use it too. */
  async function addServer(plugin: Plugin, server: PluginMcpServer) {
    const config: McpServer = { command: server.command, args: server.args };
    await api.saveMcpServer(server.name, config);
    setAdded((list) => [...list, `${plugin.id}/${server.name}`]);
  }

  const on = plugins?.filter((p) => p.enabled).length ?? 0;
  const skills = plugins?.filter((p) => p.enabled).flatMap((p) => p.skills).length ?? 0;

  return (
    <>
      <div className="flex items-center gap-1.5 pb-3">
        <span className="text-[length:var(--fs-sm)] text-fg-muted">
          {plugins === null ? "Reading plugins…" : `${on} on · ${skills} skills loaded`}
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
                    {plugin.bundled && (
                      <span className="rounded-[var(--radius-sm)] bg-selected-muted px-1.5 text-[length:var(--fs-xs)] text-fg-muted">
                        bundled
                      </span>
                    )}
                  </div>
                  {plugin.description && (
                    <div className="mt-0.5 text-[length:var(--fs-sm)] text-fg-muted">
                      {plugin.description}
                    </div>
                  )}
                  {plugin.skills.length > 0 && (
                    <div className="mt-1.5 flex flex-wrap gap-1">
                      {plugin.skills.map((skill) => (
                        <button
                          key={skill.path}
                          className="flex cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-0 bg-selected-muted px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-muted hover:text-fg"
                          title={skill.description ?? skill.path}
                          onClick={() => run(() => api.openSkill(skill.path))}
                        >
                          <Badge name={skill.name} size={14} />${skill.name}
                        </button>
                      ))}
                    </div>
                  )}
                  {plugin.mcpServers.length > 0 && (
                    <div className="mt-1.5 flex flex-wrap items-center gap-1">
                      {plugin.mcpServers.map((server) => {
                        const key = `${plugin.id}/${server.name}`;
                        return (
                          <span
                            key={key}
                            className="flex items-center gap-1 rounded-[var(--radius-sm)] bg-selected-muted px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-muted"
                            title={`${server.command} ${server.args.join(" ")}`}
                          >
                            {server.name}
                            {added.includes(key) ? (
                              <span className="text-fg-subtle">added</span>
                            ) : (
                              <button
                                className="cursor-pointer border-0 bg-transparent p-0 text-accent-text hover:underline"
                                onClick={() => run(() => addServer(plugin, server))}
                              >
                                add to MCP
                              </button>
                            )}
                          </span>
                        );
                      })}
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
          No plugins yet. A plugin is a folder with a <code>plugin.json</code> and a{" "}
          <code>skills</code> folder; drop one in the plugins folder.
        </div>
      )}

      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}

      <div className="mt-3">
        <TokenRow
          title="Folder"
          desc={`${settings.pluginsDir ?? home("")} — a plugin's skills load beside the skills folder's own.`}
        >
          <button className="btn" onClick={() => run(api.openPluginsFolder)}>
            Open
          </button>
        </TokenRow>
      </div>
      <div className="mt-1 text-[length:var(--fs-sm)] text-fg-subtle">
        <code>plugin.json</code> takes a name, a version, a description, a <code>skills</code> list,
        and <code>mcpServers</code>. Codex's <code>.codex-plugin/plugin.json</code> works as it is.
      </div>
    </>
  );
}
