import { type Node as ProseMirrorNode } from "@tiptap/pm/model";
import { Plugin } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import { highlightBlock, MAX_HIGHLIGHT_LINES } from "~/lib/highlight";

/**
 * Colours the code in code blocks. Each highlighted token becomes an inline decoration, so the
 * text stays editable and the decorations are not written into the Markdown.
 */
export function codeHighlightPlugin(): Plugin {
  return new Plugin({
    state: {
      init(_, { doc }) {
        return highlightDoc(doc);
      },
      apply(tr, decorations) {
        return tr.docChanged ? highlightDoc(tr.doc) : decorations;
      },
    },
    props: {
      decorations(state) {
        return this.getState(state);
      },
    },
  });
}

function highlightDoc(doc: ProseMirrorNode): DecorationSet {
  const decorations: Decoration[] = [];
  doc.descendants((node, pos) => {
    if (node.type.name !== "codeBlock") return true;
    const code = node.textContent;
    if (code.split("\n").length > MAX_HIGHLIGHT_LINES) return false;
    const html = highlightBlock(code, node.attrs.language ?? "");
    if (html !== null) addSpans(html, pos, decorations);
    return false;
  });
  return DecorationSet.create(doc, decorations);
}

/** Parses highlighted HTML and adds a decoration for each element that has classes. */
function addSpans(html: string, blockPos: number, out: Decoration[]) {
  // The leading <body> stops the parser from dropping a newline at the start of the code.
  const body = new DOMParser().parseFromString(`<body>${html}`, "text/html").body;
  walk(body, 0);

  // Returns the text offset after the element, so nested spans get their own ranges.
  function walk(parent: Element, offset: number): number {
    let end = offset;
    for (const child of Array.from(parent.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        end += child.textContent?.length ?? 0;
      } else if (child instanceof Element) {
        const start = end;
        end = walk(child, end);
        if (child.className && end > start) {
          out.push(
            Decoration.inline(blockPos + 1 + start, blockPos + 1 + end, {
              class: child.className,
            }),
          );
        }
      }
    }
    return end;
  }
}
