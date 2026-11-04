import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import {
  CaretDownIcon,
  FolderOpenIcon,
  PlusIcon,
  TerminalWindowIcon,
  TrashIcon,
  XIcon,
} from "@phosphor-icons/react";
import { api, type Settings, type TerminalShell } from "~/api";
import type { UpdateSettings } from "~/api";
import { CHIP_CLASS } from "~/components/chip";
import LogLevelPicker from "~/components/LogLevelPicker";
import OutputView from "~/features/panel/OutputView";
import ScreenView from "~/features/panel/ScreenView";
import { sortChannels, type Channel } from "~/features/panel/logs";
import {
  applySettings,
  killShell,
  openShells,
  restoreShells,
  showShell,
  startShell,
  useShells,
  type Shell,
} from "~/features/panel/terminals";
import ContextMenu from "~/components/ContextMenu";
import Dropdown from "~/components/Dropdown";
import { shortcutContext } from "~/lib/shortcuts";
import { errorText } from "~/lib/util";

export type PanelTab = "terminal" | "output" | "screen";

/** Where the shell last picked for new terminals is kept. */
const SHELL_KEY = "patchwork-terminal-shell";

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

/** The panel under the chat: the integrated terminal and the Output view. */
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
  const [menu, setMenu] = useState<{ x: number; y: number; shell: Shell } | null>(null);

  const [channels, setChannels] = useState<Channel[]>([]);
  const [channelId, setChannelId] = useState<string | null>(null);
  const channel = channels.find((c) => c.id === channelId) ?? channels[0] ?? null;

  // The shells a terminal can start, the user's first, and the one last picked.
  const [shellChoices, setShellChoices] = useState<TerminalShell[]>([]);
  const [picked, setPicked] = useState(() => localStorage.getItem(SHELL_KEY));
  const findShells = () =>
    api.terminalShells().then(
      (found) => {
        setShellChoices(found);
        return found;
      },
      () => [],
    );
  useEffect(() => {
    if (tab === "terminal") void findShells();
  }, [tab]);

  // A "New terminal" asked for while a shell starts, opened once that one is up.
  const startNext = useRef<{ shell?: string } | null>(null);
  const start = async (asked: boolean, chosen?: string) => {
    if (!host.current) return;
    if (starting.current) {
      if (asked) startNext.current = { shell: chosen };
      return;
    }
    starting.current = true;
    setError(null);
    try {
      // The shell last picked while it is still installed, else the user's.
      const saved = localStorage.getItem(SHELL_KEY);
      const found = chosen ? [] : await findShells();
      const path = chosen ?? (found.some((s) => s.path === saved) ? saved : null);
      const shell = await startShell(host.current, cwd, path, settings.terminal);
      setActiveId(shell.id);
      shell.term.focus();
    } catch (e) {
      setError(errorText(e));
    } finally {
      starting.current = false;
      const next = startNext.current;
      if (next) {
        startNext.current = null;
        void start(true, next.shell);
      }
    }
  };

  const pickShell = (path: string) => {
    localStorage.setItem(SHELL_KEY, path);
    setPicked(path);
    void start(true, path);
  };
  const pickedShell = shellChoices.find((s) => s.path === picked) ?? shellChoices[0];

  // The terminal view starts a shell when it opens with none, once the ones the app already has
  // are back, and on "New terminal".
  const asked = useRef(newTerminal);
  useEffect(() => {
    if (tab !== "terminal") return;
    const again = asked.current !== newTerminal;
    asked.current = newTerminal;
    void restoreShells(settings.terminal).then(() => {
      if (again || openShells().length === 0) void start(again);
    });
    // `start` reads the latest props; only these say a shell is wanted.
  }, [tab, newTerminal]);

  // The last shell ending closes the panel.
  const had = useRef(shells.length);
  useEffect(() => {
    if (had.current > 0 && shells.length === 0 && tab === "terminal") onClose();
    had.current = shells.length;
  }, [shells.length, tab, onClose]);

  // The active shell's element is shown, sized to the panel.
  useLayoutEffect(() => {
    const el = host.current;
    if (tab !== "terminal" || !el || !active) return;
    if (el.firstChild !== active.element || !active.opened) showShell(active, el);
    active.fit.fit();
    // Fit once the size settles: each fit rebuilds every row and rewrites xterm's stylesheet,
    // which restyles the whole app, so fitting on every frame of a window drag pins the CPU.
    // Until then the host clips the terminal at its old size.
    let timer = 0;
    const resized = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = window.setTimeout(() => active.fit.fit(), 100);
    });
    resized.observe(el);
    return () => {
      clearTimeout(timer);
      resized.disconnect();
    };
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
        <div className="flex h-full items-center gap-3" role="tablist" aria-label="Panel views">
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
          {settings.computerUse && (
            <button
              role="tab"
              aria-selected={tab === "screen"}
              className={tabClass(tab === "screen")}
              onClick={() => onTab("screen")}
            >
              Screen
            </button>
          )}
        </div>
        <div className="ml-auto flex min-w-0 items-center gap-1">
          {tab === "terminal" ? (
            <>
              {shells.length === 1 && (
                <span className="px-1 text-[length:var(--fs-sm)] text-fg-muted">
                  {shells[0].name}
                </span>
              )}
              <button
                className="btn-icon"
                aria-label="New terminal"
                title="New terminal"
                onClick={() => void start(true)}
              >
                <PlusIcon size={14} />
              </button>
              <Dropdown
                alignEnd
                label="Shells"
                trigger={({ open, toggle }) => (
                  <button
                    className="btn-icon"
                    aria-haspopup="menu"
                    aria-expanded={open}
                    aria-label="New terminal in a shell"
                    title="New terminal in a shell"
                    onClick={() => {
                      if (!open) void findShells();
                      toggle();
                    }}
                  >
                    <CaretDownIcon size={14} />
                  </button>
                )}
              >
                {(close) =>
                  shellChoices.map((s) => (
                    <button
                      key={s.path}
                      role="menuitemradio"
                      aria-checked={s === pickedShell}
                      title={s.path}
                      className={`flex w-full cursor-pointer items-center rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover ${s === pickedShell ? "bg-selected-muted" : ""}`}
                      onClick={() => {
                        close();
                        pickShell(s.path);
                      }}
                    >
                      {s.name}
                    </button>
                  ))
                }
              </Dropdown>
              {/* With the tab list showing, each tab has its own kill button. */}
              {shells.length < 2 && (
                <button
                  className="btn-icon"
                  aria-label="Kill terminal"
                  title="Kill terminal"
                  disabled={!active}
                  onClick={() => active && killShell(active)}
                >
                  <TrashIcon size={14} />
                </button>
              )}
            </>
          ) : tab === "output" ? (
            <>
              <Dropdown
                alignEnd
                label="Logs"
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
          ) : null}
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
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={[
            {
              label: "Kill terminal",
              Icon: TrashIcon,
              danger: true,
              onSelect: () => killShell(menu.shell),
            },
          ]}
          onClose={() => setMenu(null)}
        />
      )}
      {error && (
        <div role="alert" className="flex-none px-3 pb-1 text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
      <div className="flex min-h-0 flex-1">
        {tab === "terminal" ? (
          <>
            <div
              ref={host}
              role="group"
              aria-label="Terminal"
              className="h-full min-w-0 flex-1 overflow-hidden pl-3"
              {...shortcutContext("terminalFocus")}
            />
            {/* The terminals, listed beside the active one once there is more than one. */}
            {shells.length > 1 && (
              <div
                className="flex w-[120px] flex-none flex-col overflow-y-auto border-l border-border py-0.5"
                role="tablist"
                aria-label="Terminals"
                aria-orientation="vertical"
              >
                {shells.map((shell, i) => (
                  <ShellTab
                    key={shell.id}
                    shell={shell}
                    label={i === 0 ? shell.name : `${shell.name} ${i + 1}`}
                    active={shell === active}
                    onSelect={() => setActiveId(shell.id)}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      // The list sits at the panel's right edge; keep the menu inside the window.
                      setMenu({
                        x: Math.min(e.clientX, window.innerWidth - 170),
                        y: e.clientY,
                        shell,
                      });
                    }}
                  />
                ))}
              </div>
            )}
          </>
        ) : tab === "output" ? (
          <OutputView channel={channel?.id ?? null} />
        ) : (
          <ScreenView enabled={settings.computerUse} />
        )}
      </div>
    </section>
  );
}

