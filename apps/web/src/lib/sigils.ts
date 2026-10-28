import { useSyncExternalStore } from "react";

/**
 * The characters that open the composer's menus: files, skills, agents and commands. The user can
 * type a different one for each, but what is sent always uses the default, which is what the
 * agents and the backend read.
 */
export type SigilKind = "files" | "skills" | "agents" | "commands";
export type Sigils = Record<SigilKind, string>;

export const SIGILS: { kind: SigilKind; title: string }[] = [
  { kind: "files", title: "Mention a file or folder" },
  { kind: "skills", title: "Use a skill" },
  { kind: "agents", title: "Summon an agent" },
  { kind: "commands", title: "Run a command" },
];

export const DEFAULT_SIGILS: Sigils = { files: "@", skills: "$", agents: "&", commands: "/" };

const STORAGE_KEY = "patchworkSigils";
const KINDS = SIGILS.map((s) => s.kind);

/** One symbol or punctuation mark: letters, digits and spaces are part of what is typed. */
const isSigil = (char: string) => char.length === 1 && /[^\p{L}\p{N}\s]/u.test(char);

function load(): Sigils {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    const sigils = { ...DEFAULT_SIGILS };
    if (saved && typeof saved === "object") {
      for (const [kind, char] of Object.entries(saved)) {
        if (KINDS.includes(kind as SigilKind) && typeof char === "string" && isSigil(char)) {
          sigils[kind as SigilKind] = char;
        }
      }
    }
    // Two kinds sharing a character would be ambiguous, so a clash falls back to the defaults.
    return new Set(Object.values(sigils)).size === KINDS.length ? sigils : { ...DEFAULT_SIGILS };
  } catch {
    return { ...DEFAULT_SIGILS };
  }
}

const store = {
  sigils: load(),
  listeners: new Set<() => void>(),
  set(sigils: Sigils) {
    this.sigils = sigils;
    const changed = Object.fromEntries(
      KINDS.filter((k) => sigils[k] !== DEFAULT_SIGILS[k]).map((k) => [k, sigils[k]]),
    );
    localStorage.setItem(STORAGE_KEY, JSON.stringify(changed));
    this.listeners.forEach((listener) => listener());
  },
};

const subscribe = (listener: () => void) => {
  store.listeners.add(listener);
  return () => void store.listeners.delete(listener);
};

/** The sigils; re-renders when the user changes one. */
export function useSigils(): Sigils {
  return useSyncExternalStore(subscribe, () => store.sigils);
}

/** The sigils, for the editor, which reads them outside React as each key is typed. */
export function getSigils(): Sigils {
  return store.sigils;
}

/** Why `char` cannot open `kind`'s menu, or null when it can. */
export function sigilProblem(kind: SigilKind, char: string): string | null {
  if (!isSigil(char)) return "Use a single symbol, such as # or !";
  const taken = SIGILS.find((s) => s.kind !== kind && store.sigils[s.kind] === char);
  return taken ? `${char} already does: ${taken.title}` : null;
}

export function setSigil(kind: SigilKind, char: string) {
  if (sigilProblem(kind, char)) return;
  store.set({ ...store.sigils, [kind]: char });
}

export function resetSigil(kind: SigilKind) {
  const sigils = { ...store.sigils, [kind]: DEFAULT_SIGILS[kind] };
  // Another kind may have taken the default meanwhile; then everything goes back.
  store.set(new Set(Object.values(sigils)).size === KINDS.length ? sigils : DEFAULT_SIGILS);
}

export function resetSigils() {
  store.set(DEFAULT_SIGILS);
}

/** The kind `char` opens, if any. */
export function sigilKind(char: string): SigilKind | undefined {
  return KINDS.find((k) => store.sigils[k] === char);
}
