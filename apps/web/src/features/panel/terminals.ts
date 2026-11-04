import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ITerminalOptions, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useSyncExternalStore } from "react";
import { api, type TerminalFrame, type TerminalSettings } from "~/api";
import { boundCommand } from "~/lib/shortcuts";
import { copyText } from "~/lib/clipboard";

/**
 * The integrated terminal's shells. Each keeps its xterm and element here, outside React, so it
 * keeps running and keeps its scrollback while the panel is closed; the panel shows the active
 * one's element. The shells belong to the app: a window that reloads attaches to them again.
 */

export interface Shell {
  id: number;
  /** This window's attachment, which types into the shell. */
  attachment: number;
  /** The shell's file name, such as "zsh". */
  name: string;
  term: Terminal;
  fit: FitAddon;
  element: HTMLDivElement;
  /** Set once the shell has ended with an error, while its last output stays readable. */
  exitCode?: number;
  /** Whether its xterm has been drawn into its element yet. */
  opened: boolean;
  /** Output kept until it is drawn: a shell attached again is drawn at the panel's size, then
   * gets its screen. */
  backlog?: string;
}

/** App commands a terminal hands back instead of sending their keys to the shell, as VS Code's
 * `terminal.integrated.commandsToSkipShell`. */
const SKIP_SHELL = new Set([
  "terminal.toggle",
  "terminal.new",
  "panel.toggle",
  "output.toggle",
  "palette.toggle",
  "settings.open",
]);

const isMac = /Mac|iPhone|iPad/.test(navigator.platform);

/** VS Code's ANSI colors for its dark and light themes. */
const ANSI = {
  dark: [
    "#000000",
    "#cd3131",
    "#0dbc79",
    "#e5e510",
    "#2472c8",
    "#bc3fbc",
    "#11a8cd",
    "#e5e5e5",
    "#666666",
    "#f14c4c",
    "#23d18b",
    "#f5f543",
    "#3b8eea",
    "#d670d6",
    "#29b8db",
    "#e5e5e5",
  ],
  light: [
    "#000000",
    "#cd3131",
    "#107c10",
    "#949800",
    "#0451a5",
    "#bc05bc",
    "#0598bc",
    "#555555",
    "#666666",
    "#f14c4c",
    "#14ce14",
    "#b5ba00",
    "#3b8eea",
    "#d670d6",
    "#29b8db",
    "#a5a5a5",
  ],
};
const ANSI_NAMES = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const;

/** The app's colors, so the terminal blends into the panel around it. */
function theme(): ITheme {
  const style = getComputedStyle(document.documentElement);
  const color = (name: string) => style.getPropertyValue(name).trim();
  const dark = document.documentElement.dataset.theme !== "light";
  const ansi = Object.fromEntries(
    ANSI_NAMES.map((name, i) => [name, (dark ? ANSI.dark : ANSI.light)[i]]),
  );
  return {
    background: color("--bg-editor"),
    foreground: color("--text"),
    cursor: color("--text"),
    cursorAccent: color("--bg-editor"),
    selectionBackground: color("--bg-selected"),
    ...ansi,
  };
}

function options(settings: TerminalSettings): ITerminalOptions {
  const mono = getComputedStyle(document.documentElement).getPropertyValue("--font-mono").trim();
  return {
    fontFamily: settings.fontFamily || mono,
    fontSize: settings.fontSize,
    lineHeight: settings.lineHeight,
    letterSpacing: settings.letterSpacing,
    fontWeight: settings.fontWeight as ITerminalOptions["fontWeight"],
    fontWeightBold: settings.fontWeightBold as ITerminalOptions["fontWeightBold"],
    cursorStyle: settings.cursorStyle === "line" ? "bar" : settings.cursorStyle,
    cursorBlink: settings.cursorBlinking,
    cursorWidth: settings.cursorWidth,
    scrollback: settings.scrollback,
    altClickMovesCursor: settings.altClickMovesCursor,
    macOptionIsMeta: settings.macOptionIsMeta,
    drawBoldTextInBrightColors: settings.drawBoldTextInBrightColors,
    minimumContrastRatio: settings.minimumContrastRatio,
    smoothScrollDuration: settings.smoothScrolling ? 125 : 0,
    scrollSensitivity: settings.mouseWheelScrollSensitivity,
    fastScrollSensitivity: settings.fastScrollSensitivity,
    rightClickSelectsWord: settings.rightClickBehavior === "selectWord",
    theme: theme(),
    allowProposedApi: true,
  };
}

