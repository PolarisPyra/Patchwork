import { useRef } from "react";
import { useEditor, type Editor } from "@tiptap/react";
import { pasteText } from "~/features/chat/composer/lib/pastedText";
import {
  composerExtensions,
  keepDraftText,
  type MentionSuggestion,
} from "~/features/chat/lib/composerExtensions";

const EDITOR_CLASS =
  "min-h-16 max-h-[280px] overflow-y-auto whitespace-pre-wrap break-words px-1.5 text-fg text-[length:var(--fs-md)] leading-[20px] outline-none selectable [&_p]:m-0 [&_ul]:my-0 [&_ol]:my-0 [&_ul]:pl-5 [&_ol]:pl-5 [&_pre]:my-1 [&_pre]:rounded-[var(--radius-sm)] [&_pre]:bg-panel [&_pre]:px-2.5 [&_pre]:py-1.5 [&_pre]:font-[family-name:var(--font-mono)] [&_pre]:text-[12px] [&_code]:font-[family-name:var(--font-mono)] [&_code]:text-[12px]";

interface Handlers {
  /** Keys the composer takes before the editor; true when it took the key. */
  onKeyDown: (event: KeyboardEvent) => boolean;
  /** Opens a file mentioned in the draft beside the chat; null without a project. */
  onOpenFile: ((path: string) => void) | null;
  /** The user changed the draft. */
  onUpdate: (editor: Editor) => void;
  onSelectionUpdate: (editor: Editor) => void;
}

/**
 * The draft's editor: its chips, its keys, and how it takes clicks, pastes and input methods.
 * The `is…` checks tell which typed names become chips.
 */
export function useComposerEditor({
  value,
  mentionSuggestion,
  isSkill,
  isAgent,
  isCommand,
  ...handlers
}: Handlers & {
  /** The draft as Markdown, which the editor starts with. */
  value: string;
  mentionSuggestion: MentionSuggestion;
  isSkill: (name: string) => boolean;
  isAgent: (name: string) => boolean;
  isCommand: (name: string) => boolean;
}): Editor | null {
  // Event handlers are read through a ref, so the editor always calls the current ones.
  const latest = useRef(handlers);
  latest.current = handlers;

  const editor: Editor | null = useEditor({
    extensions: composerExtensions(
      "Reply or describe the next change…",
      mentionSuggestion,
      isSkill,
      isAgent,
      isCommand,
    ),
    content: value,
    contentType: "markdown",
    immediatelyRender: false,
    editorProps: {
      attributes: {
        class: EDITOR_CLASS,
        role: "textbox",
        "aria-label": "Message",
        "aria-multiline": "true",
      },
      handleKeyDown: (_view, event) => latest.current.onKeyDown(event),
      // Marks the editor's box while an input method composes, which hides the placeholder (see
      // theme.css). The editor sets its own element's attributes, so the mark goes on the box.
      handleDOMEvents: {
        compositionstart: (view) => {
          view.dom.parentElement?.setAttribute("data-composing", "");
          return false;
        },
        compositionend: (view) => {
          view.dom.parentElement?.removeAttribute("data-composing");
          return false;
        },
      },
      // A file chip opens the file beside the chat; a folder's has nothing to show.
      handleClickOn: (_view, _pos, node) => {
        const path: unknown = node.attrs.id;
        if (node.type.name !== "mention" || typeof path !== "string" || path.endsWith("/")) {
          return false;
        }
        const { onOpenFile } = latest.current;
        onOpenFile?.(path);
        return onOpenFile !== null;
      },
      handlePaste: (view, event) =>
        pasteText(
          view,
          event,
          (markdown) =>
            editor?.commands.insertContent(markdown, { contentType: "markdown" }) ?? false,
        ),
    },
    onCreate: ({ editor }) => {
      if (editor.markdown) keepDraftText(editor.markdown);
    },
    onUpdate: ({ editor }) => latest.current.onUpdate(editor),
    onSelectionUpdate: ({ editor }) => latest.current.onSelectionUpdate(editor),
  });
  return editor;
}
