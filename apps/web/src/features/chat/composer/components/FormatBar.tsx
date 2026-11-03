import type { Editor } from "@tiptap/core";
import { useEditorState } from "@tiptap/react";
import {
  CodeBlockIcon,
  CodeIcon,
  ListBulletsIcon,
  ListNumbersIcon,
  TextBIcon,
  TextItalicIcon,
  TextStrikethroughIcon,
  type Icon,
} from "@phosphor-icons/react";
import { hasMarker, toggleMarker, type InlineMarker } from "~/features/chat/lib/composerExtensions";

interface FormatButtonSpec {
  label: string;
  Icon: Icon;
  /** Whether the selection already has this formatting, for the button's pressed state. */
  active: (editor: Editor) => boolean;
  toggle: (editor: Editor) => void;
}

/** Bold, italic and the like are the Markdown markers written around the selection. */
const marker = (label: string, Icon: Icon, text: InlineMarker): FormatButtonSpec => ({
  label,
  Icon,
  active: (e) => hasMarker(e.state, text),
  toggle: (e) => toggleMarker(e, text),
});

const BUTTONS: FormatButtonSpec[] = [
  marker("Bold", TextBIcon, "**"),
  marker("Italic", TextItalicIcon, "*"),
  marker("Strikethrough", TextStrikethroughIcon, "~~"),
  marker("Inline code", CodeIcon, "`"),
  {
    label: "Code block",
    Icon: CodeBlockIcon,
    active: (e) => e.isActive("codeBlock"),
    toggle: (e) => e.chain().focus().toggleCodeBlock().run(),
  },
  {
    label: "Bullet list",
    Icon: ListBulletsIcon,
    active: (e) => e.isActive("bulletList"),
    toggle: (e) => e.chain().focus().toggleBulletList().run(),
  },
  {
    label: "Numbered list",
    Icon: ListNumbersIcon,
    active: (e) => e.isActive("orderedList"),
    toggle: (e) => e.chain().focus().toggleOrderedList().run(),
  },
];

function FormatButton({ editor, spec }: { editor: Editor | null; spec: FormatButtonSpec }) {
  const active = !!useEditorState({
    editor,
    selector: ({ editor: current }) => (current ? spec.active(current) : false),
  });
  const { Icon } = spec;
  return (
    <button
      type="button"
      className={`grid h-8 w-8 flex-none cursor-pointer place-items-center rounded-[var(--radius)] border-none disabled:cursor-default disabled:opacity-50 ${active ? "bg-selected text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
      // Keeps the focus in the editor.
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => {
        if (editor) spec.toggle(editor);
      }}
      disabled={!editor}
      title={spec.label}
      aria-label={spec.label}
      aria-pressed={active}
    >
      <Icon size={16} />
    </button>
  );
}

/** Buttons that wrap the composer's selection in Markdown formatting. */
export default function FormatBar({ editor }: { editor: Editor | null }) {
  return (
    <div role="toolbar" aria-label="Formatting" className="flex items-center gap-0.5">
      {BUTTONS.map((spec) => (
        <FormatButton key={spec.label} editor={editor} spec={spec} />
      ))}
    </div>
  );
}