function ShellTab({
  shell,
  label,
  active,
  onSelect,
  onContextMenu,
}: {
  shell: Shell;
  label: string;
  active: boolean;
  onSelect: () => void;
  onContextMenu: (e: MouseEvent) => void;
}) {
  return (
    // The tab is the label: a tab's children are left out of the accessibility tree, and the kill
    // button beside it has to stay in.
    <div
      className={`group flex h-[22px] flex-none cursor-pointer items-center gap-1.5 pr-0.5 pl-2 text-[length:var(--fs-sm)] ${active ? "bg-selected-muted text-fg" : "text-fg-muted hover:bg-hover hover:text-fg"}`}
      title={shell.exitCode !== undefined ? `Exited with code ${shell.exitCode}` : undefined}
      onClick={onSelect}
      // Middle-click kills the terminal.
      onAuxClick={(e) => e.button === 1 && killShell(shell)}
      onContextMenu={onContextMenu}
    >
      <TerminalWindowIcon size={14} className="flex-none" />
      <span
        role="tab"
        aria-selected={active}
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key !== "Enter" && e.key !== " ") return;
          e.preventDefault();
          onSelect();
        }}
        className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap"
      >
        {label}
      </span>
      <button
        className={`btn-icon size-[18px] ${active ? "" : "pointer-events-none opacity-0 group-hover:pointer-events-auto group-hover:opacity-100 focus-visible:opacity-100"}`}
        aria-label={`Kill ${label}`}
        title="Kill terminal"
        onClick={(e) => {
          e.stopPropagation();
          killShell(shell);
        }}
      >
        <TrashIcon size={12} />
      </button>
    </div>
  );
}
