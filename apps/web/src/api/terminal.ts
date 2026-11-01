import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "~/api/invoke";
import type { StartedTerminal, TerminalExit, TerminalOutput } from "~/api/types";

export const terminalApi = {
  terminalSpawn: (cwd: string | null, cols: number, rows: number) =>
    invoke<StartedTerminal>("terminal_spawn", { cwd, cols, rows }),
  terminalWrite: (id: number, data: string) => invoke<void>("terminal_write", { id, data }),
  terminalResize: (id: number, cols: number, rows: number) =>
    invoke<void>("terminal_resize", { id, cols, rows }),
  terminalKill: (id: number) => invoke<void>("terminal_kill", { id }),
};

export function onTerminalOutput(cb: (e: TerminalOutput) => void): Promise<UnlistenFn> {
  return listen<TerminalOutput>("terminal-output", (e) => cb(e.payload));
}

export function onTerminalExit(cb: (e: TerminalExit) => void): Promise<UnlistenFn> {
  return listen<TerminalExit>("terminal-exit", (e) => cb(e.payload));
}
