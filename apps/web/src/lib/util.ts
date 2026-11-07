import { AGENT_LABELS, type AgentKind, type AgentStatus, type CliKind } from "~/api";

export const AGENT_KINDS = Object.keys(AGENT_LABELS) as AgentKind[];
/** The agents that run an installed CLI; API providers run in Patchwork itself. */
export const CLI_KINDS = AGENT_KINDS.filter((k): k is CliKind => k !== "api");
/** Names of the folders a sort by agent puts tasks in. */
export const AGENT_FOLDER_NAMES: ReadonlySet<string> = new Set(Object.values(AGENT_LABELS));

const pad = (n: number) => String(n).padStart(2, "0");

/** Local 12-hour time, e.g. "4:27 PM". */
export function formatClock(ts: number): string {
  const d = new Date(ts);
  const h = d.getHours();
  return `${h % 12 || 12}:${pad(d.getMinutes())} ${h < 12 ? "AM" : "PM"}`;
}

/** Compact age: "now", "5m", "3h", "2d". */
export function formatRelative(ts: number): string {
  const minutes = Math.floor((Date.now() - ts) / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

const BYTE_UNITS = ["B", "kB", "MB", "GB"];

/** File size, e.g. "787 B" or "12.4 kB". Decimal units, as file managers show them, so 1 kB is 1000 B. */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < BYTE_UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  if (unit === 0) return `${value} B`;
  return `${value < 100 ? value.toFixed(1) : Math.round(value)} ${BYTE_UNITS[unit]}`;
}

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function dirName(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? "" : path.slice(0, i);
}

/** A Markdown file name from a title: lowercase words joined by dashes, else `fallback`. */
export function slugFileName(title: string, fallback: string): string {
  const slug = title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${slug || fallback}.md`;
}

/** A path under the person's home folder, written from `~` so it fits in a settings row. */
export function tildify(path: string): string {
  return path.replace(/^(\/home\/[^/]+|\/Users\/[^/]+|[A-Za-z]:\\Users\\[^\\]+)/, "~");
}

/** Compact duration for a row: "42s", "3m", "1.5h", "12h". */
export function compactElapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = s / 3600;
  return h < 10 ? `${Number(h.toFixed(1))}h` : `${Math.floor(h)}h`;
}

/** Full duration for a details card or a status line: "42s", "3m 05s", "2h 14m". */
export function fullElapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

const LANGUAGES: Record<string, string> = {
  ts: "TypeScript",
  tsx: "TypeScript JSX",
  js: "JavaScript",
  jsx: "JavaScript JSX",
  css: "CSS",
  html: "HTML",
  json: "JSON",
  md: "Markdown",
  rs: "Rust",
  py: "Python",
  go: "Go",
  toml: "TOML",
  yml: "YAML",
  yaml: "YAML",
  sh: "Shell",
  svg: "SVG",
};

export function languageLabel(path: string): string {
  const name = baseName(path);
  const dot = name.lastIndexOf(".");
  if (dot === -1) return "Plain text";
  const ext = name.slice(dot + 1).toLowerCase();
  return LANGUAGES[ext] ?? ext.toUpperCase();
}

/** A thread title for display, with its first letter capitalized. */
export function displayTitle(title: string): string {
  return title.charAt(0).toUpperCase() + title.slice(1);
}

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** One-line agent summary. `undefined` = detection pending, `null` = detection failed. */
export function describeAgent(status: AgentStatus | null | undefined): string {
  if (status === undefined) return "Checking…";
  if (status === null) return "Unavailable";
  if (!status.installed) return status.detail || "Not installed";
  const state = status.signedIn ? "Ready" : "Sign-in required";
  return status.version ? `${state} · ${status.version}` : state;
}
