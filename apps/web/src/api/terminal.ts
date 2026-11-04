import { Channel } from "@tauri-apps/api/core";
import { invoke } from "~/api/invoke";
import type { AttachedTerminal, TerminalFrame, TerminalInfo, TerminalShell } from "~/api/types";

/** A channel that hands each of a terminal's frames to `onFrame`. */
function frames(onFrame: (frame: TerminalFrame) => void) {
  const channel = new Channel<TerminalFrame>();
  channel.onmessage = onFrame;
  return channel;
}

export const terminalApi = {
  terminalShells: () => invoke<TerminalShell[]>("terminal_shells"),
  /** The terminals open in the app, which outlive the window that started them. */
  terminalList: () => invoke<TerminalInfo[]>("terminal_list"),
  /** Starts a terminal attached to this window; its output and its end go to `onFrame`. */
  terminalSpawn: (
    cwd: string | null,
    shell: string | null,
    cols: number,
    rows: number,
    onFrame: (frame: TerminalFrame) => void,
  ) =>
    invoke<AttachedTerminal>("terminal_spawn", {
      cwd,
      shell,
      cols,
      rows,
      onFrame: frames(onFrame),
    }),
  /** Attaches this window to a terminal: its screen so far, then its frames to `onFrame`. */
  terminalAttach: (id: number, onFrame: (frame: TerminalFrame) => void) =>
    invoke<AttachedTerminal>("terminal_attach", { id, onFrame: frames(onFrame) }),
  terminalDetach: (id: number, attachment: number) =>
    invoke<void>("terminal_detach", { id, attachment }),
  terminalWrite: (id: number, attachment: number, data: string) =>
    invoke<void>("terminal_write", { id, attachment, data }),
  terminalResize: (id: number, attachment: number, cols: number, rows: number) =>
    invoke<void>("terminal_resize", { id, attachment, cols, rows }),
  /** Ends the terminal's shell and everything it started. */
  terminalClose: (id: number) => invoke<void>("terminal_close", { id }),
};
