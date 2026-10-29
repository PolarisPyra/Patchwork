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

interface FormatButtonSpec {
  label: string;
  /** The mark or node this button toggles, so its pressed state can be read. */
  name: string;
  Icon: Icon;
  toggle: (editor: Editor) => void;
}

const BUTTONS: FormatButtonSpec[] = [
  {
    label: "Bold",
    name: "bold",
    Icon: TextBIcon,
    toggle: (e) => e.chain().focus().toggleBold().run(),
  },
  {
    label: "Italic",
    name: "italic",
    Icon: TextItalicIcon,
    toggle: (e) => e.chain().focus().toggleItalic().run(),
  },
  {
    label: "Strikethrough",
    name: "strike",
    Icon: TextStrikethroughIcon,
    toggle: (e) => e.chain().focus().toggleStrike().run(),
  },
  {
    label: "Inline code",
    name: "code",
    Icon: CodeIcon,
    toggle: (e) => e.chain().focus().toggleCode().run(),
  },
  {
    label: "Code block",
    name: "codeBlock",
    Icon: CodeBlockIcon,
    toggle: (e) => e.chain().focus().toggleCodeBlock().run(),
  },
  {
    label: "Bullet list",
    name: "bulletList",
    Icon: ListBulletsIcon,
    toggle: (e) => e.chain().focus().toggleBulletList().run(),
  },
  {
    label: "Numbered list",
    name: "orderedList",
    Icon: ListNumbersIcon,
    toggle: (e) => e.chain().focus().toggleOrderedList().run(),
  },
];

function FormatButton({ editor, spec }: { editor: Editor | null; spec: FormatButtonSpec }) {
  const active = !!useEditorState({
    editor,
    selector: ({ editor: current }) => !!current?.isActive(spec.name),
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
    <div className="flex items-center gap-0.5">
      {BUTTONS.map((spec) => (
        <FormatButton key={spec.name} editor={editor} spec={spec} />
      ))}
    </div>
  );
}
