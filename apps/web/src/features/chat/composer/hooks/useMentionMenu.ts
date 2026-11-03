import { useRef, useState } from "react";
import type { MentionSuggestion } from "~/features/chat/lib/composerExtensions";
import { DEFAULT_SIGILS, getSigils } from "~/lib/sigils";
import type { MentionItem } from "~/features/chat/composer/types";

/** Most files the @ menu offers at once. */
const MENTION_LIMIT = 8;

export interface MentionMenu {
  items: MentionItem[];
  index: number;
}

/**
 * The @ menu, which lists project files and folders. Its handlers run inside the editor, so state
 * they touch lives in refs as well as in React state, and the editor always sees the current
 * values.
 */
export function useMentionMenu(files: string[]) {
  const filesRef = useRef(files);
  filesRef.current = files;
  const [menu, setMenu] = useState<MentionMenu | null>(null);
  const items = useRef<MentionItem[]>([]);
  const index = useRef(0);
  const open = useRef(false);
  const pick = useRef<(item: MentionItem) => void>(() => undefined);

  function show(next: MentionItem[], onPick: (item: MentionItem) => void) {
    items.current = next;
    pick.current = onPick;
    setMenu({ items: next, index: index.current });
  }
  function move(next: number) {
    index.current = next;
    setMenu((prev) => (prev ? { ...prev, index: next } : prev));
  }
  function close() {
    open.current = false;
    setMenu(null);
  }

  const suggestion: MentionSuggestion = {
    char: DEFAULT_SIGILS.files,
    // Opens on the user's sigil for files; the mention is still sent with "@".
    findSuggestionMatch: ({ $position }) => {
      const text = $position.nodeBefore?.isText ? $position.nodeBefore.text : undefined;
      const sigil = getSigils().files;
      const at = text?.lastIndexOf(sigil) ?? -1;
      if (!text || at < 0 || (at > 0 && text[at - 1] !== " ")) return null;
      const word = text.slice(at);
      if (/\s/.test(word)) return null;
      const from = $position.pos - text.length + at;
      return { range: { from, to: $position.pos }, query: word.slice(1), text: word };
    },
    // Folders, written with a trailing "/", are listed with the files. Paths starting with what was
    // typed come first, then the shallower ones, so a folder drilled into lists what is directly in it.
    items: ({ query }) => {
      const q = query.toLowerCase();
      const folders = new Set<string>();
      for (const path of filesRef.current) {
        for (let at = path.indexOf("/"); at >= 0; at = path.indexOf("/", at + 1)) {
          folders.add(path.slice(0, at + 1));
        }
      }
      const rank = (path: string) => [
        Number(!path.toLowerCase().startsWith(q)),
        path.replace(/\/$/, "").split("/").length,
      ];
      return [...folders, ...filesRef.current]
        .filter((path) => path.toLowerCase().includes(q))
        .sort((a, b) => {
          const [ra, rb] = [rank(a), rank(b)];
          return ra[0] - rb[0] || ra[1] - rb[1];
        })
        .slice(0, MENTION_LIMIT)
        .map((path) => ({ id: path, label: path }));
    },
    // The menu's list and its keys.
    render: () => ({
      onStart: (props) => {
        open.current = true;
        index.current = 0;
        show(props.items, props.command);
      },
      onUpdate: (props) => {
        index.current = 0;
        show(props.items, props.command);
      },
      onKeyDown: ({ event, view, range }) => {
        const count = items.current.length;
        // Tab on a folder opens it: the menu lists what is in it, rather than mentioning it.
        const folder = items.current[index.current]?.id;
        if (event.key === "Tab" && folder?.endsWith("/")) {
          view.dispatch(view.state.tr.insertText(getSigils().files + folder, range.from, range.to));
          return true;
        }
        if (event.key === "ArrowDown" && count > 0) {
          move((index.current + 1) % count);
          return true;
        }
        if (event.key === "ArrowUp" && count > 0) {
          move((index.current + count - 1) % count);
          return true;
        }
        if ((event.key === "Enter" || event.key === "Tab") && count > 0) {
          const item = items.current[index.current];
          if (item) pick.current(item);
          return true;
        }
        if (event.key === "Escape") {
          close();
          return true;
        }
        return false;
      },
      onExit: close,
    }),
  };

  return {
    menu,
    suggestion,
    pick: (item: MentionItem) => pick.current(item),
    /** While the menu is open, its own handler gets the keys. With no file listed there is
     * nothing for Enter to pick, so it still sends. */
    ownsKey: (event: KeyboardEvent) =>
      open.current && (items.current.length > 0 || event.key !== "Enter"),
    hasFile: (path: string) => filesRef.current.includes(path),
  };
}
