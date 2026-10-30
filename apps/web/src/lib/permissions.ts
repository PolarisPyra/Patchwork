import { createContext } from "react";
import type {
  Approval,
  Effect,
  PermissionKind,
  PermissionRule,
  PermissionScope,
  PermissionSettings,
  Project,
  ScopeRules,
} from "~/api";

export const PERMISSION_KINDS: { kind: PermissionKind; label: string; placeholder: string }[] = [
  { kind: "command", label: "Run commands", placeholder: "git status" },
];

export const kindLabel = (kind: PermissionKind) =>
  PERMISSION_KINDS.find((k) => k.kind === kind)?.label ?? kind;

export const EFFECT_LABELS: Record<Effect, string> = { allow: "Allow", ask: "Ask", deny: "Deny" };

const EMPTY: ScopeRules = { defaults: {}, rules: [] };

/** The scope a thread's rules live in: its project's, or the one for chats. */
export function threadScope(projectId: string | null | undefined): PermissionScope {
  return projectId ? { type: "project", projectId } : { type: "chats" };
}

export const sameScope = (a: PermissionScope, b: PermissionScope) =>
  a.type === b.type && (a.type !== "project" || a.projectId === (b as typeof a).projectId);

export function scopeName(scope: PermissionScope, projects: Project[]): string {
  if (scope.type === "everywhere") return "Everywhere";
  if (scope.type === "chats") return "Chats";
  return projects.find((p) => p.id === scope.projectId)?.name ?? "Removed project";
}

export function scopeRules(permissions: PermissionSettings, scope: PermissionScope): ScopeRules {
  if (scope.type === "everywhere") return permissions.everywhere;
  if (scope.type === "chats") return permissions.chats;
  return permissions.projects[scope.projectId] ?? EMPTY;
}

/** `permissions` with one scope's rules changed by `change`. */
export function withScopeRules(
  permissions: PermissionSettings,
  scope: PermissionScope,
  change: (rules: ScopeRules) => ScopeRules,
): PermissionSettings {
  const next = change(scopeRules(permissions, scope));
  if (scope.type === "everywhere") return { ...permissions, everywhere: next };
  if (scope.type === "chats") return { ...permissions, chats: next };
  return { ...permissions, projects: { ...permissions.projects, [scope.projectId]: next } };
}

/** The rule `ruleId` in `scope`, if it still exists. */
export function findRule(
  permissions: PermissionSettings,
  scope: PermissionScope,
  ruleId: string,
): PermissionRule | undefined {
  return scopeRules(permissions, scope).rules.find((r) => r.id === ruleId);
}

/** A rule's pattern as people read it. */
export function shownPattern(pattern: string): string {
  if (pattern === "*") return "Anything";
  if (pattern === ".") return "Its own folder";
  return pattern;
}

/** The rules "Always allow" saves for an approval, or none when it cannot be covered safely. */
export function suggestedRules(approval: Approval): PermissionRule[] {
  const { kind, suggestions = [] } = approval;
  if (!kind) return [];
  return suggestions.map((pattern) => ({
    id: crypto.randomUUID(),
    kind,
    pattern,
    effect: "allow",
  }));
}

/** How the last approval of each kind was answered, which the Allow button offers first. */
export type AllowChoice = "once" | "local" | "everywhere";

const choiceKey = "patchwork-allow-choice";

export function lastAllowChoice(kind: PermissionKind | null | undefined): AllowChoice {
  if (!kind) return "once";
  try {
    const saved = JSON.parse(localStorage.getItem(choiceKey) ?? "{}") as Record<
      string,
      AllowChoice
    >;
    return saved[kind] ?? "once";
  } catch {
    return "once";
  }
}

export function rememberAllowChoice(kind: PermissionKind | null | undefined, choice: AllowChoice) {
  if (!kind) return;
  try {
    const saved = JSON.parse(localStorage.getItem(choiceKey) ?? "{}") as Record<
      string,
      AllowChoice
    >;
    localStorage.setItem(choiceKey, JSON.stringify({ ...saved, [kind]: choice }));
  } catch {
    /* The button offers Allow once next time. */
  }
}

/** What the transcript needs to show and change the rules behind approvals. */
export interface PermissionsContextValue {
  permissions: PermissionSettings;
  projects: Project[];
  /** The scope of the thread on screen. */
  scope: PermissionScope;
  /** Saves `rules` as allow rules in `scope`. */
  addRules: (scope: PermissionScope, rules: PermissionRule[]) => void;
  removeRule: (scope: PermissionScope, ruleId: string) => void;
  /** Opens Settings → Permissions on `scope`, with `ruleId` picked out. */
  openRule: (scope: PermissionScope, ruleId?: string) => void;
}

export const PermissionsContext = createContext<PermissionsContextValue | null>(null);

/** `rules` with the starting rules put back as they first were: deleted ones added again and
 * changed ones reset. The user's own rules stay, and a starting rule is not added again where one
 * of them already covers the same pattern. */
export function withDefaultRules(
  rules: PermissionRule[],
  defaults: PermissionRule[],
): PermissionRule[] {
  const isDefault = (r: PermissionRule) => defaults.some((d) => d.id === r.id);
  const own = rules.filter((r) => !isDefault(r));
  const restored = defaults.filter(
    (d) => !own.some((r) => r.kind === d.kind && r.pattern.trim() === d.pattern),
  );
  return [...restored, ...own];
}

/** Whether restoring the starting rules would change `rules`. */
export function defaultsChanged(rules: PermissionRule[], defaults: PermissionRule[]): boolean {
  const restored = withDefaultRules(rules, defaults);
  const key = (r: PermissionRule) => JSON.stringify([r.id, r.kind, r.pattern, r.effect]);
  const now = new Set(rules.map(key));
  return restored.length !== rules.length || restored.some((r) => !now.has(key(r)));
}