// --- the store ---

let shells: Shell[] = [];
const listeners = new Set<() => void>();
let current: TerminalSettings | null = null;
let restoring: Promise<void> | null = null;

function changed() {
  shells = [...shells];
  listeners.forEach((l) => l());
}

export function useShells(): Shell[] {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => void listeners.delete(l);
    },
    () => shells,
  );
}

/** The shells open now, outside React. */
export function openShells(): Shell[] {
  return shells;
}

// The terminal follows the app's light and dark theme.
new MutationObserver(() => {
  for (const shell of shells) shell.term.options.theme = theme();
}).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

// A module swapped while the app runs lets go of its shells, which the new one attaches to again.
import.meta.hot?.dispose(() => {
  for (const shell of shells) {
    void api.terminalDetach(shell.id, shell.attachment).catch(() => {});
    shell.term.dispose();
    shell.element.remove();
  }
});

function print(shell: Shell, data: string) {
  if (shell.backlog !== undefined) shell.backlog += data;
  else shell.term.write(data);
}

function exitText(shell: Shell, code: number) {
  return `\r\n\x1b[90mThe terminal process "${shell.name}" terminated with exit code: ${code}.\x1b[0m\r\n`;
}

function onFrame(shell: Shell, frame: TerminalFrame) {
  if (!shells.includes(shell)) return;
  if (frame.type === "output") {
    print(shell, frame.data);
    return;
  }
  // A shell that failed stays, so its last output can be read; one that ended cleanly closes.
  if (frame.code !== null && frame.code !== 0 && shell.exitCode === undefined) {
    shell.exitCode = frame.code;
    print(shell, exitText(shell, frame.code));
    changed();
    return;
  }
  remove(shell);
}

/** A terminal's frames, held until its shell is known, since they can come before the reply. */
function follower() {
  let shell: Shell | null = null;
  const early: TerminalFrame[] = [];
  return {
    onFrame: (frame: TerminalFrame) => (shell ? onFrame(shell, frame) : early.push(frame)),
    bind(to: Shell) {
      shell = to;
      for (const frame of early.splice(0)) onFrame(to, frame);
    },
  };
}

/** Takes the shell's tab away now, and ends its processes behind it. */
function remove(shell: Shell) {
  shell.term.dispose();
  shell.element.remove();
  shells = shells.filter((s) => s !== shell);
  changed();
  void api.terminalClose(shell.id).catch(() => {});
}

function create(settings: TerminalSettings) {
  const element = document.createElement("div");
  element.className = "h-full w-full";
  const term = new Terminal(options(settings));
  const fit = new FitAddon();
  term.loadAddon(fit);
  return { term, fit, element };
}

/** Connects the shell's xterm to its process, the clipboard and the app's keys. */
function wire(shell: Shell) {
  const { term, element } = shell;
  term.onData((data) => {
    if (shell.exitCode === undefined) void api.terminalWrite(shell.id, shell.attachment, data);
  });
  // Sent once the size settles: each resize makes the shell redraw its prompt, so a drag would
  // leave a trail of stale prompts behind.
  let resizeTimer = 0;
  term.onResize(({ cols, rows }) => {
    clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(
      () => void api.terminalResize(shell.id, shell.attachment, cols, rows).catch(() => {}),
      100,
    );
  });
  term.onSelectionChange(() => {
    if (current?.copyOnSelection && term.hasSelection()) {
      void copyText(term.getSelection()).catch(() => undefined);
    }
  });
  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown") return true;
    // The app's own keys go to the app.
    if (SKIP_SHELL.has(boundCommand(event) ?? "")) return false;
    const copyPaste = isMac ? event.metaKey && !event.shiftKey : event.ctrlKey && event.shiftKey;
    const key = event.key.toLowerCase();
    if (copyPaste && key === "c" && term.hasSelection()) {
      void copyText(term.getSelection()).catch(() => undefined);
      event.preventDefault();
      return false;
    }
    // The browser's paste reaches the terminal as a paste event.
    if (copyPaste && key === "v") return false;
    return true;
  });
  element.addEventListener("contextmenu", (event) => {
    const behavior = current?.rightClickBehavior ?? "default";
    if (behavior === "default" || behavior === "selectWord") return;
    event.preventDefault();
    if (behavior === "nothing") return;
    if (behavior === "copyPaste" && term.hasSelection()) {
      void copyText(term.getSelection()).catch(() => undefined);
      term.clearSelection();
      return;
    }
    navigator.clipboard.readText().then(
      (text) => term.paste(text),
      () => {},
    );
  });
}

