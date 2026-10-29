import { invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { profile } from "~/lib/profile";

export const invoke = <T>(cmd: string, args?: InvokeArgs) =>
  profile(`ipc:${cmd}`, () => tauriInvoke<T>(cmd, args));
