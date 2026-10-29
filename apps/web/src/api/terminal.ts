import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "~/api/invoke";

/** A shell the terminal started. */
export interface StartedTerminal {
  id: number;
  /** The shell's file name, such as "zsh". */
  name: string;
  cwd: string;
}

export const terminalApi = {
  terminalSpawn: (cwd: string | null, cols: number, rows: number) =>
    invoke<StartedTerminal>("terminal_spawn", { cwd, cols, rows }),
  terminalWrite: (id: number, data: string) => invoke<void>("terminal_write", { id, data }),
  terminalResize: (id: number, cols: number, rows: number) =>
    invoke<void>("terminal_resize", { id, cols, rows }),
  terminalKill: (id: number) => invoke<void>("terminal_kill", { id }),
};

export function onTerminalOutput(
  cb: (e: { id: number; data: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ id: number; data: string }>("terminal-output", (e) => cb(e.payload));
}

export function onTerminalExit(
  cb: (e: { id: number; code: number | null }) => void,
): Promise<UnlistenFn> {
  return listen<{ id: number; code: number | null }>("terminal-exit", (e) => cb(e.payload));
}
