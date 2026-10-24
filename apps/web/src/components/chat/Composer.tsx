import { useEffect, useRef, useState } from "react";
import { ArrowUpIcon, CaretDownIcon, CheckIcon, SquareIcon } from "@phosphor-icons/react";
import { EditorContent, useEditor } from "@tiptap/react";
import { Fragment, Slice } from "@tiptap/pm/model";
import { TextSelection } from "@tiptap/pm/state";
import type { EditorView } from "@tiptap/pm/view";
import {
  api,
  type AccessMode,
  type AgentKind,
  type Attachment,
  type ModelOption,
  type ProviderSlashCommand,
} from "~/api";
import AccessPicker from "~/components/chat/AccessPicker";
import { detectLanguage, isTerminalOutput } from "~/components/chat/languageDetection";
import EffortPicker from "~/components/chat/EffortPicker";
import AttachmentChip from "~/components/chat/AttachmentChip";
import { commandScore, SLASH_COMMANDS } from "~/components/chat/composerActions";
import {
  composerExtensions,
  continueList,
  keepDraftText,
  type MentionSuggestion,
} from "~/components/chat/composerExtensions";
import FormatBar from "~/components/chat/FormatBar";
import ModelPicker from "~/components/chat/ModelPicker";

interface ComposerProps {
  /** The draft as Markdown. */
  value: string;
  onChange: (text: string) => void;
  /** Sends the draft; `queue` holds it for after the running turn instead of steering the turn. */
  onSubmit: (queue: boolean) => void;
  /** Promotes the first queued message while preserving the current draft. */
  onSteerQueued: () => void;
  onEditLatestQueued: () => void;
  canSteerQueued: boolean;
  editingQueued: boolean;
  canQueueFollowUp: boolean;
  onStop: () => void;
  working: boolean;
  error: string | null;
  agent: AgentKind;
  model: string | null;
  /** Agents whose models are offered (only the thread's own once it has started). */
  agents: AgentKind[];
  /** Picking another agent's model switches the agent too. */
  onPickModel: (agent: AgentKind, model: string | null) => void;
  modelPickerRequest: number;
  providerSlashCommands: ProviderSlashCommand[];
  onSelectSlashCommand: (name: string) => boolean;
  accessMode: AccessMode;
  onPickAccess: (mode: AccessMode) => void;
  effort: string | null;
  onPickEffort: (effort: string | null) => void;
  attachments: Attachment[];
  onRemoveAttachment: (index: number) => void;
  /** A drag is hovering over the chat; the composer shows where it will land. */
  dropping: boolean;
  /** The thread's goal is active: its border turns instead of holding still. */
  goalActive: boolean;
  /** The chat's earlier messages, oldest first, recalled with the arrow keys. */
  history: HistoryEntry[];
  /** Project files an @ mention can refer to, relative to the project root. */
  files: string[];
  /** A choice panel is open: the editor is read-only until the user picks writing their own answer. */
  deferFocus: boolean;
  /** Nothing can be written or sent, such as while the project's folder is missing. */
  disabled: boolean;
}

export interface HistoryEntry {
  id: string;
  text: string;
}

/** The recalled history entry, while the draft still holds it unchanged. */
interface Recalled {
  id: string;
  text: string;
}

/** Most characters a prompt can hold; a longer draft cannot be sent. */
const PROMPT_LIMIT = 120_000;

/** An entry in the @ menu. */
interface MentionItem {
  id: string;
  label: string;
}

interface MentionMenu {
  items: MentionItem[];
  index: number;
}

/** Most files the @ menu offers at once. */
const MENTION_LIMIT = 8;

const EDITOR_CLASS =
  "min-h-16 max-h-[280px] overflow-y-auto whitespace-pre-wrap break-words px-1.5 text-fg text-[length:var(--fs-md)] leading-[20px] outline-none selectable [&_p]:m-0 [&_ul]:my-0 [&_ol]:my-0 [&_ul]:pl-5 [&_ol]:pl-5 [&_pre]:my-1 [&_pre]:rounded-[var(--radius-sm)] [&_pre]:bg-panel [&_pre]:px-2.5 [&_pre]:py-1.5 [&_pre]:font-[family-name:var(--font-mono)] [&_pre]:text-[12px] [&_code]:font-[family-name:var(--font-mono)] [&_code]:text-[12px]";

