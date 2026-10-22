import { useEffect, useRef, useSyncExternalStore } from "react";
import { COMMANDS, DEFAULT_RULES } from "~/lib/shortcutDefaults";

/**
 * Keyboard shortcuts as commands, keys and contexts, in three parts that know nothing of each
 * other:
 *  - a component registers what a command does with `useCommand("chat.new", run)`;
 *  - a keybinding rule `{ key, command, when }` says which key runs it (`shortcutDefaults.ts`),
 *    and the user can replace the key in settings;
 *  - the `when` clause is checked against the context where the key was pressed: flags set on a
 *    region with `shortcutContext(...)`, flags a component reports with `useContextKey(...)`, and
 *    `textInputFocus` and `hasTextSelection`, which come from the page.
 *
 * One window listener resolves a key press: the last matching rule whose command is registered
 * runs, and the key's default action is prevented.
 */

export type Rule = { key: string; command: string; when?: string };
export type CommandInfo = { id: string; title: string; category: string };

// --- when clauses: flag names joined with !, && and || and grouped with parentheses ---

type Lookup = (flag: string) => boolean;
type Clause = (has: Lookup) => boolean;
const clauses = new Map<string, Clause>();

function compile(source: string): Clause {
  const tokens = source.match(/\|\||&&|!|\(|\)|[\w.]+/g) ?? [];
  let at = 0;
  const fail = () => new Error(`Bad when clause: ${source}`);
  const unary = (): Clause => {
    const token = tokens[at++];
    if (token === "!") {
      const inner = unary();
      return (has) => !inner(has);
    }
    if (token === "(") {
      const inner = or();
      if (tokens[at++] !== ")") throw fail();
      return inner;
    }
    if (token && /^[\w.]+$/.test(token)) return (has) => has(token);
    throw fail();
  };
  const and = (): Clause => {
    let left = unary();
    while (tokens[at] === "&&") {
      at++;
      const [a, b] = [left, unary()];
      left = (has) => a(has) && b(has);
    }
    return left;
  };
  const or = (): Clause => {
    let left = and();
    while (tokens[at] === "||") {
      at++;
      const [a, b] = [left, and()];
      left = (has) => a(has) || b(has);
    }
    return left;
  };
  const clause = or();
  if (at !== tokens.length) throw fail();
  return clause;
}

function holds(when: string | undefined, has: Lookup): boolean {
  if (!when) return true;
  let clause = clauses.get(when);
  if (!clause) {
    try {
      clause = compile(when);
    } catch (e) {
      console.warn(e);
      clause = () => false;
    }
    clauses.set(when, clause);
  }
  return clause(has);
}

// --- contexts ---

const CONTEXT_ATTR = "data-shortcut-context";

/** Spread onto a region to flag it, e.g. `shortcutContext("transcriptFocus")`. A key pressed inside
 * the region sees the flag. The region must be focusable (`tabIndex={-1}`) so a click moves focus in. */
export const shortcutContext = (...flags: string[]) => ({ [CONTEXT_ATTR]: flags.join(" ") });

type Live<T> = { current: T };
const reporters = new Map<string, Live<() => boolean>[]>();

/** Reports a flag that depends on app state. `value` is read when a key is pressed, so it needs no
 * re-registering as the state changes. */
export function useContextKey(flag: string, value: () => boolean) {
  const latest = useRef(value);
  latest.current = value;
  useEffect(() => {
    reporters.set(flag, [...(reporters.get(flag) ?? []), latest]);
    return () => {
      reporters.set(
        flag,
        (reporters.get(flag) ?? []).filter((r) => r !== latest),
      );
    };
  }, [flag]);
}

const inTextField = (el: Element) =>
  el instanceof HTMLElement &&
  (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));

function contextOf(target: Element): Lookup {
  const flags = new Set<string>();
  const selector = `[${CONTEXT_ATTR}]`;
  for (let el = target.closest(selector); el; el = el.parentElement?.closest(selector) ?? null) {
    el.getAttribute(CONTEXT_ATTR)
      ?.split(" ")
      .forEach((flag) => flags.add(flag));
  }
  if (inTextField(target)) flags.add("textInputFocus");
  const selection = window.getSelection();
  if (selection && !selection.isCollapsed) flags.add("hasTextSelection");
  return (flag) => flags.has(flag) || !!reporters.get(flag)?.some((r) => r.current());
}

// --- commands ---

type Handler = (event: KeyboardEvent) => void;
const handlers = new Map<string, Live<Handler>[]>();
let listening = false;

/** Registers what `command` does. If several components register one command, the latest mounted wins. */
export function useCommand(command: string, run: Handler) {
  const latest = useRef(run);
  latest.current = run;
  useEffect(() => {
    handlers.set(command, [...(handlers.get(command) ?? []), latest]);
    if (!listening) {
      window.addEventListener("keydown", dispatch);
      listening = true;
    }
    return () => {
      handlers.set(
        command,
        (handlers.get(command) ?? []).filter((h) => h !== latest),
      );
    };
  }, [command]);
}

let suspended = 0;
/** Stops keys running commands, e.g. while a settings row records a new key. Returns the resume function. */
export function suspendShortcuts(): () => void {
  suspended++;
  return () => void suspended--;
}

function dispatch(event: KeyboardEvent) {
  if (suspended > 0 || event.isComposing || !(event.target instanceof Element)) return;
  const key = keyFromEvent(event);
  if (!key) return;
  const rules = store.snapshot.rules;
  let has: Lookup | undefined;
  for (let i = rules.length - 1; i >= 0; i--) {
    const rule = rules[i];
    if (rule.key !== key) continue;
    const run = handlers.get(rule.command)?.at(-1);
    if (!run) continue;
    has ??= contextOf(event.target);
    if (!holds(rule.when, has)) continue;
    event.preventDefault();
    run.current(event);
    return;
  }
}

