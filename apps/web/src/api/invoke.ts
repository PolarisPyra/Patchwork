import { invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { profile } from "~/lib/profile";

export const invoke = <T>(cmd: string, args?: InvokeArgs) =>
  profile(`ipc:${cmd}`, () => tauriInvoke<T>(cmd, args)).catch((error: unknown) => {
    logFailure(cmd, args, error);
    throw error;
  });

/**
 * A command that failed goes to `window/renderer.log` and, when it names a chat, to that chat's
 * log, so a chat's log has what the person was told went wrong.
 */
function logFailure(cmd: string, args: InvokeArgs | undefined, error: unknown) {
  if (cmd === "log_renderer") return;
  const fields =
    args && !Array.isArray(args) && !(args instanceof ArrayBuffer) && !ArrayBuffer.isView(args)
      ? (args as Record<string, unknown>)
      : {};
  const named = (key: string) => (typeof fields[key] === "string" ? fields[key] : undefined);
  const threadId = named("threadId") ?? (cmd.includes("thread") ? named("id") : undefined);
  const message = `${cmd} failed: ${error instanceof Error ? error.message : String(error)}`;
  tauriInvoke("log_renderer", { level: "warning", message, threadId }).catch(() => {});
}
