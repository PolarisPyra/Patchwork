import { AGENT_LABELS, type AgentKind, type AgentStatus } from "~/api";

export const AGENT_KINDS = Object.keys(AGENT_LABELS) as AgentKind[];
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

export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function dirName(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? "" : path.slice(0, i);
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
  return status.version ? `${state} · v${status.version}` : state;
}
