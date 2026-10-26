import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ITerminalOptions, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useSyncExternalStore } from "react";
import { api, onTerminalExit, onTerminalOutput, type TerminalSettings } from "~/api";
import { boundCommand } from "~/lib/shortcuts";

/**
 * The integrated terminal's shells. Each keeps its xterm and element here, outside React, so it
 * keeps running and keeps its scrollback while the panel is closed; the panel shows the active
 * one's element.
 */

export interface Shell {
  id: number;
  /** The shell's file name, such as "zsh". */
  name: string;
  term: Terminal;
  fit: FitAddon;
  element: HTMLDivElement;
  /** Set once the shell has ended with an error, while its last output stays readable. */
  exitCode?: number;
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
    "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
    "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#e5e5e5",
  ],
  light: [
    "#000000", "#cd3131", "#107c10", "#949800", "#0451a5", "#bc05bc", "#0598bc", "#555555",
    "#666666", "#f14c4c", "#14ce14", "#b5ba00", "#3b8eea", "#d670d6", "#29b8db", "#a5a5a5",
  ],
};
const ANSI_NAMES = [
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow",
  "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
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
/** Output that arrived before its shell's id was known. */
const early = new Map<number, string[]>();
let current: TerminalSettings | null = null;

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

void onTerminalOutput(({ id, data }) => {
  const shell = shells.find((s) => s.id === id);
  if (shell) shell.term.write(data);
  else early.set(id, [...(early.get(id) ?? []), data]);
});

void onTerminalExit(({ id, code }) => {
  const shell = shells.find((s) => s.id === id);
  if (!shell) return;
  // A shell that failed stays, so its last output can be read; one that ended cleanly closes.
  if (code !== null && code !== 0 && shell.exitCode === undefined) {
    shell.exitCode = code;
    shell.term.write(
      `\r\n\x1b[90mThe terminal process "${shell.name}" terminated with exit code: ${code}.\x1b[0m\r\n`,
    );
    changed();
    return;
  }
  remove(shell);
});

// The terminal follows the app's light and dark theme.
new MutationObserver(() => {
  for (const shell of shells) shell.term.options.theme = theme();
}).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

function remove(shell: Shell) {
  shell.term.dispose();
  shell.element.remove();
  shells = shells.filter((s) => s !== shell);
  changed();
}

/** Starts a shell in `cwd` (the project folder), drawn into `host` to measure its size. */
export async function startShell(
  host: HTMLElement,
  cwd: string | null,
  settings: TerminalSettings,
): Promise<Shell> {
  current = settings;
  // Measured in its own font, so the size the shell starts at is the one it keeps.
  const family = settings.fontFamily || getComputedStyle(document.documentElement).getPropertyValue("--font-mono");
  await document.fonts.load(`${settings.fontSize}px ${family}`).catch(() => {});
  const element = document.createElement("div");
  element.className = "h-full w-full";
  host.replaceChildren(element);
  const term = new Terminal(options(settings));
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(element);
  fit.fit();
  let started;
  try {
    // At the measured size: a shell resized while it starts redraws its prompt out of place.
    started = await api.terminalSpawn(cwd, term.cols, term.rows);
  } catch (e) {
    term.dispose();
    element.remove();
    throw e;
  }
  const shell: Shell = { id: started.id, name: started.name, term, fit, element };
  for (const data of early.get(shell.id) ?? []) term.write(data);
  early.delete(shell.id);

  term.onData((data) => {
    if (shell.exitCode === undefined) void api.terminalWrite(shell.id, data);
  });
  // Sent once the size settles: each resize makes the shell redraw its prompt, so a drag would
  // leave a trail of stale prompts behind.
  let resizeTimer = 0;
  term.onResize(({ cols, rows }) => {
    clearTimeout(resizeTimer);
    resizeTimer = window.setTimeout(() => void api.terminalResize(shell.id, cols, rows), 100);
  });
  term.onSelectionChange(() => {
    if (current?.copyOnSelection && term.hasSelection()) {
      void navigator.clipboard.writeText(term.getSelection());
    }
  });
  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown") return true;
    // The app's own keys go to the app.
    if (SKIP_SHELL.has(boundCommand(event) ?? "")) return false;
    const copyPaste = isMac
      ? event.metaKey && !event.shiftKey
      : event.ctrlKey && event.shiftKey;
    const key = event.key.toLowerCase();
    if (copyPaste && key === "c" && term.hasSelection()) {
      void navigator.clipboard.writeText(term.getSelection());
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
      void navigator.clipboard.writeText(term.getSelection());
      term.clearSelection();
      return;
    }
    navigator.clipboard.readText().then(
      (text) => term.paste(text),
      () => {},
    );
  });
  shells = [...shells, shell];
  changed();
  return shell;
}

/** Ends a shell; it closes once its process has exited. One that already ended closes now. */
export function killShell(shell: Shell) {
  if (shell.exitCode !== undefined) remove(shell);
  else void api.terminalKill(shell.id);
}

/** Applies changed settings to every open shell. */
export function applySettings(settings: TerminalSettings) {
  if (current === settings) return;
  current = settings;
  for (const shell of shells) {
    shell.term.options = options(settings);
    shell.fit.fit();
  }
}
