import { api, type LogLevel } from "~/api";

/**
 * The window's log, `window1/renderer.log`, as VS Code's renderer log: uncaught errors, rejected
 * promises, and the console. A message below the log's level is dropped here, so it never
 * crosses to the backend.
 */

const RANK: Record<LogLevel, number> = { off: 0, error: 1, warning: 2, info: 3, debug: 4, trace: 5 };
let level: LogLevel = "info";

/** Follows Settings → Logging. */
export function setRendererLogLevel(next: LogLevel) {
  level = next;
}

function log(at: Exclude<LogLevel, "off">, parts: unknown[]) {
  if (RANK[at] > RANK[level]) return;
  const message = parts.map(text).join(" ");
  api.logRenderer(at, message).catch(() => {});
}

function text(part: unknown): string {
  if (part instanceof Error) return part.stack ?? `${part.name}: ${part.message}`;
  if (typeof part === "string") return part;
  try {
    return JSON.stringify(part);
  } catch {
    return String(part);
  }
}

/** Starts logging; called once when the window loads. */
export function installRendererLog() {
  window.addEventListener("error", (e) =>
    log("error", [e.error ?? `${e.message} (${e.filename}:${e.lineno}:${e.colno})`]),
  );
  window.addEventListener("unhandledrejection", (e) =>
    log("error", ["Unhandled promise rejection:", e.reason]),
  );
  const methods: [keyof Console, Exclude<LogLevel, "off">][] = [
    ["error", "error"],
    ["warn", "warning"],
    ["info", "info"],
    ["log", "debug"],
    ["debug", "trace"],
  ];
  for (const [name, at] of methods) {
    const original = console[name] as (...args: unknown[]) => void;
    (console as unknown as Record<string, unknown>)[name] = (...args: unknown[]) => {
      original.apply(console, args);
      log(at, args);
    };
  }
}
