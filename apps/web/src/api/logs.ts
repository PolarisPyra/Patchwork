import { invoke } from "~/api/invoke";
import type { LogChunk, LogLevel } from "~/api/types";

export const logsApi = {
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  exportLogs: (path: string) => invoke<void>("export_logs", { path }),
  openThreadLogs: (threadId: string) => invoke<void>("open_thread_logs", { threadId }),
  logChannels: () => invoke<string[]>("log_channels"),
  readLog: (channel: string, offset: number) => invoke<LogChunk>("read_log", { channel, offset }),
  logRenderer: (level: LogLevel, message: string) =>
    invoke<void>("log_renderer", { level, message }),
};