/** Whether copied HTML is styled monospace text, as terminals and code editors copy it,
 * rather than a document with paragraphs, lists or headings. */
function isMonospaceHtml(html: string): boolean {
  const body = new DOMParser().parseFromString(html, "text/html").body;
  if (body.querySelector("p, h1, h2, h3, h4, h5, h6, li, table, blockquote")) return false;
  return /<pre[\s>]|monospace|white-space:\s*pre/i.test(html);
}

/** Characters a line of prose is made of: letters, digits, spaces and sentence punctuation. */
const PROSE_CHAR = /[\p{L}\p{N}\s.,!?'"’“”:()\-–—/]/u;

/**
 * Whether plain text reads as sentences rather than code or output: no line is indented, symbol
 * heavy, or terminal output, and most lines are long or end like a sentence.
 */
function isProse(text: string): boolean {
  if (isTerminalOutput(text)) return false;
  const lines = text.split("\n").filter((line) => line.trim());
  let sentences = 0;
  let words = 0;
  for (const line of lines) {
    if (/^\s/.test(line)) return false;
    const chars = [...line];
    if (chars.filter((c) => PROSE_CHAR.test(c)).length < chars.length * 0.97) return false;
    const count = line.split(/\s+/).filter((word) => /\p{L}/u.test(word)).length;
    words += count;
    if (count >= 8 || /[.!?:,]$/.test(line)) sentences++;
  }
  return words >= 8 && sentences * 2 >= lines.length;
}

/** VS Code language ids that code blocks name differently. */
const VSCODE_LANGUAGES: Record<string, string | null> = {
  typescriptreact: "tsx",
  javascriptreact: "jsx",
  shellscript: "bash",
  jsonc: "json",
  makefile: "make",
  plaintext: null,
};

/** The language of a copy from VS Code's editor, from its clipboard metadata, or null. */
function vscodeLanguage(metadata: string | null): string | null {
  try {
    const mode = (JSON.parse(metadata ?? "") as { mode?: unknown }).mode;
    if (typeof mode !== "string") return null;
    return mode in VSCODE_LANGUAGES ? VSCODE_LANGUAGES[mode] : mode;
  } catch {
    return null;
  }
}

/** Short language names that web pages use in class names, by the id code blocks use. */
const LANGUAGE_ALIASES: Record<string, string> = {
  js: "javascript",
  ts: "typescript",
  py: "python",
  rs: "rust",
  sh: "bash",
  shell: "bash",
  yml: "yaml",
};

/**
 * The language a web page gave copied code, from a `language-rust`, `lang-rust` or
 * `highlight-source-rust` class in the copied HTML, or null.
 */
function htmlLanguage(html: string): string | null {
  const match = /class="[^"]*\b(?:language|lang|highlight-source)-([\w+#-]+)/i.exec(html);
  if (!match) return null;
  const id = match[1].toLowerCase();
  return LANGUAGE_ALIASES[id] ?? id;
}

/**
 * Labels a pasted code block once the block is in. A copy from VS Code or another Electron editor
 * names its language in clipboard metadata that the webview does not show to the page, so the app
 * reads it from the system clipboard. Other code gets the language VS Code would detect for it.
 */
function labelPastedCode(view: EditorView, text: string) {
  void api
    .clipboardEditorData()
    .catch(() => null)
    .then((metadata) => vscodeLanguage(metadata) ?? detectLanguage(text))
    .then((language) => {
      if (!language || view.isDestroyed) return;
      let at: number | null = null;
      view.state.doc.descendants((node, pos) => {
        if (at === null && node.type.name === "codeBlock" && !node.attrs.language) {
          if (node.textContent === text) at = pos;
        }
        return at === null;
      });
      if (at !== null) view.dispatch(view.state.tr.setNodeAttribute(at, "language", language));
    });
}

/** Models per agent, fetched once per agent kind. */
function useModels(agents: AgentKind[]): Partial<Record<AgentKind, ModelOption[]>> {
  const [models, setModels] = useState<Partial<Record<AgentKind, ModelOption[]>>>({});
  const wanted = agents.join(",");
  useEffect(() => {
    let cancelled = false;
    for (const kind of wanted.split(",") as AgentKind[]) {
      api.listModels(kind).then(
        (list) => {
          if (!cancelled) setModels((prev) => (prev[kind] ? prev : { ...prev, [kind]: list }));
        },
        () => {},
      );
    }
    return () => {
      cancelled = true;
    };
  }, [wanted]);
  return models;
}

export default function Composer({
  value,
  onChange,
  onSubmit,
  onSteerQueued,
  onEditLatestQueued,
  canSteerQueued,
  editingQueued,
  canQueueFollowUp,
  onStop,
  working,
  error,
  agent,
  model,
  agents,
  onPickModel,
  modelPickerRequest,
  providerSlashCommands,
  onSelectSlashCommand,
  accessMode,
  onPickAccess,
  effort,
  onPickEffort,
  attachments,
  onRemoveAttachment,
  dropping,
  goalActive,
  history,
  files,
  deferFocus,
  disabled,
}: ComposerProps) {
  const models = useModels(agents);
  // The slash menu lists commands for a draft that is still one "/word"; Escape hides it
  // until the draft changes again.
  const [menuClosed, setMenuClosed] = useState(false);
  const [dispatchMenuOpen, setDispatchMenuOpen] = useState(false);
  const [commandIndex, setCommandIndex] = useState(0);
  // Antigravity and OpenCode read the message as a prompt, so there are no commands to offer.
  const slash =
    agent !== "antigravity" && agent !== "opencode" && /^\/\S*$/.test(value)
      ? value.toLowerCase()
      : null;
  const builtInNames = new Set(SLASH_COMMANDS.map((command) => command.name.slice(1)));
  const commands = [
    ...SLASH_COMMANDS,
    ...providerSlashCommands
      .filter((command) => !builtInNames.has(command.name.replace(/^\//, "")))
      .map((command) => ({
        name: command.name.startsWith("/") ? command.name : `/${command.name}`,
        description: command.description ?? "Claude Code command",
      })),
  ];
  const matches =
    slash === null
      ? []
      : commands
          .flatMap((c) => {
            const score = commandScore(c.name, slash.slice(1));
            return score === null ? [] : [{ ...c, score }];
          })
          .sort((a, b) => a.score - b.score);
  const menuOpen = !menuClosed && slash !== null;
  const activeCommand = matches[Math.min(commandIndex, matches.length - 1)];
  const commandOptionRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (menuOpen) commandOptionRef.current?.scrollIntoView({ block: "nearest" });
  }, [menuOpen, commandIndex, activeCommand?.name]);

  const overBy = value.length - PROMPT_LIMIT;

  // The @ menu lists project files. Its handlers run inside the editor, so state they touch
  // lives in refs as well as in React state, and the editor always sees the current values.
  const filesRef = useRef(files);
  filesRef.current = files;
  const [mention, setMention] = useState<MentionMenu | null>(null);
  const mentionItems = useRef<MentionItem[]>([]);
  const mentionIndex = useRef(0);
  const mentionOpen = useRef(false);
  const mentionPick = useRef<(item: MentionItem) => void>(() => undefined);

  function showMention(items: MentionItem[], pick: (item: MentionItem) => void) {
    mentionItems.current = items;
    mentionPick.current = pick;
    setMention({ items, index: mentionIndex.current });
  }
  function moveMention(index: number) {
    mentionIndex.current = index;
    setMention((prev) => (prev ? { ...prev, index } : prev));
  }
  function closeMention() {
    mentionOpen.current = false;
    setMention(null);
  }

  const mentionSuggestion: MentionSuggestion = {
    char: "@",
    items: ({ query }) => {
      const q = query.toLowerCase();
      return filesRef.current
        .filter((path) => path.toLowerCase().includes(q))
        .sort(
          (a, b) => Number(!a.toLowerCase().startsWith(q)) - Number(!b.toLowerCase().startsWith(q)),
        )
        .slice(0, MENTION_LIMIT)
        .map((path) => ({ id: path, label: path }));
    },
    render: mentionMenuRender,
  };

  // The @ menu's list and its keys.
  function mentionMenuRender(): ReturnType<NonNullable<MentionSuggestion["render"]>> {
    return {
      onStart: (props) => {
        mentionOpen.current = true;
        mentionIndex.current = 0;
        showMention(props.items, props.command);
      },
      onUpdate: (props) => {
        mentionIndex.current = 0;
        showMention(props.items, props.command);
      },
      onKeyDown: ({ event }) => {
        const count = mentionItems.current.length;
        if (event.key === "ArrowDown" && count > 0) {
          moveMention((mentionIndex.current + 1) % count);
          return true;
        }
        if (event.key === "ArrowUp" && count > 0) {
          moveMention((mentionIndex.current + count - 1) % count);
          return true;
        }
        if ((event.key === "Enter" || event.key === "Tab") && count > 0) {
          const item = mentionItems.current[mentionIndex.current];
          if (item) mentionPick.current(item);
          return true;
        }
        if (event.key === "Escape") {
          closeMention();
          return true;
        }
        return false;
      },
      onExit: closeMention,
    };
  }

  // Event handlers are read through refs, so the editor always calls the current ones.
  const changed = useRef(onChange);
  changed.current = onChange;
  const keyDown = useRef<(event: KeyboardEvent) => boolean>(() => false);

  // Holds the next message for after the running turn. Cleared by each send.
  // Match T3's default follow-up behavior: a message sent during generation queues.
  const [queueNext, setQueueNext] = useState(true);
  // While an autonomous goal is running, new follow-ups wait by default. Sending remains
  // explicit: Enter or the send button enqueues the current draft, and the user can switch to
  // Steer before sending when they want to interrupt the current step.
  useEffect(() => {
    setQueueNext(true);
  }, [working, goalActive]);

  const editor = useEditor({
    extensions: composerExtensions("Reply or describe the next change…", mentionSuggestion),
    content: value,
    contentType: "markdown",
    immediatelyRender: false,
    editorProps: {
      attributes: { class: EDITOR_CLASS },
      handleKeyDown: (_view, event) => keyDown.current(event),
      // Pastes go in as plain text; copied HTML is only read to tell what was copied, never
      // parsed into the draft. A copy from this editor keeps its blocks and mentions.
      handlePaste: (view, event) => {
        const data = event.clipboardData;
        if (!data) return false;
        const html = data.getData("text/html");
        if (html.includes("data-pm-slice")) return false;
        const text = data.getData("text/plain").replace(/\r\n?/g, "\n").replace(/\n+$/, "");
        if (!text) return true;
        const { state } = view;
        // Inside a code block, and on a single line, the text goes in exactly as copied.
        if (state.selection.$from.parent.type.spec.code || !text.includes("\n")) {
          view.dispatch(state.tr.insertText(text).scrollIntoView());
          return true;
        }
        const language = vscodeLanguage(data.getData("vscode-editor-data")) ?? htmlLanguage(html);
        // Multi-line text from a page or document, or plain text that reads as sentences, keeps
        // its paragraphs and line breaks.
        if ((html && !isMonospaceHtml(html)) || (!language && isProse(text))) {
          const paragraphs = text.split(/\n{2,}/).map((paragraph) =>
            state.schema.nodes.paragraph.create(
              null,
              paragraph
                .split("\n")
                .flatMap((line, i) => [
                  ...(i > 0 ? [state.schema.nodes.hardBreak.create()] : []),
                  ...(line ? [state.schema.text(line)] : []),
                ]),
            ),
          );
          const slice = new Slice(Fragment.from(paragraphs), 1, 1);
          view.dispatch(state.tr.replaceSelection(slice).scrollIntoView());
          return true;
        }
        // Terminal output and code go in as one code block, in the language VS Code or the web
        // page gave the copy.
        const block = state.schema.nodes.codeBlock.create({ language }, state.schema.text(text));
        const tr = state.tr.replaceSelectionWith(block);
        // The caret goes to the line after the block, which is added when there is none, so
        // typing goes on outside the pasted code.
        const $end = tr.selection.$to;
        const after = $end.after($end.depth);
        if (!tr.doc.resolve(after).nodeAfter?.isTextblock) {
          tr.insert(after, state.schema.nodes.paragraph.create());
        }
        tr.setSelection(TextSelection.create(tr.doc, after + 1));
        view.dispatch(tr.scrollIntoView());
        if (!language) labelPastedCode(view, text);
        return true;
      },
    },
    onCreate: ({ editor }) => {
      if (editor.markdown) keepDraftText(editor.markdown);
    },
    onUpdate: ({ editor }) => {
      setMenuClosed(false);
      setCommandIndex(0);
      changed.current(editor.getMarkdown());
    },
  });

  // A draft changed from outside (history, a completed command, sending) loads into the editor.
  // Edits the user typed already match, so typing never reloads the document.
  useEffect(() => {
    if (!editor || editor.getMarkdown() === value) return;
    editor.commands.setContent(value, { contentType: "markdown", emitUpdate: false });
    if (editor.isFocused) editor.commands.focus("end");
  }, [editor, value]);

  // Opening a chat puts the caret in the composer, unless the user is typing in another field,
  // the task list keeps the focus (the chat opened from it), or a choice panel is open.
  const deferFocusRef = useRef(deferFocus || disabled);
  deferFocusRef.current = deferFocus || disabled;
  useEffect(() => {
    const active = document.activeElement;
    const focusStays =
      active instanceof HTMLElement &&
      (active.isContentEditable ||
        active.matches("input, textarea, select") ||
        active.closest("[data-keeps-focus]") !== null);
    if (editor && !focusStays && !deferFocusRef.current) editor.commands.focus("end");
  }, [editor]);

  // The Stop button holds focus once clicked, and it leaves the page when the turn or goal ends,
  // which drops focus to the body. Focus the editor then, unless the user moved elsewhere or the
  // reply offered choices.
  const wasWorking = useRef(working);
  useEffect(() => {
    if (
      wasWorking.current &&
      !working &&
      editor &&
      !deferFocus &&
      !disabled &&
      document.activeElement === document.body
    ) {
      editor.commands.focus("end");
    }
    wasWorking.current = working;
  }, [editor, working, deferFocus, disabled]);

  // A choice panel takes the keys: the editor is read-only and unfocused while it is open, and
  // takes the focus when it closes so the user can write their own answer. A disabled composer
  // is read-only the same way.
  const readOnly = deferFocus || disabled;
  const wasDeferred = useRef(readOnly);
  useEffect(() => {
    if (!editor) return;
    editor.setEditable(!readOnly, false);
    if (readOnly && editor.isFocused) editor.commands.blur();
    else if (!readOnly && wasDeferred.current) editor.commands.focus("end");
    wasDeferred.current = readOnly;
  }, [editor, readOnly]);

  const recalled = useRef<Recalled | null>(null);

  function complete(name: string) {
    if (!editor) return;
    // The menu only shows for a bare "/word", so the whole draft is replaced by the command.
    editor
      .chain()
      .focus()
      .selectAll()
      .deleteSelection()
      .insertContent(
        name === "/goal" ? [{ type: "goalCommand" }, { type: "text", text: " " }] : `${name} `,
      )
      .run();
    changed.current(editor.getMarkdown());
  }

  function selectCommand(name: string) {
    if (onSelectSlashCommand(name)) {
      editor?.chain().focus().clearContent().run();
      changed.current("");
      return;
    }
    complete(name);
  }

  /** Up and Down recall earlier messages only from the first or last line of the draft, counting
   * wrapped lines. Anywhere else they move the caret a line at a time, so a multi-line draft or a
   * recalled message can be walked through before the next one is reached. */
  function stepHistory(event: KeyboardEvent, ed: NonNullable<typeof editor>): boolean {
    const back = event.key === "ArrowUp";
    if (
      (!back && event.key !== "ArrowDown") ||
      event.shiftKey ||
      event.altKey ||
      event.metaKey ||
      event.ctrlKey
    ) {
      return false;
    }
    if (attachments.length > 0) return false;
    const { selection, doc } = ed.state;
    if (!selection.empty) return false;
    const at = recalled.current?.text === value ? recalled.current : null;
    // The caret must be in the first (Up) or last (Down) block, on that block's top or bottom line.
    let edgeBlock = -1;
    doc.descendants((node, pos) => {
      if (node.isTextblock && (!back || edgeBlock < 0)) edgeBlock = pos;
      return !node.isTextblock;
    });
    if (selection.$from.before() !== edgeBlock) return false;
    if (!ed.view.endOfTextblock(back ? "up" : "down")) return false;
    const index = at ? history.findIndex((h) => h.id === at.id) : -1;
    if (index < 0 && (value.length > 0 || !back)) return false;
    const next = back ? history[index < 0 ? history.length - 1 : index - 1] : history[index + 1];
    if (back && !next) return false;
    recalled.current = next ?? null;
    changed.current(next?.text ?? "");
    return true;
  }

  function handleKey(event: KeyboardEvent): boolean {
    if (!editor || event.isComposing) return false;
    // Same as clicking the queued message's Steer button. The draft in the composer is not sent.
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey) && canSteerQueued) {
      onSteerQueued();
      return true;
    }
    // While the @ menu is open, the editor's suggestion handler gets the keys.
    if (mentionOpen.current) return false;
    if (!menuOpen && stepHistory(event, editor)) return true;
    if (menuOpen) {
      if (event.key === "Escape") {
        setMenuClosed(true);
        return true;
      }
      if (event.key === "ArrowDown" && matches.length > 0) {
        event.preventDefault();
        setCommandIndex((i) => (i + 1) % matches.length);
        return true;
      }
      if (event.key === "ArrowUp" && matches.length > 0) {
        event.preventDefault();
        setCommandIndex((i) => (i + matches.length - 1) % matches.length);
        return true;
      }
      // Enter on a command typed in full sends it (a bare "/goal" shows the goal).
      const typed = activeCommand !== undefined && value.toLowerCase() === activeCommand.name;
      if (
        activeCommand &&
        (event.key === "Tab" || (event.key === "Enter" && !typed && !event.shiftKey))
      ) {
        complete(activeCommand.name);
        return true;
      }
    }
    if (event.key === "ArrowUp" && event.altKey && !value.trim()) {
      onEditLatestQueued();
      return true;
    }
    // Escape leaves the composer, so the page's keys work again.
    if (event.key === "Escape") {
      editor.commands.blur();
      return true;
    }
    // Enter sends, unless the draft is over the limit; Shift+Enter continues a list, or starts a
    // new line (the editor's default) elsewhere.
    if (event.key === "Enter" && !event.shiftKey) {
      if (overBy <= 0) sendDraft(working && (event.metaKey || event.ctrlKey));
      return true;
    }
    if (event.key === "Enter" && continueList(editor)) return true;
    return false;
  }
  keyDown.current = handleKey;

  function sendDraft(alternate = false) {
    const queue = working && (alternate ? !queueNext : queueNext);
    if (queue && !canQueueFollowUp && !editingQueued) return;
    onSubmit(queue);
    setQueueNext(true);
  }

  // Picks how the next send is dispatched. Enter and the send button use it; nothing is sent here.
  function chooseMode(queue: boolean) {
    setQueueNext(queue);
    setDispatchMenuOpen(false);
  }

  const surface = dropping
    ? "border border-dashed border-accent bg-editor"
    : goalActive
      ? "goal-border"
      : disabled
        ? "border border-border-strong bg-editor"
        : "border border-border-strong bg-editor focus-within:border-accent";
  return (
    <div
      className={`relative flex flex-col gap-2 rounded-[8px] pt-3.5 pr-3 pb-2.5 pl-3 ${surface}`}
      // The frame's padding and gaps belong to the text field: a click there keeps the caret in
      // the draft, so Ctrl+A still selects the draft.
      onMouseDown={(e) => {
        if (e.target !== e.currentTarget || !editor) return;
        e.preventDefault();
        if (!editor.isFocused) editor.commands.focus("end");
      }}
    >
      {menuOpen && (
        <div
          role="listbox"
          aria-label="Slash commands"
          className="absolute right-0 bottom-full left-0 mb-1.5 flex max-h-[min(50vh,32rem)] flex-col gap-px overflow-y-auto overscroll-contain rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]"
        >
          {matches.length === 0 && (
            <div className="px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted">
              No matching command.
            </div>
          )}
          {matches.map((c) => (
            <button
              key={c.name}
              ref={c.name === activeCommand?.name ? commandOptionRef : undefined}
              role="option"
              aria-selected={c.name === activeCommand?.name}
              className={`flex w-full flex-none cursor-pointer items-baseline gap-2.5 rounded-[var(--radius)] border-none px-2 py-1.5 text-left ${c.name === activeCommand?.name ? "bg-selected" : "bg-transparent hover:bg-hover"}`}
              // Keeps the focus in the editor.
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => selectCommand(c.name)}
            >
              <span className="font-[family-name:var(--font-mono)] text-fg">{c.name}</span>
              <span className="text-[length:var(--fs-sm)] text-fg-muted">{c.description}</span>
            </button>
          ))}
        </div>
      )}
      {mention && (
        <div className="absolute right-0 bottom-full left-0 mb-1.5 flex flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]">
          {mention.items.length === 0 && (
            <div className="px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted">
              No matching files
            </div>
          )}
          {mention.items.map((item, i) => (
            <button
              key={item.id}
              className={`flex w-full cursor-pointer items-baseline rounded-[var(--radius)] border-none px-2 py-1.5 text-left font-[family-name:var(--font-mono)] text-[length:var(--fs-sm)] text-fg ${i === mention.index ? "bg-selected" : "bg-transparent hover:bg-hover"}`}
              // Keeps the focus in the editor.
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => mentionPick.current(item)}
            >
              {item.label}
            </button>
          ))}
        </div>
      )}
      {attachments.length > 0 && (
        <div className="flex flex-wrap gap-1.5 px-1.5">
          {attachments.map((a, i) => (
            <AttachmentChip key={a.path} attachment={a} onRemove={() => onRemoveAttachment(i)} />
          ))}
        </div>
      )}
      <EditorContent editor={editor} className={`composer-editor min-w-0 `} />
      <div className="@container flex flex-wrap items-center gap-1.5">
        <ModelPicker
          agent={agent}
          model={model}
          agents={agents}
          models={models}
          onPick={onPickModel}
          openRequest={modelPickerRequest}
        />
        <EffortPicker agent={agent} effort={effort} onPick={onPickEffort} />
        <AccessPicker mode={accessMode} onPick={onPickAccess} />
        <span className="flex-1" />
        {/* Hidden when the composer is too narrow for the pickers and the formatting buttons together. */}
        <div className="hidden @[640px]:block">
          <FormatBar editor={editor} />
        </div>
        {working && (
          <button
            className="grid h-8 w-8 flex-none cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-selected-muted text-fg enabled:hover:bg-accent-hover"
            onClick={onStop}
            title="Stop"
            aria-label="Stop"
          >
            <SquareIcon size={12} weight="fill" />
          </button>
        )}
        {(!working || value.trim() || attachments.length > 0) && (
          <div className="relative flex flex-none items-center">
            <button
              className={`grid h-8 w-8 cursor-pointer place-items-center ${working && !editingQueued ? "rounded-l-[var(--radius)]" : "rounded-[var(--radius)]"} border-none bg-accent text-on-accent enabled:hover:bg-accent-hover disabled:cursor-default disabled:bg-selected-muted disabled:text-fg-subtle`}
              onClick={() => sendDraft()}
              disabled={
                disabled ||
                (!value.trim() && attachments.length === 0) ||
                overBy > 0 ||
                (working && queueNext && !canQueueFollowUp && !editingQueued)
              }
              title={
                editingQueued
                  ? "Update queued message"
                  : working
                    ? queueNext
                      ? "Queue for after the turn"
                      : "Steer the running turn"
                    : "Send"
              }
              aria-label={
                editingQueued
                  ? "Update queued message"
                  : working
                    ? queueNext
                      ? "Queue message"
                      : "Steer message"
                    : "Send"
              }
            >
              {editingQueued ? <CheckIcon size={16} /> : <ArrowUpIcon size={16} />}
            </button>
            {working && !editingQueued && (
              <>
                <button
                  type="button"
                  className="grid h-8 w-6 cursor-pointer place-items-center rounded-r-[var(--radius)] border-0 border-l border-l-on-accent/20 bg-accent text-on-accent hover:bg-accent-hover"
                  onClick={() => setDispatchMenuOpen((open) => !open)}
                  disabled={overBy > 0 || (!value.trim() && attachments.length === 0)}
                  aria-expanded={dispatchMenuOpen}
                  aria-label="Choose queue or steer"
                  title="Choose queue or steer"
                >
                  <CaretDownIcon size={12} />
                </button>
                {dispatchMenuOpen && (
                  <div className="absolute right-0 bottom-full z-50 mb-1.5 flex min-w-48 flex-col gap-px rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]">
                    <button
                      disabled={overBy > 0}
                      aria-pressed={!queueNext}
                      className="w-full cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-2.5 py-2 text-left text-fg hover:bg-hover disabled:cursor-default disabled:opacity-40"
                      onClick={() => chooseMode(false)}
                    >
                      <span className="flex items-center gap-2">
                        <span className="grid w-3.5 flex-none place-items-center">
                          {!queueNext && <CheckIcon size={14} />}
                        </span>
                        Steer current turn
                      </span>
                      <span className="mt-0.5 block pl-[22px] text-[length:var(--fs-xs)] text-fg-subtle">
                        Send the next message into the active turn
                      </span>
                    </button>
                    <button
                      disabled={overBy > 0 || !canQueueFollowUp}
                      aria-pressed={queueNext}
                      className="w-full cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-2.5 py-2 text-left text-fg hover:bg-hover disabled:cursor-default disabled:opacity-40"
                      onClick={() => chooseMode(true)}
                    >
                      <span className="flex items-center gap-2">
                        <span className="grid w-3.5 flex-none place-items-center">
                          {queueNext && <CheckIcon size={14} />}
                        </span>
                        Queue for next turn
                      </span>
                      <span className="mt-0.5 block pl-[22px] text-[length:var(--fs-xs)] text-fg-subtle">
                        {canQueueFollowUp
                          ? "Wait until the active turn finishes"
                          : "Edit or remove the queued message first"}
                      </span>
                    </button>
                  </div>
                )}
              </>
            )}
          </div>
        )}
      </div>
      {error && <div className="text-[length:var(--fs-sm)] text-danger">{error}</div>}
      {overBy > 0 && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {`Prompt is ${overBy.toLocaleString("en-US")} ${overBy === 1 ? "character" : "characters"} over the ${PROMPT_LIMIT.toLocaleString("en-US")}-character limit. Shorten or split it before sending.`}
        </div>
      )}
    </div>
  );
}
