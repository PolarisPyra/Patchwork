import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { NodeViewContent, NodeViewWrapper, type NodeViewProps } from "@tiptap/react";
import { SYNTAX_CLASSES } from "~/components/canvas/syntaxClasses";
import { CODE_LANGUAGES } from "~/components/chat/codeLanguages";

const PLAIN_TEXT = "Plain text";
/** The picker's height with a full list: the filter field and up to 224px of rows. */
const PICKER_HEIGHT = 270;

/** A code block with its language in a header, and a filterable popup to pick another. */
export default function CodeBlockNodeView({ node, updateAttributes, editor }: NodeViewProps) {
  const language: string = node.attrs.language || "";
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);

  // The picker lives outside the composer, whose scrolling would cut it off. It sits under the
  // label, or above it when there is more room there, and follows the label as the page moves.
  useLayoutEffect(() => {
    if (!open) return;
    const position = () => {
      const button = buttonRef.current;
      const picker = pickerRef.current;
      if (!button || !picker) return;
      const rect = button.getBoundingClientRect();
      const above = rect.top - 8;
      const below = window.innerHeight - rect.bottom - 8;
      const up = below < PICKER_HEIGHT && above > below;
      Object.assign(picker.style, {
        left: `${Math.max(8, Math.min(rect.left, window.innerWidth - picker.offsetWidth - 8))}px`,
        top: up ? "auto" : `${rect.bottom + 4}px`,
        bottom: up ? `${window.innerHeight - rect.top + 4}px` : "auto",
        maxHeight: `${Math.min(PICKER_HEIGHT, up ? above : below)}px`,
      });
    };
    position();
    inputRef.current?.focus({ preventScroll: true });
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    return () => {
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
    };
  }, [open]);

  // A click anywhere else closes the picker.
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!pickerRef.current?.contains(target) && !buttonRef.current?.contains(target)) {
        setOpen(false);
      }
    };
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  }, [open]);

  const q = query.toLowerCase();
  const matches = CODE_LANGUAGES.filter((id) => (id || PLAIN_TEXT).toLowerCase().includes(q));

  function close() {
    setOpen(false);
    editor.commands.focus();
  }

  function choose(id: string) {
    updateAttributes({ language: id });
    close();
  }

  return (
    <NodeViewWrapper className="relative">
      <div contentEditable={false} className="flex h-6 items-center">
        <button
          ref={buttonRef}
          type="button"
          className="cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0 font-[family-name:var(--font-mono)] text-[12px] text-fg-muted hover:bg-hover hover:text-fg"
          aria-haspopup="listbox"
          aria-expanded={open}
          onClick={() => {
            setQuery("");
            setOpen(!open);
          }}
        >
          {language || PLAIN_TEXT}
        </button>
      </div>
      {open &&
        createPortal(
          <div
            ref={pickerRef}
            className="fixed z-[1000] flex w-48 flex-col gap-1 rounded-[var(--radius)] border border-border bg-panel p-1 shadow-[var(--shadow-popup)]"
            onKeyDown={(e) => {
              if (e.key === "Escape") close();
            }}
          >
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter languages"
              aria-label="Filter languages"
              className="rounded-[var(--radius-sm)] border border-border bg-editor px-2 py-1 text-[length:var(--fs-sm)] text-fg outline-none focus:border-accent"
            />
            <div className="flex max-h-56 min-h-0 flex-col gap-px overflow-y-auto">
              {matches.length === 0 && (
                <div className="px-2 py-1.5 text-[length:var(--fs-sm)] text-fg-muted">
                  No matching languages
                </div>
              )}
              {matches.map((id) => (
                <button
                  key={id || "plain"}
                  type="button"
                  className={`flex w-full cursor-pointer items-baseline rounded-[var(--radius)] border-none px-2 py-1.5 text-left font-[family-name:var(--font-mono)] text-[length:var(--fs-sm)] text-fg ${id === language ? "bg-selected" : "bg-transparent hover:bg-hover"}`}
                  // Keeps the focus in the editor.
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => choose(id)}
                >
                  {id || PLAIN_TEXT}
                </button>
              ))}
            </div>
          </div>,
          document.body,
        )}
      <pre>
        <NodeViewContent<"code"> as="code" className={SYNTAX_CLASSES} />
      </pre>
    </NodeViewWrapper>
  );
}