/**
 * Starts `program` (a shell's path; null for the user's) in `cwd` (the project folder), drawn into `host`
 * to measure its size.
 */
export async function startShell(
  host: HTMLElement,
  cwd: string | null,
  program: string | null,
  settings: TerminalSettings,
): Promise<Shell> {
  current = settings;
  // Measured in its own font, so the size the shell starts at is the one it keeps.
  const family =
    settings.fontFamily ||
    getComputedStyle(document.documentElement).getPropertyValue("--font-mono");
  await document.fonts.load(`${settings.fontSize}px ${family}`).catch(() => {});
  const { term, fit, element } = create(settings);
  // The shell shown now comes back if this one fails to start.
  const shown = [...host.childNodes];
  host.replaceChildren(element);
  term.open(element);
  fit.fit();
  const follow = follower();
  let started;
  try {
    // At the measured size: a shell resized while it starts redraws its prompt out of place.
    started = await api.terminalSpawn(cwd, program, term.cols, term.rows, follow.onFrame);
  } catch (e) {
    term.dispose();
    host.replaceChildren(...shown);
    throw e;
  }
  const shell: Shell = {
    id: started.info.id,
    attachment: started.attachment,
    name: started.info.name,
    term,
    fit,
    element,
    opened: true,
  };
  wire(shell);
  shells = [...shells, shell];
  changed();
  follow.bind(shell);
  return shell;
}

/**
 * Attaches to the terminals the app has open, once per window: after a reload they come back
 * with their screens. A shell that ended cleanly before is closed instead.
 */
export function restoreShells(settings: TerminalSettings): Promise<void> {
  current ??= settings;
  restoring ??= (async () => {
    const open = await api.terminalList().catch(() => []);
    for (const info of open) {
      if (!info.running && !info.exitCode) {
        void api.terminalClose(info.id).catch(() => {});
        continue;
      }
      const follow = follower();
      const attached = await api.terminalAttach(info.id, follow.onFrame).catch(() => null);
      if (!attached) continue;
      const { term, fit, element } = create(settings);
      const shell: Shell = {
        id: info.id,
        attachment: attached.attachment,
        name: attached.info.name,
        term,
        fit,
        element,
        opened: false,
        backlog: attached.screen,
      };
      const code = attached.info.exitCode;
      if (!attached.info.running && code) {
        shell.exitCode = code;
        print(shell, exitText(shell, code));
      }
      wire(shell);
      shells = [...shells, shell];
      changed();
      follow.bind(shell);
    }
  })();
  return restoring;
}

/** Shows `shell` in `host`, drawing it there the first time at the host's size. */
export function showShell(shell: Shell, host: HTMLElement) {
  if (host.firstChild !== shell.element) host.replaceChildren(shell.element);
  if (shell.opened) {
    shell.term.refresh(0, shell.term.rows - 1);
    return;
  }
  shell.opened = true;
  shell.term.open(shell.element);
  shell.fit.fit();
  shell.term.write(shell.backlog ?? "");
  shell.backlog = undefined;
  const { cols, rows } = shell.term;
  void api.terminalResize(shell.id, shell.attachment, cols, rows).catch(() => {});
}

/** Closes a shell: its tab goes now, and its processes end behind it. */
export function killShell(shell: Shell) {
  remove(shell);
}

/** Applies changed settings to every open shell. */
export function applySettings(settings: TerminalSettings) {
  if (current === settings) return;
  current = settings;
  for (const shell of shells) {
    shell.term.options = options(settings);
    if (shell.opened) shell.fit.fit();
  }
}
