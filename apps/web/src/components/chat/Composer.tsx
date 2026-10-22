import { useEffect, useRef, useState } from "react";
import { ArrowUpIcon, CaretDownIcon, CheckIcon, SquareIcon } from "@phosphor-icons/react";
import { EditorContent, useEditor } from "@tiptap/react";
import {
  api,
  type AccessMode,
  type AgentKind,
  type Attachment,
  type ModelOption,
  type ProviderSlashCommand,
} from "~/api";
import AccessPicker from "~/components/chat/AccessPicker";
import EffortPicker from "~/components/chat/EffortPicker";
import AttachmentChip from "~/components/chat/AttachmentChip";
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
  /** The queued strip is expanded, so Ctrl+Enter presses its Steer button. */
  queueOpen: boolean;
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

/** Slash commands offered while the draft is a single "/word". */
const COMMANDS = [
  { name: "/model", description: "Switch the response model for this chat" },
  { name: "/plan", description: "Switch this chat into plan mode" },
  { name: "/default", description: "Switch this chat back to normal mode" },
  { name: "/goal", description: "Keep working until a condition is met" },
  { name: "/new", description: "Start a fresh chat in this project" },
  { name: "/clear", description: "Delete this chat's messages and start the agent fresh" },
  { name: "/compact", description: "Ask Claude to summarise the conversation to free context" },
];

/** Most characters a prompt can hold; a longer draft cannot be sent. */
const PROMPT_LIMIT = 120_000;

/**
 * How well a command name matches the typed query (without the "/"): lower is better, and null
 * drops the command. Prefixes rank above matches that start a word, which rank above any match.
 */
function commandScore(name: string, query: string): number | null {
  const bare = name.slice(1);
  if (bare === query) return 0;
  if (bare.startsWith(query)) return 2;
  for (let i = bare.indexOf(query, 1); i > 0; i = bare.indexOf(query, i + 1)) {
    if ("-_/".includes(bare[i - 1])) return 4;
  }
  return bare.includes(query) ? 6 : null;
}

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
  queueOpen,
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
}: ComposerProps) {
  const models = useModels(agents);
  // The slash menu lists commands for a draft that is still one "/word"; Escape hides it
  // until the draft changes again.
  const [menuClosed, setMenuClosed] = useState(false);
  const [dispatchMenuOpen, setDispatchMenuOpen] = useState(false);
  const [commandIndex, setCommandIndex] = useState(0);
  const slash = agent !== "antigravity" && /^\/\S*$/.test(value) ? value.toLowerCase() : null;
  const builtInNames = new Set(COMMANDS.map((command) => command.name.slice(1)));
  const commands = [
    ...COMMANDS,
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
      // Plain multi-line text, such as terminal output, goes in as one code block with its lines
      // and spaces kept. Text copied with HTML keeps the editor's default paste.
      handlePaste: (view, event) => {
        const data = event.clipboardData;
        const text = data?.getData("text/plain").replace(/\r\n?/g, "\n").replace(/\n+$/, "") ?? "";
        if (!text.includes("\n") || data?.types.includes("text/html")) return false;
        const { state } = view;
        const block = state.schema.nodes.codeBlock.create(null, state.schema.text(text));
        view.dispatch(state.tr.replaceSelectionWith(block).scrollIntoView());
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
  const deferFocusRef = useRef(deferFocus);
  deferFocusRef.current = deferFocus;
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
      document.activeElement === document.body
    ) {
      editor.commands.focus("end");
    }
    wasWorking.current = working;
  }, [editor, working, deferFocus]);

  // A choice panel takes the keys: the editor is read-only and unfocused while it is open, and
  // takes the focus when it closes so the user can write their own answer.
  const wasDeferred = useRef(deferFocus);
  useEffect(() => {
    if (!editor) return;
    editor.setEditable(!deferFocus, false);
    if (deferFocus && editor.isFocused) editor.commands.blur();
    else if (!deferFocus && wasDeferred.current) editor.commands.focus("end");
    wasDeferred.current = deferFocus;
  }, [editor, deferFocus]);

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

  /** Unchanged recalled messages cycle directly, including Markdown with multiple blocks.
   * Editing a recalled message returns the arrows to normal text navigation. */
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
    // A new or edited multi-line draft keeps normal caret navigation. A recalled message
    // stays in history navigation until edited, regardless of its formatting or caret position.
    if (!at) {
      const pos = selection.from;
      const lines = back
        ? doc.textBetween(0, pos, "\n", "\n")
        : doc.textBetween(pos, doc.content.size, "\n", "\n");
      if (lines.includes("\n")) return false;
    }
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
    // Same as clicking the queued strip's Steer button: Ctrl+Enter while it is expanded.
    if (
      event.key === "Enter" &&
      (event.metaKey || event.ctrlKey) &&
      (event.shiftKey || queueOpen) &&
      canSteerQueued
    ) {
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

  function submitAs(queue: boolean) {
    if (queue && !canQueueFollowUp) return;
    onSubmit(queue);
    setQueueNext(true);
    setDispatchMenuOpen(false);
  }

  const surface = dropping
    ? "border border-dashed border-accent bg-editor"
    : goalActive
      ? "goal-border"
      : "border border-border-strong bg-editor focus-within:border-accent";
  return (
    <div
      className={`relative flex flex-col gap-2 rounded-[8px] pt-3.5 pr-3 pb-2.5 pl-3 ${surface}`}
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
      <EditorContent editor={editor} className="composer-editor min-w-0" />
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
                      className="w-full cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-2.5 py-2 text-left text-fg hover:bg-hover disabled:cursor-default disabled:opacity-40"
                      onClick={() => submitAs(false)}
                    >
                      Steer current turn
                      <span className="mt-0.5 block text-[length:var(--fs-xs)] text-fg-subtle">
                        Send this message into the active turn
                      </span>
                    </button>
                    <button
                      disabled={overBy > 0 || !canQueueFollowUp}
                      className="w-full cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-2.5 py-2 text-left text-fg hover:bg-hover disabled:cursor-default disabled:opacity-40"
                      onClick={() => submitAs(true)}
                    >
                      Queue for next turn
                      <span className="mt-0.5 block text-[length:var(--fs-xs)] text-fg-subtle">
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
