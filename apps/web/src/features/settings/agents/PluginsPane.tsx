import { useEffect, useState } from "react";
import { CaretRightIcon, FolderSimpleIcon, PuzzlePieceIcon } from "@phosphor-icons/react";
import {
  AGENT_LABELS,
  api,
  type Plugin,
  type PluginHook,
  type PluginHookHandler,
  type Settings,
} from "~/api";
import type { UpdateSettings } from "~/api";
import { TokenRow, TokenSwitch } from "~/features/settings/controls";
import { AGENT_KINDS, errorText } from "~/lib/util";

/**
 * Settings → Hooks: plugin bundles of hooks that run on turn events and the apps they ask for. A bundle
 * is a folder with a `hooks.json`, or a `plugin.json` when it carries more; a Codex plugin bundle works
 * as it is because the manifest is the same shape.
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
          {plugins === null
            ? "Reading plugins…"
            : `${on} on · ${hooks} ${hooks === 1 ? "hook" : "hooks"}`}
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
                    <div className="mt-2 flex flex-col divide-y divide-border rounded-[var(--radius-sm)] border border-border">
                      {plugin.hooks.map((hook, i) => (
                        <HookRow key={i} hook={hook} />
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
          No plugins yet. A plugin is a folder with a <code>hooks.json</code>; drop one in the
          plugins folder.
        </div>
      )}

      {error && (
        <div role="alert" className="mt-2 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}

      <div className="mt-3">
        <TokenRow title="Folder" desc={settings.pluginsDir ?? ""}>
          <button className="btn" onClick={() => run(api.openPluginsFolder)}>
            Open
          </button>
        </TokenRow>
      </div>
    </>
  );
}

/** One hook: when it runs and what it does, opening to the event, matcher, and handler as written. */
function HookRow({ hook }: { hook: PluginHook }) {
  const [open, setOpen] = useState(false);
  const shell = isShell(hook.matcher);
  const skipped = AGENT_KINDS.filter((agent) => !hook.runBy.includes(agent));
  const { handler } = hook;
  return (
    <div className="text-[length:var(--fs-xs)]">
      <button
        className="flex w-full min-w-0 cursor-pointer flex-col gap-1 border-none bg-transparent px-2.5 py-2 text-left hover:bg-selected-muted"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <div className="flex w-full min-w-0 items-center gap-1.5">
          <span className="font-semibold text-fg">{whenText(hook.event, shell)}</span>
          {hook.matcher && !shell && (
            <code className="truncate rounded-[var(--radius-sm)] bg-selected-muted px-1.5 py-0.5 text-fg-muted">
              {hook.matcher}
            </code>
          )}
          {skipped.length > 0 && (
            <span className="ml-auto flex-none text-fg-subtle">
              {hook.runBy.length === 0
                ? "Not run"
                : `Not run by ${skipped.map((agent) => AGENT_LABELS[agent]).join(", ")}`}
            </span>
          )}
          <CaretRightIcon
            size={12}
            className={`flex-none text-fg-subtle ${skipped.length > 0 ? "" : "ml-auto"} ${open ? "rotate-90" : ""}`}
          />
        </div>
        {handler.type === "command" ? (
          <code className="w-full truncate text-fg-muted">{handler.command}</code>
        ) : (
          <span className="text-fg-muted">{handlerText(handler)}</span>
        )}
      </button>
      {open && (
        <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 px-2.5 pb-2.5 text-fg-muted">
          <dt className="text-fg-subtle">Event</dt>
          <dd className="m-0">
            <code>{hook.event}</code>
          </dd>
          {hook.matcher && (
            <>
              <dt className="text-fg-subtle">Tools</dt>
              <dd className="m-0 break-all">
                <code>{hook.matcher}</code>
              </dd>
            </>
          )}
          {handler.type === "command" && (
            <>
              <dt className="text-fg-subtle">Command</dt>
              <dd className="m-0">
                <pre className="m-0 whitespace-pre-wrap break-all rounded-[var(--radius-sm)] bg-selected-muted px-2 py-1.5 font-mono text-fg select-text">
                  {handler.command}
                </pre>
              </dd>
            </>
          )}
          {handler.type === "builtin" && (
            <>
              <dt className="text-fg-subtle">Built-in</dt>
              <dd className="m-0">
                <code>{handler.name}</code>
              </dd>
            </>
          )}
          {(handler.type === "command" || handler.type === "builtin") && (
            <>
              <dt className="text-fg-subtle">Timeout</dt>
              <dd className="m-0">{handler.timeoutSecs}s</dd>
            </>
          )}
        </dl>
      )}
    </div>
  );
}

/** The shell tool's name in each agent, as `provider::hooks::SHELL_TOOLS` lists them. */
const SHELL_TOOLS = new Set(["Bash", "bash", "run_command", "execute_command", "shell"]);

/** Whether a matcher names only shell tools. */
function isShell(matcher: string | null): boolean {
  return matcher !== null && matcher.split("|").every((tool) => SHELL_TOOLS.has(tool));
}

/** When a hook runs, in words. */
function whenText(event: PluginHook["event"], shell: boolean): string {
  const calls = shell ? "shell commands" : "tool calls";
  switch (event) {
    case "userPromptSubmit":
      return "When a prompt is sent";
    case "sessionStart":
      return "When a session starts";
    case "preToolUse":
      return `Before ${calls}`;
    case "permissionRequest":
      return shell ? "When a shell command asks permission" : "When a tool asks permission";
    case "postToolUse":
      return `After ${calls}`;
    case "preCompact":
      return "Before compacting";
    case "postCompact":
      return "After compacting";
    case "subagentStart":
      return "When a subagent starts";
    case "subagentStop":
      return "When a subagent stops";
    case "stop":
      return "After each turn";
    case "interrupt":
      return "When a turn is interrupted";
    case "sessionEnd":
      return "When a session ends";
  }
}

/** What a hook that runs no command does, in words. */
function handlerText(handler: Exclude<PluginHookHandler, { type: "command" }>): string {
  switch (handler.type) {
    case "builtin":
      return handler.description ?? `Unknown built-in: ${handler.name}`;
    case "mcpTool":
      return `${handler.server} → ${handler.tool}`;
    case "prompt":
    case "agent":
      return `A ${handler.type} hook`;
    case "unsupported":
      return `A ${handler.kind} hook`;
  }
}
