import { useEffect, useMemo, useState } from "react";
import { ArrowCounterClockwiseIcon, XIcon } from "@phosphor-icons/react";
import { COMMANDS } from "~/lib/shortcutDefaults";
import {
  conflictsWith,
  formatKey,
  keyFor,
  keyFromEvent,
  keyProblem,
  resetKeybinding,
  resetKeybindings,
  setKeybinding,
  suspendShortcuts,
  useKeybindings,
} from "~/lib/shortcuts";

/** Keys the editor and the mouse own, so they cannot be remapped. */
const FIXED: { keys: string[]; action: string }[] = [
  { keys: ["Enter"], action: "Send the message" },
  { keys: ["Shift", "Enter"], action: "New line" },
  { keys: ["Up", "Down"], action: "Recall earlier messages from the first or last line" },
  { keys: [formatKey("mod")[0], "Click"], action: "Select or unselect a chat or task" },
];

function Keys({ parts }: { parts: string[] }) {
  return (
    <span className="flex flex-none items-center gap-1">
      {parts.map((part, i) => (
        <kbd key={i} className="kbd">
          {part}
        </kbd>
      ))}
    </span>
  );
}

export default function KeybindingsPane() {
  const { rules, overrides } = useKeybindings();
  const [query, setQuery] = useState("");
  // The command whose new key is being recorded, and why the last key tried was refused.
  const [recording, setRecording] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);

  // While recording, the next key press is the new shortcut: nothing else may act on it.
  useEffect(() => {
    if (!recording) return;
    const resume = suspendShortcuts();
    function onKey(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      if (event.isComposing) return;
      const key = keyFromEvent(event);
      if (!key) return;
      if (key === "escape") {
        setRecording(null);
        return;
      }
      const problem = keyProblem(recording!, key);
      if (problem) {
        setRefused(problem);
        return;
      }
      setKeybinding(recording!, key);
      setRecording(null);
    }
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      resume();
    };
  }, [recording]);

  const startRecording = (command: string) => {
    setRefused(null);
    setRecording((current) => (current === command ? null : command));
  };

  const groups = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const shown = COMMANDS.filter((c) => {
      if (!needle) return true;
      const key = keyFor(rules, c.id);
      const keyText = key ? formatKey(key).join(" ").toLowerCase() : "";
      return (
        c.title.toLowerCase().includes(needle) ||
        c.category.toLowerCase().includes(needle) ||
        keyText.includes(needle)
      );
    });
    const byCategory = new Map<string, typeof shown>();
    for (const c of shown) byCategory.set(c.category, [...(byCategory.get(c.category) ?? []), c]);
    return [...byCategory];
  }, [query, rules]);

  return (
    <>
      <div className="sticky top-0 z-10 -mx-5 mb-1 flex items-center gap-2 bg-elevated px-5 pb-2">
        <input
          className="input min-w-0 flex-1"
          placeholder="Search by name or key"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          spellCheck={false}
        />
        <button
          className="btn"
          disabled={Object.keys(overrides).length === 0}
          onClick={() => {
            setRecording(null);
            resetKeybindings();
          }}
        >
          Reset all
        </button>
      </div>
      <p className="mt-0 mb-2 text-[length:var(--fs-sm)] text-fg-muted">
        Click a shortcut, then press the new keys. Esc cancels.
      </p>
      {groups.length === 0 && <div className="py-4 text-fg-muted">No shortcuts match.</div>}
      {groups.map(([category, commands]) => (
        <section key={category} className="mb-3">
          <h3 className="m-0 mb-0.5 text-[length:var(--fs-sm)] font-medium text-fg-subtle">
            {category}
          </h3>
          {commands.map((c) => {
            const key = keyFor(rules, c.id);
            const changed = c.id in overrides;
            const isRecording = recording === c.id;
            const conflicts = conflictsWith(rules, c.id);
            return (
              <div key={c.id} className="group border-b border-border py-1.5">
                <div className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 text-fg-muted">{c.title}</span>
                  <span className="flex flex-none items-center justify-end">
                    {key && !isRecording && (
                      <button
                        className="btn-icon !h-6 !w-6 opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
                        title="Remove shortcut"
                        aria-label={`Remove shortcut for ${c.title}`}
                        onClick={() => setKeybinding(c.id, null)}
                      >
                        <XIcon size={12} />
                      </button>
                    )}
                    {changed && !isRecording && (
                      <button
                        className="btn-icon !h-6 !w-6"
                        title="Reset to default"
                        aria-label={`Reset shortcut for ${c.title}`}
                        onClick={() => resetKeybinding(c.id)}
                      >
                        <ArrowCounterClockwiseIcon size={12} />
                      </button>
                    )}
                  </span>
                  <button
                    type="button"
                    title="Change shortcut"
                    aria-label={`Change shortcut for ${c.title}`}
                    className={`flex cursor-pointer items-center rounded-[var(--radius-sm)] border px-2 py-1 ${isRecording ? "border-accent bg-accent-soft" : "border-transparent bg-transparent hover:bg-hover"}`}
                    onClick={() => startRecording(c.id)}
                  >
                    {isRecording ? (
                      <span className="text-[length:var(--fs-sm)] text-accent-text">
                        Press keys…
                      </span>
                    ) : key ? (
                      <Keys parts={formatKey(key)} />
                    ) : (
                      <span className="text-[length:var(--fs-sm)] text-fg-subtle">Unbound</span>
                    )}
                  </button>
                </div>
                {isRecording && refused && (
                  <div className="mt-0.5 text-right text-[length:var(--fs-sm)] text-danger">
                    {refused}
                  </div>
                )}
                {!isRecording && conflicts.length > 0 && (
                  <div className="mt-0.5 text-right text-[length:var(--fs-sm)] text-warning">
                    Also used by {conflicts.map((x) => x.title).join(", ")}
                  </div>
                )}
              </div>
            );
          })}
        </section>
      ))}
      {!query && (
        <section className="mb-1">
          <h3 className="m-0 mb-0.5 text-[length:var(--fs-sm)] font-medium text-fg-subtle">
            Fixed
          </h3>
          {FIXED.map(({ keys, action }) => (
            <div key={action} className="flex items-center gap-2 border-b border-border py-1.5">
              <span className="min-w-0 flex-1 text-fg-muted">{action}</span>
              {/* Inset by the editable rows' highlight box (padding plus border), so the keys line up. */}
              <span className="mr-[9px]">
                <Keys parts={keys} />
              </span>
            </div>
          ))}
        </section>
      )}
    </>
  );
}
