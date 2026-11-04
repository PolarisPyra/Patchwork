import { Fragment, Slice, type Node, type Schema } from "@tiptap/pm/model";
import { TextSelection } from "@tiptap/pm/state";
import type { EditorView } from "@tiptap/pm/view";
import { api } from "~/api";
import { detectLanguage, isTerminalOutput } from "~/features/chat/composer/lib/languageDetection";
import { copiedMarkdown, keepTexEscapes } from "~/features/chat/lib/selectionMarkdown";

/**
 * How the composer reads a multi-line paste: as prose that keeps its paragraphs, or as code in a
 * language named by the copy's source or detected from the text.
 */

/** Whether copied HTML is styled monospace text, as terminals and code editors copy it,
 * rather than a document with paragraphs, lists or headings. */
function isMonospaceHtml(html: string): boolean {
  const body = new DOMParser().parseFromString(html, "text/html").body;
  if (body.querySelector("p, h1, h2, h3, h4, h5, h6, li, table, blockquote")) return false;
  return /<pre[\s>]|monospace|white-space:\s*pre/i.test(html);
}

/** Characters a line of prose is made of: letters, digits, spaces, sentence punctuation, and the
 * underscores of a name written into it. */
const PROSE_CHAR = /[\p{L}\p{N}\s.,!?'"’“”:()\-–—/_]/u;

/** A `code` span written into prose, as Markdown and chat messages quote a command or a name. */
const INLINE_CODE = /`([^`\n]+)`/g;

/** The Markdown a message is written in, which is not code: a heading, quote or list marker, a
 * task box, bold, struck text, and a link, which keeps its text. */
const MARKDOWN: [RegExp, string][] = [
  [/^\s*(#{1,6}|>|[-*+]|\d+[.)])\s+(\[[ xX]\]\s+)?/, ""],
  [/\*\*|__|~~/g, ""],
  [/\[([^\]\n]*)\]\([^)\s]*\)/g, "$1"],
];

/** A list item indented under another, which prose has too. */
const NESTED_ITEM = /^\s+([-*+]|\d+[.)])\s/;

/** Words lined up in columns with runs of spaces, as command output and tables are. */
const COLUMNS = /[^\s.!?] {2,}\S/;

/** Whether a line has the look of code: symbols prose does not use, or columns. */
function codeLine(line: string): boolean {
  if (COLUMNS.test(line.trim())) return true;
  const text = MARKDOWN.reduce((out, [pattern, to]) => out.replace(pattern, to), line).replace(
    INLINE_CODE,
    "",
  );
  const chars = [...text];
  return chars.filter((c) => PROSE_CHAR.test(c)).length < chars.length * 0.97;
}

/**
 * Whether plain text goes in as prose: it is not terminal output, has no indented code, and at
 * most a few of its lines look like code. Text is prose unless it shows it is not, so a message
 * of short lines, paths or names keeps its lines rather than turning into a code block.
 */
function isProse(text: string): boolean {
  if (isTerminalOutput(text)) return false;
  const lines = text.split("\n").filter((line) => line.trim());
  let code = 0;
  for (const line of lines) {
    if (/^\s/.test(line) && !NESTED_ITEM.test(line)) return false;
    if (/^(#!|\/\/|\/\*)/.test(line)) return false;
    if (codeLine(line)) code++;
  }
  return code * 2 < lines.length;
}

/** A line of pasted prose as the composer's text. Its Markdown, such as `code`, stays as written. */
function proseLine(schema: Schema, line: string): Node[] {
  return line ? [schema.text(line)] : [];
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
 * reads it from the system clipboard. Other code gets the language detected for it.
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

/**
 * Pastes go in as plain text; copied HTML is only read to tell what was copied, never parsed into
 * the draft. A copy from this editor keeps its blocks and mentions, and is left to the editor.
 * `insertMarkdown` puts Markdown in at the caret the way the draft loads it.
 */
export function pasteText(
  view: EditorView,
  event: ClipboardEvent,
  insertMarkdown: (markdown: string) => boolean,
): boolean {
  const data = event.clipboardData;
  if (!data) return false;
  const html = data.getData("text/html");
  if (html.includes("data-pm-slice")) return false;
  const text = data.getData("text/plain").replace(/\r\n?/g, "\n").replace(/\n+$/, "");
  if (!text) return true;
  // A copy from a reply carries its Markdown in its HTML, which goes in as the draft would
  // load it: lists and code as blocks, headings and quotes as the lines they were typed as.
  const markdown = copiedMarkdown(html);
  if (markdown !== null && !view.state.selection.$from.parent.type.spec.code) {
    return insertMarkdown(keepTexEscapes(markdown));
  }
  const { state } = view;
  // Inside a code block, and on a single line, the text goes in exactly as copied.
  if (state.selection.$from.parent.type.spec.code || !text.includes("\n")) {
    view.dispatch(state.tr.insertText(text).scrollIntoView());
    return true;
  }
  const language = vscodeLanguage(data.getData("vscode-editor-data")) ?? htmlLanguage(html);
  // Multi-line text from a page or document, or plain text that does not look like code,
  // keeps its paragraphs and line breaks.
  if ((html && !isMonospaceHtml(html)) || (!language && isProse(text))) {
    const paragraphs = text.split(/\n{2,}/).map((paragraph) =>
      state.schema.nodes.paragraph.create(
        null,
        paragraph
          .split("\n")
          .flatMap((line, i) => [
            ...(i > 0 ? [state.schema.nodes.hardBreak.create()] : []),
            ...proseLine(state.schema, line),
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
}
