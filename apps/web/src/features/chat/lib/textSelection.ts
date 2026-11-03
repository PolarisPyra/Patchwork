import { selectionMarkdown } from "~/features/chat/lib/selectionMarkdown";

/**
 * Copying from the chat puts the text the reader sees on the clipboard: the words of what can be
 * selected, a line break between blocks and a tab between table cells, without the chrome around
 * them. A paste into a terminal or another app gets that text and nothing else.
 *
 * A selection inside one rendered Markdown block also carries the Markdown it was rendered from,
 * in the copy's HTML, so a paste into the composer keeps its formatting (`selectionMarkdown.ts`).
 *
 * What the projection reads from the DOM:
 * - Only text inside `.selectable` counts; the app's chrome cannot be selected.
 * - `[data-text-exclude]`, buttons, `.sr-only` and `[hidden]` copy as nothing.
 * - A formula (`data-tex`) or a diagram (`data-artifact`) copies as its source, whole, even when
 *   only part of it is selected: the text KaTeX or a drawing renders reads as nonsense.
 */

const BLOCK = "p, h1, h2, h3, h4, h5, h6, ul, ol, li, blockquote, pre, hr, table, tr, div, section";
/** Blocks set apart by a blank line rather than a line break. */
const PARAGRAPH_TAGS = new Set([
  "P",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "PRE",
  "BLOCKQUOTE",
  "TABLE",
]);
const EXCLUDED = "[data-text-exclude], button, .sr-only, [hidden]";
const EDITABLE = '[contenteditable="true"], input, textarea';

const isBlockAtom = (el: Element) =>
  el.hasAttribute("data-artifact") || el.classList.contains("katex-display");

/** A paragraph, heading, code block, quote, table, formula, diagram, or a list that is not
 * nested in another. */
const isParagraph = (el: Element | null) =>
  el !== null &&
  (PARAGRAPH_TAGS.has(el.tagName) ||
    isBlockAtom(el) ||
    el.hasAttribute("data-code") ||
    ((el.tagName === "UL" || el.tagName === "OL") && el.parentElement?.tagName !== "LI"));

/** The ancestor of `el` that is a child of `top`, or null when `el` is `top`. */
function childOf(top: Element, el: Element): Element | null {
  let node: Element | null = el;
  while (node && node.parentElement !== top) node = node.parentElement;
  return node;
}

/**
 * The break between two blocks of text: a blank line when either is a paragraph, or when they
 * sit in different paragraphs further up, as a list item before a table does; otherwise a line.
 */
function breakBetween(a: Element | null, b: Element | null): string {
  if (!a || !b || isParagraph(a) || isParagraph(b)) return "\n\n";
  let top: Element | null = a;
  while (top && !top.contains(b)) top = top.parentElement;
  if (!top) return "\n\n";
  return isParagraph(childOf(top, a)) || isParagraph(childOf(top, b)) ? "\n\n" : "\n";
}

/**
 * The selection's range when it is one range inside `root` and not in an editor or a code block,
 * or null: an editor copies itself, and a code block's own copy is already exact.
 */
export function readTextSelection(root: HTMLElement): Range | null {
  const selection = root.ownerDocument.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount !== 1) return null;
  const range = selection.getRangeAt(0);
  const common = range.commonAncestorContainer;
  const container = common instanceof Element ? common : common.parentElement;
  if (!container || !root.contains(container)) return null;
  if (container.closest(`${EDITABLE}, pre, [data-code]`)) return null;
  return range;
}

/** The visible text `range` covers inside `root`, trimmed, with block breaks and cell tabs. */
export function selectedText(root: HTMLElement, range: Range): string {
  let out = "";
  /** The block and table cell the last piece of text sat in. */
  let lastBlock: Element | null = null;
  let lastCell: Element | null = null;
  const preserved = new Map<Element, boolean>();
  const keepsSpaces = (el: Element) => {
    let keeps = preserved.get(el);
    if (keeps === undefined) {
      const { whiteSpace } = getComputedStyle(el);
      keeps = whiteSpace.startsWith("pre") || whiteSpace === "break-spaces";
      preserved.set(el, keeps);
    }
    return keeps;
  };

  /** Adds a piece of text; a `collapsed` one drops its leading space at the start of a line. */
  const emit = (piece: string, block: Element | null, cell: Element | null, collapsed = true) => {
    if (out && block !== lastBlock) {
      out = out.replace(/ +$/, "") + breakBetween(lastBlock, block);
    } else if (out && cell !== lastCell) {
      out = out.replace(/ +$/, "") + "\t";
    }
    if (collapsed && (!out || /[\n\t ]$/.test(out))) piece = piece.replace(/^ /, "");
    out += piece;
    lastBlock = block;
    lastCell = cell;
  };

  const visit = (node: Node) => {
    if (!range.intersectsNode(node)) return;
    if (node.nodeType === Node.TEXT_NODE) {
      const parent = node.parentElement;
      if (!parent?.closest(".selectable")) return;
      const data = (node as Text).data;
      const start = node === range.startContainer ? range.startOffset : 0;
      const end = node === range.endContainer ? range.endOffset : data.length;
      let piece = data.slice(start, end);
      const collapsed = !keepsSpaces(parent);
      if (collapsed) {
        piece = piece.replace(/[ \t\n\r\f]+/g, " ");
        // Whitespace between blocks, as a renderer leaves between list items, is not text.
        if (piece === " " && parent.closest(BLOCK) !== lastBlock) return;
      }
      if (piece) emit(piece, parent.closest(BLOCK), parent.closest("td, th"), collapsed);
      return;
    }
    if (!(node instanceof Element) || node.matches(EXCLUDED)) return;
    const source = node.getAttribute("data-tex") ?? node.getAttribute("data-artifact-source");
    if (source !== null) {
      if (!node.closest(".selectable")) return;
      // A display formula or a diagram is a block of its own; an inline formula sits in its line.
      emit(source, isBlockAtom(node) ? node : node.closest(BLOCK), node.closest("td, th"), false);
      return;
    }
    for (const child of node.childNodes) visit(child);
  };
  visit(root);
  return out.trim();
}

/** The parts of a copy event the handler uses, which React's and the DOM's both have. */
interface CopyEvent {
  currentTarget: EventTarget & HTMLElement;
  target: EventTarget;
  clipboardData: DataTransfer | null;
  preventDefault(): void;
}

/**
 * Copies the selection inside the element it is set on as the text the reader sees, plus the
 * Markdown it was rendered from when that is one Markdown block. A selection it cannot read, in
 * an editor or a code block, is left to the browser.
 */
export function copySelection(event: CopyEvent) {
  if (event.target instanceof Element && event.target.closest(EDITABLE)) return;
  const root = event.currentTarget;
  const range = readTextSelection(root);
  if (!range || !event.clipboardData) return;
  const text = selectedText(root, range);
  if (!text) return;
  event.preventDefault();
  event.clipboardData.setData("text/plain", text);
  const markdown = selectionMarkdown(range);
  if (markdown) event.clipboardData.setData("text/html", markdown.html);
}
