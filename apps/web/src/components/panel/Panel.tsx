import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { CaretDownIcon, FolderOpenIcon, PlusIcon, TrashIcon, XIcon } from "@phosphor-icons/react";
import { api, type Settings } from "~/api";
import type { UpdateSettings } from "~/app/App";
import { CHIP_CLASS } from "~/components/chat/ModelPicker";
import LogLevelPicker from "~/components/panel/LogLevelPicker";
import OutputView from "~/components/panel/OutputView";
import { sortChannels, type Channel } from "~/components/panel/logs";
import {
  applySettings,
  killShell,
  startShell,
  useShells,
  type Shell,
} from "~/components/panel/terminals";
import Dropdown from "~/components/workspace/Dropdown";
import { shortcutContext } from "~/lib/shortcuts";
import { errorText } from "~/lib/util";

export type PanelTab = "terminal" | "output";

interface PanelProps {
  tab: PanelTab;
  onTab: (tab: PanelTab) => void;
  onClose: () => void;
  settings: Settings;
  updateSettings: UpdateSettings;
  /** The folder new terminals start in: the open project's. */
  cwd: string | null;
  /** Bumped to start another terminal. */
  newTerminal: number;
  /** Bumped to move focus into the terminal. */
  focusTerminal: number;
}

/** The panel under the chat, as VS Code's: the integrated terminal and the Output view. */
export default function Panel({
  tab,
  onTab,
  onClose,
  settings,
  updateSettings,
  cwd,
  newTerminal,
  focusTerminal,
}: PanelProps) {
  const shells = useShells();
  const [activeId, setActiveId] = useState<number | null>(null);
  const active = shells.find((s) => s.id === activeId) ?? shells.at(-1) ?? null;
  const host = useRef<HTMLDivElement>(null);
  const starting = useRef(false);
  const [error, setError] = useState<string | null>(null);

  const [channels, setChannels] = useState<Channel[]>([]);
  const [channelId, setChannelId] = useState<string | null>(null);
  const channel = channels.find((c) => c.id === channelId) ?? channels[0] ?? null;

  const start = async () => {
    if (!host.current || starting.current) return;
    starting.current = true;
    setError(null);
    try {
      const shell = await startShell(host.current, cwd, settings.terminal);
      setActiveId(shell.id);
      shell.term.focus();
    } catch (e) {
      setError(errorText(e));
    } finally {
      starting.current = false;
    }
  };

  // The terminal view starts a shell when it opens with none, and on "New terminal".
  const asked = useRef(newTerminal);
  useEffect(() => {
    if (tab !== "terminal") return;
    if (shells.length === 0 || asked.current !== newTerminal) {
      asked.current = newTerminal;
      void start();
    }
    // `start` reads the latest props; only these say a shell is wanted.
  }, [tab, newTerminal]);

  // The last shell ending closes the panel, as closing the last terminal does in VS Code.
  const had = useRef(shells.length);
  useEffect(() => {
    if (had.current > 0 && shells.length === 0 && tab === "terminal") onClose();
    had.current = shells.length;
  }, [shells.length, tab, onClose]);

  // The active shell's element is shown, sized to the panel.
  useLayoutEffect(() => {
    const el = host.current;
    if (tab !== "terminal" || !el || !active) return;
    if (el.firstChild !== active.element) {
      el.replaceChildren(active.element);
      active.term.refresh(0, active.term.rows - 1);
    }
    active.fit.fit();
    const resized = new ResizeObserver(() => active.fit.fit());
    resized.observe(el);
    return () => resized.disconnect();
  }, [tab, active]);

  useEffect(() => {
    if (tab === "terminal") active?.term.focus();
    // Focus follows only the request, not every change of shell.
  }, [focusTerminal]);

  useEffect(() => applySettings(settings.terminal), [settings.terminal]);

  // The Output view lists this launch's logs, and finds new ones as they are written.
  useEffect(() => {
    if (tab !== "output") return;
    const list = () =>
      api.logChannels().then(
        (ids) => setChannels(sortChannels(ids)),
        () => {},
      );
    void list();
    const timer = setInterval(() => void list(), 2000);
    return () => clearInterval(timer);
  }, [tab]);

  const tabClass = (on: boolean) =>
    `h-[22px] cursor-pointer border-x-0 border-t-0 border-b-2 bg-transparent px-0 text-[length:var(--fs-sm)] font-medium ${on ? "border-accent text-fg" : "border-transparent text-fg-muted hover:text-fg"}`;

  return (
    <section
      className="flex h-full min-h-0 flex-col border-t border-border bg-editor"
      aria-label="Panel"
      {...shortcutContext("panelFocus")}
      tabIndex={-1}
    >
      <header className="flex h-8 flex-none items-center gap-3 px-3">
        <div className="flex h-full items-center gap-3" role="tablist">
          <button
            role="tab"
            aria-selected={tab === "terminal"}
            className={tabClass(tab === "terminal")}
            onClick={() => onTab("terminal")}
          >
            Terminal
          </button>
          <button
            role="tab"
            aria-selected={tab === "output"}
            className={tabClass(tab === "output")}
            onClick={() => onTab("output")}
          >
            Output
          </button>
        </div>
        <div className="ml-auto flex min-w-0 items-center gap-1">
          {tab === "terminal" ? (
            <>
              {shells.length > 1 &&
                shells.map((shell, i) => (
                  <ShellChip
                    key={shell.id}
                    shell={shell}
                    label={i === 0 ? shell.name : `${shell.name} ${i + 1}`}
                    active={shell === active}
                    onSelect={() => setActiveId(shell.id)}
                  />
                ))}
              <button
                className="btn-icon"
                aria-label="New terminal"
                title="New terminal"
                onClick={() => void start()}
              >
                <PlusIcon size={14} />
              </button>
              <button
                className="btn-icon"
                aria-label="Kill terminal"
                title="Kill terminal"
                disabled={!active}
                onClick={() => active && killShell(active)}
              >
                <TrashIcon size={14} />
              </button>
            </>
          ) : (
            <>
              <Dropdown
                alignEnd
                trigger={({ open, toggle }) => (
                  <button
                    className={CHIP_CLASS}
                    aria-haspopup="menu"
                    aria-expanded={open}
                    aria-label={`Log: ${channel?.name ?? "none"}`}
                    onClick={toggle}
                  >
                    <span>{channel?.name ?? "No logs yet"}</span>
                    <CaretDownIcon size={14} />
                  </button>
                )}
              >
                {(close) =>
                  channels.map((c) => (
                    <button
                      key={c.id}
                      role="menuitemradio"
                      aria-checked={c === channel}
                      title={c.id}
                      className={`flex w-full cursor-pointer items-center rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover ${c === channel ? "bg-selected-muted" : ""}`}
                      onClick={() => {
                        close();
                        setChannelId(c.id);
                      }}
                    >
                      {c.name}
                    </button>
                  ))
                }
              </Dropdown>
              {channel && (
                <LogLevelPicker
                  label={channel.name}
                  value={settings.logging[channel.setting]}
                  onChange={(level) =>
                    void updateSettings((s) => ({
                      ...s,
                      logging: { ...s.logging, [channel.setting]: level },
                    }))
                  }
                />
              )}
              <button
                className="btn-icon"
                aria-label="Open logs folder"
                title="Open logs folder"
                onClick={() => api.openLogsFolder().catch((e) => setError(errorText(e)))}
              >
                <FolderOpenIcon size={14} />
              </button>
            </>
          )}
          <button
            className="btn-icon"
            aria-label="Close panel"
            title="Close panel"
            onClick={onClose}
          >
            <XIcon size={14} />
          </button>
        </div>
      </header>
      {error && (
        <div className="flex-none px-3 pb-1 text-[length:var(--fs-sm)] text-danger">{error}</div>
      )}
      <div className="min-h-0 flex-1">
        {tab === "terminal" ? (
          <div ref={host} className="h-full w-full pl-3" {...shortcutContext("terminalFocus")} />
        ) : (
          <OutputView channel={channel?.id ?? null} />
        )}
      </div>
    </section>
  );
}

function ShellChip({
  shell,
  label,
  active,
  onSelect,
}: {
  shell: Shell;
  label: string;
  active: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      className={`h-6 cursor-pointer rounded-[var(--radius-sm)] border-none px-2 text-[length:var(--fs-sm)] ${active ? "bg-selected-muted text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
      title={shell.exitCode !== undefined ? `Exited with code ${shell.exitCode}` : undefined}
      onClick={onSelect}
    >
      {label}
    </button>
  );
}