// --- keys ---

const MODIFIER_KEYS = new Set(["control", "shift", "alt", "meta", "altgraph", "capslock"]);

/** The key as stored: "mod+shift+k", "escape", "mod+,". `mod` is Ctrl, or Cmd on a Mac. Null for a bare modifier. */
export function keyFromEvent(event: KeyboardEvent): string | null {
  const name = event.key.toLowerCase();
  if (MODIFIER_KEYS.has(name)) return null;
  const parts: string[] = [];
  if (event.ctrlKey || event.metaKey) parts.push("mod");
  if (event.altKey) parts.push("alt");
  if (event.shiftKey) parts.push("shift");
  parts.push(name === " " ? "space" : name === "+" ? "plus" : name);
  return parts.join("+");
}

const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
const KEY_NAMES: Record<string, string> = {
  escape: "Esc",
  arrowup: "Up",
  arrowdown: "Down",
  arrowleft: "Left",
  arrowright: "Right",
  plus: "+",
};

/** A stored key as the parts to show: ["Ctrl", "Shift", "K"]. */
export function formatKey(key: string): string[] {
  return key.split("+").map((part) => {
    if (part === "mod") return isMac ? "Cmd" : "Ctrl";
    if (part === "alt") return isMac ? "Option" : "Alt";
    return (
      KEY_NAMES[part] ??
      (part.length === 1 ? part.toUpperCase() : part[0].toUpperCase() + part.slice(1))
    );
  });
}

/** Whether a rule could run while typing: a bare printable key (or Shift plus one) would take the
 * letters out of a text field unless the rule's `when` leaves text fields alone. */
export function keyProblem(command: string, key: string): string | null {
  const parts = key.split("+");
  const bare = parts.at(-1)!;
  const printable = bare.length === 1 || bare === "space" || bare === "plus";
  const modified = parts.includes("mod") || parts.includes("alt");
  if (!printable || modified) return null;
  const rule = DEFAULT_RULES.find((r) => r.command === command);
  return rule?.when?.includes("!textInputFocus")
    ? null
    : "This shortcut works while typing, so it needs Ctrl, Cmd or Alt";
}

// --- the keybinding table: the defaults, with the user's changes on top ---

const STORAGE_KEY = "patchworkKeybindings";
/** A command's key replaced by the user, or null for a command they unbound. */
type Overrides = Record<string, string | null>;
type Snapshot = { rules: Rule[]; overrides: Overrides };

function load(): Overrides {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    if (!saved || typeof saved !== "object") return {};
    return Object.fromEntries(
      Object.entries(saved).filter(
        ([id, key]) =>
          COMMANDS.some((c) => c.id === id) && (key === null || typeof key === "string"),
      ),
    );
  } catch {
    return {};
  }
}

function build(overrides: Overrides): Snapshot {
  const remapped = (r: Rule) => r.command in overrides;
  return {
    overrides,
    rules: [
      ...DEFAULT_RULES.filter((r) => !remapped(r)),
      // After the defaults, so a key the user picked beats the built-in use of it.
      ...DEFAULT_RULES.filter(remapped).flatMap((r) => {
        const key = overrides[r.command];
        return key ? [{ ...r, key }] : [];
      }),
    ],
  };
}

const store = {
  snapshot: build(load()),
  listeners: new Set<() => void>(),
  set(overrides: Overrides) {
    this.snapshot = build(overrides);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides));
    this.listeners.forEach((listener) => listener());
  },
};

const subscribe = (listener: () => void) => {
  store.listeners.add(listener);
  return () => void store.listeners.delete(listener);
};

/** The current keybindings; re-renders when the user changes one. */
export function useKeybindings(): Snapshot {
  return useSyncExternalStore(subscribe, () => store.snapshot);
}

/** Gives `command` a new key, or null to unbind it. A key equal to the default clears the change. */
export function setKeybinding(command: string, key: string | null) {
  const { [command]: _dropped, ...rest } = store.snapshot.overrides;
  const base = DEFAULT_RULES.find((r) => r.command === command);
  store.set(key === base?.key ? rest : { ...rest, [command]: key });
}

export function resetKeybinding(command: string) {
  const { [command]: _dropped, ...rest } = store.snapshot.overrides;
  store.set(rest);
}

export function resetKeybindings() {
  store.set({});
}

/** The rule that decides a command's key: the last one, which wins a clash. */
function ruleFor(rules: Rule[], command: string): Rule | undefined {
  return rules.filter((r) => r.command === command).at(-1);
}

export function keyFor(rules: Rule[], command: string): string | null {
  return ruleFor(rules, command)?.key ?? null;
}

/** "Ctrl T" for a command's key, for a hint beside its menu item. Undefined when unbound. */
export function keyLabel(rules: Rule[], command: string): string | undefined {
  const key = keyFor(rules, command);
  return key ? formatKey(key).join(" ") : undefined;
}

/** Other commands on the same key that could run in the same place: either rule has no `when`,
 * or both have the same one. Rules with different `when` clauses, like the Ctrl+A ones, do not clash. */
export function conflictsWith(rules: Rule[], command: string): CommandInfo[] {
  const mine = ruleFor(rules, command);
  if (!mine) return [];
  const ids = rules
    .filter((r) => r.command !== command && r.key === mine.key)
    .filter((r) => !r.when || !mine.when || r.when === mine.when)
    .map((r) => r.command);
  return COMMANDS.filter((c) => ids.includes(c.id));
}
