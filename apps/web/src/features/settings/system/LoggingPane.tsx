import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, type LoggingSettings } from "~/api";
import LogLevelPicker from "~/components/LogLevelPicker";
import { TokenRow } from "~/features/settings/controls";
import type { PaneProps } from "~/features/settings/types";
import { errorText } from "~/lib/util";

/** The log files each launch writes, and what each one holds. */
const LOGS: { key: keyof LoggingSettings; title: string; desc: string }[] = [
  { key: "app", title: "Main", desc: "Patchwork's own messages, in main.log" },
  {
    key: "window",
    title: "Window",
    desc: "The window's errors and warnings, in window/renderer.log",
  },
  { key: "agents", title: "Agents", desc: "Each agent's runs, in agents/" },
  { key: "imports", title: "Imports", desc: "Each scan and import, in imports.log" },
];

/** The logs each launch writes to its own folder in the config folder's `logs`, as VS Code's,
 * which people can send with a bug report. */
export default function LoggingPane({ settings, updateSettings }: PaneProps) {
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  async function exportLogs() {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[-:]/g, "");
    const path = await save({
      defaultPath: `patchwork-logs-${stamp}.zip`,
      filters: [{ name: "Zip", extensions: ["zip"] }],
    });
    if (!path) return;
    setError(null);
    setExporting(true);
    try {
      await api.exportLogs(path);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setExporting(false);
    }
  }

  return (
    <>
      {LOGS.map((log) => (
        <TokenRow key={log.key} title={log.title} desc={log.desc}>
          <LogLevelPicker
            label={log.title}
            value={settings.logging[log.key]}
            onChange={(level) =>
              void updateSettings((s) => ({ ...s, logging: { ...s.logging, [log.key]: level } }))
            }
          />
        </TokenRow>
      ))}
      <TokenRow title="Log folder">
        <button className="btn" disabled={exporting} onClick={() => void exportLogs()}>
          {exporting ? "Exporting…" : "Export…"}
        </button>
        <button
          className="btn"
          onClick={() => api.openLogsFolder().catch((e) => setError(errorText(e)))}
        >
          Open
        </button>
      </TokenRow>
      {error && (
        <div role="alert" className="mt-2 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
    </>
  );
}
