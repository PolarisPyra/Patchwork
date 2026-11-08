import { useEffect, useRef, useState, type RefObject } from "react";
import { ChatTextIcon, PencilSimpleIcon, TrashIcon } from "@phosphor-icons/react";
import type { ReviewNote } from "~/features/canvas/review";

/** A note being written or edited under the lines it is about. Ctrl+Enter saves it and Escape
 * leaves it. */
export function NoteEditor({
  where,
  initial = "",
  onSave,
  onCancel,
}: {
  where: string;
  initial?: string;
  onSave: (text: string) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState(initial);
  const input = useRef<HTMLTextAreaElement>(null);
  useEffect(() => input.current?.focus(), []);
  const save = () => {
    if (text.trim()) onSave(text.trim());
  };
  return (
    <div
      role="group"
      aria-label={`Note on ${where}`}
      className="my-1 mr-4 ml-[116px] flex flex-col gap-2 rounded-[var(--radius)] border border-border bg-panel p-2 font-[family-name:var(--font-sans)] whitespace-normal"
    >
      <textarea
        ref={input}
        rows={3}
        value={text}
        aria-label={`Note on ${where}`}
        placeholder={`Note on ${where}`}
        className="resize-y rounded-[var(--radius-sm)] border border-border bg-input px-2 py-1 text-[length:var(--fs-sm)] text-fg outline-none focus:border-accent"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            save();
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onCancel();
          }
        }}
      />
      <div className="flex justify-end gap-2">
        <button className="btn" onClick={onCancel}>
          Cancel
        </button>
        <button className="btn btn-primary" disabled={!text.trim()} onClick={save}>
          {initial ? "Save" : "Add note"}
        </button>
      </div>
    </div>
  );
}

/** A saved note, under the lines it is about. */
export function NoteCard({
  note,
  onEdit,
  onRemove,
}: {
  note: ReviewNote;
  onEdit: () => void;
  onRemove: () => void;
}) {
  return (
    <div
      role="note"
      aria-label={`Note on ${note.where}`}
      className="group my-1 mr-4 ml-[116px] flex items-start gap-2 rounded-[var(--radius)] border border-border bg-panel px-3 py-2 font-[family-name:var(--font-sans)] text-[length:var(--fs-sm)] whitespace-pre-wrap"
    >
      <ChatTextIcon size={14} className="mt-0.5 flex-none text-accent-text" aria-hidden />
      <span className="min-w-0 flex-1 break-words">{note.text}</span>
      <button
        className="btn-icon hover-reveal !h-5 !w-5 flex-none"
        aria-label={`Edit note on ${note.where}`}
        title="Edit"
        onClick={onEdit}
      >
        <PencilSimpleIcon size={13} />
      </button>
      <button
        className="btn-icon hover-reveal !h-5 !w-5 flex-none"
        aria-label={`Delete note on ${note.where}`}
        title="Delete"
        onClick={onRemove}
      >
        <TrashIcon size={13} />
      </button>
    </div>
  );
}

/** A Note button over lines selected in the diff. It is read on mouse down, since the click
 * would clear the selection. */
export function SelectionNote({
  root,
  onNote,
}: {
  root: RefObject<HTMLDivElement | null>;
  onNote: (range: Range) => void;
}) {
  const [at, setAt] = useState<{ top: number; left: number } | null>(null);
  useEffect(() => {
    let frame = 0;
    const place = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const el = root.current;
        const selection = window.getSelection();
        const range = selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null;
        if (!el || !range || range.collapsed || !el.contains(range.commonAncestorContainer)) {
          setAt(null);
          return;
        }
        const rect = range.getBoundingClientRect();
        const box = el.getBoundingClientRect();
        setAt({ top: rect.top - box.top, left: rect.right - box.left });
      });
    };
    document.addEventListener("selectionchange", place);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("selectionchange", place);
    };
  }, [root]);

  if (!at) return null;
  return (
    <button
      type="button"
      style={{ top: Math.max(at.top - 6, 0), left: at.left }}
      className="absolute z-10 flex -translate-x-1/2 -translate-y-full cursor-pointer items-center gap-1 rounded-[var(--radius)] border border-border bg-panel px-2 py-1 font-[family-name:var(--font-sans)] text-[length:var(--fs-sm)] text-fg shadow-[var(--shadow-popup)] hover:bg-hover"
      onMouseDown={(e) => {
        e.preventDefault();
        const selection = window.getSelection();
        if (selection && selection.rangeCount > 0) onNote(selection.getRangeAt(0));
        selection?.removeAllRanges();
      }}
    >
      <ChatTextIcon size={14} aria-hidden />
      Note
    </button>
  );
}
