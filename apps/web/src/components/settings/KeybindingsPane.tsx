import { useEffect, useMemo, useState } from "react";
import { ArrowCounterClockwiseIcon, XIcon } from "@phosphor-icons/react";
import { COMMANDS, SHORTCUT_PAGES, type ShortcutPage } from "~/lib/shortcutDefaults";
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

const MOD = formatKey("mod")[0];

/** Keys the editor and the mouse own, so they cannot be remapped, listed on the page they belong to. */
const FIXED: Record<ShortcutPage, { keys: string[]; action: string }[]> = {
  Main: [],
  Chat: [],
  Sidebar: [{ keys: [MOD, "Click"], action: "Select or unselect a chat or task" }],
  Archive: [{ keys: [MOD, "Click"], action: "Select or unselect an archived chat" }],
  Composer: [
    { keys: ["Enter"], action: "Send the message" },
    { keys: ["Shift", "Enter"], action: "New line" },
    { keys: ["Up", "Down"], action: "Recall earlier messages from the first or last line" },
    { keys: ["Alt", "Up"], action: "Edit the latest queued message, with an empty composer" },
    { keys: [MOD, "Enter"], action: "Steer with the queued message, while the queue is open" },
  ],
};

/** The pages with something to show: a page whose shortcuts are all app-wide ones has no tab. */
const PAGES = SHORTCUT_PAGES.filter(
  (p) => FIXED[p].length > 0 || COMMANDS.some((c) => c.page === p && !c.fixed),
);

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
  const [page, setPage] = useState<ShortcutPage>(PAGES[0]);
  const [query, setQuery] = useState("");
  // The command whose new key is being recorded, and why the last key tried was refused.
  const [recording, setRecording] = useState<string | null>(null);
  // The fixed row last clicked, which says it cannot be changed.
  const [fixedNote, setFixedNote] = useState<string | null>(null);
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

  const selectPage = (next: ShortcutPage) => {
    setRecording(null);
    setFixedNote(null);
    setPage(next);
  };

  // A search looks across every page and names each; otherwise only the open page is shown.
  const searching = query.trim() !== "";
  const sections = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const matches = (...texts: (string | null | undefined)[]) =>
      texts.some((text) => text?.toLowerCase().includes(needle));
    return PAGES.filter((p) => searching || p === page).flatMap((p) => {
      const commands = COMMANDS.filter((c) => {
        if (c.page !== p || c.fixed) return false;
        const key = keyFor(rules, c.id);
        return !searching || matches(c.title, c.description, p, key && formatKey(key).join(" "));
      });
      const fixed = FIXED[p].filter((f) => !searching || matches(f.action, p, f.keys.join(" ")));
      return commands.length + fixed.length > 0 ? [{ page: p, commands, fixed }] : [];
    });
  }, [page, query, rules, searching]);

  return (
    <>
      <div className="sticky top-0 z-10 -mx-5 mb-1 bg-elevated px-5 pb-1">
        <div className="mb-2 flex items-center gap-2">
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
        {!searching && (
          <div className="flex gap-1">
            {PAGES.map((title) => (
              <button
                key={title}
                className={`h-7 cursor-pointer rounded-[var(--radius-sm)] border-none px-2.5 text-[length:var(--fs-sm)] ${title === page ? "bg-selected text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
                onClick={() => selectPage(title)}
              >
                {title}
              </button>
            ))}
          </div>
        )}
      </div>
      {sections.some((s) => s.commands.length > 0) && (
        <p className="mt-1 mb-1 text-[length:var(--fs-sm)] text-fg-muted">
          Click a shortcut, then press the new keys. Esc cancels.
        </p>
      )}
      {sections.length === 0 && <div className="py-4 text-fg-muted">No shortcuts match.</div>}
      {sections.map(({ page: sectionPage, commands, fixed }) => (
        <section key={sectionPage} className="mb-3">
          {searching && (
            <h3 className="m-0 mt-2 mb-0.5 text-[length:var(--fs-sm)] font-medium text-fg-subtle">
              {sectionPage}
            </h3>
          )}
          {commands.map((c) => {
            const key = keyFor(rules, c.id);
            const changed = c.id in overrides;
            const isRecording = recording === c.id;
            const conflicts = conflictsWith(rules, c.id);
            return (
              <div key={c.id} className="group border-b border-border py-1.5">
                <div className="flex items-center gap-2">
                  <span className="min-w-0 flex-1">
                    <span className="block text-fg">{c.title}</span>
                    {c.description && (
                      <span className="block text-[length:var(--fs-sm)] text-fg-muted">
                        {c.description}
                      </span>
                    )}
                  </span>
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
                    Also used by {conflicts.map((x) => `${x.page}: ${x.title}`).join(", ")}
                  </div>
                )}
              </div>
            );
          })}
          {fixed.map(({ keys, action }) => (
            <div key={action} className="border-b border-border py-1.5">
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1 text-fg-muted">{action}</span>
                {/* The same box as an editable row's keys, so the keys line up and highlight alike. */}
                <button
                  type="button"
                  title="Fixed: this shortcut cannot be changed"
                  aria-label={`${action}: fixed shortcut`}
                  className="flex cursor-pointer items-center rounded-[var(--radius-sm)] border border-transparent bg-transparent px-2 py-1 hover:bg-hover"
                  onClick={() => setFixedNote((current) => (current === action ? null : action))}
                >
                  <Keys parts={keys} />
                </button>
              </div>
              {fixedNote === action && (
                <div className="mt-0.5 text-right text-[length:var(--fs-sm)] text-fg-subtle">
                  Fixed: this shortcut cannot be changed
                </div>
              )}
            </div>
          ))}
        </section>
      ))}
    </>
  );
}
