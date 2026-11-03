import { useEffect, type MutableRefObject, type RefObject } from "react";
import type { Editor } from "@tiptap/react";
import { Selection } from "@tiptap/pm/state";
import type { DropMentions } from "~/features/chat/composer/types";

/**
 * Lets the chat write into the draft: files dropped on the composer, and text quoted from a
 * reply. Each is a function the composer sets on the chat's ref while its editor is up.
 */
export function useDraftInserts(
  editor: Editor | null,
  surfaceRef: RefObject<HTMLDivElement | null>,
  dropMentions: MutableRefObject<DropMentions | null>,
  quoteInto: MutableRefObject<((lines: string[]) => void) | null>,
) {
  // Files and folders dropped on the composer go into the draft as mentions where they land, or
  // at the caret when they land beside the text. Images go in the same way, as themselves.
  useEffect(() => {
    if (!editor) return;
    dropMentions.current = (point) => {
      const box = surfaceRef.current?.getBoundingClientRect();
      if (
        !box ||
        !editor.isEditable ||
        (point &&
          (point.x < box.left || point.x > box.right || point.y < box.top || point.y > box.bottom))
      ) {
        return null;
      }
      const at =
        (point && editor.view.posAtCoords({ left: point.x, top: point.y })?.pos) ??
        editor.state.selection.to;
      return (mentions) => {
        if (mentions.length === 0) return;
        // A point beside or below the text is between blocks, where the mentions would start a
        // line of their own; they go at the end of the nearest line before it instead.
        const { doc } = editor.state;
        const pos = Selection.near(doc.resolve(Math.min(at, doc.content.size)), -1).from;
        const before = doc.textBetween(Math.max(pos - 1, 0), pos, "\n", "￼");
        editor
          .chain()
          .focus()
          .insertContentAt(pos, [
            ...(before && !/\s/.test(before) ? [{ type: "text", text: " " }] : []),
            ...mentions.flatMap(({ id, label, image }) => [
              { type: image ? "inlineImage" : "mention", attrs: { id, label } },
              { type: "text", text: " " },
            ]),
          ])
          .run();
      };
    };
    return () => {
      dropMentions.current = null;
    };
  }, [editor, dropMentions]);

  // A quote goes at the end of the draft as its own lines, the way a typed one is kept, and the
  // caret goes to a fresh line after it for the reply.
  useEffect(() => {
    if (!editor) return;
    quoteInto.current = (lines) => {
      if (!editor.isEditable) return;
      const end = editor.state.doc.content.size;
      editor
        .chain()
        .insertContentAt(editor.isEmpty ? { from: 0, to: end } : end, [
          ...lines.map((line) => ({ type: "paragraph", content: [{ type: "text", text: line }] })),
          { type: "paragraph" },
        ])
        .focus("end")
        .run();
    };
    return () => {
      quoteInto.current = null;
    };
  }, [editor, quoteInto]);
}
