import type { Node, Schema } from "@tiptap/pm/model";
import type { EditorView } from "@tiptap/pm/view";
import { api } from "~/api";
import { detectLanguage, isTerminalOutput } from "~/features/chat/composer/lib/languageDetection";

/**
 * How the composer reads a multi-line paste: as prose that keeps its paragraphs, or as code in a
 * language named by the copy's source or detected from the text.
 */

/** Whether copied HTML is styled monospace text, as terminals and code editors copy it,
 * rather than a document with paragraphs, lists or headings. */
export function isMonospaceHtml(html: string): boolean {
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
export function isProse(text: string): boolean {
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

/** A line of pasted prose as the composer's inline nodes, its `code` spans marked as code. */
export function proseLine(schema: Schema, line: string): Node[] {
  return line.split(INLINE_CODE).flatMap((part, i) => {
    if (!part) return [];
    // Split on a pattern with one group, every odd part is a span's code.
    return [schema.text(part, i % 2 ? [schema.marks.code.create()] : null)];
  });
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
export function vscodeLanguage(metadata: string | null): string | null {
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
export function htmlLanguage(html: string): string | null {
  const match = /class="[^"]*\b(?:language|lang|highlight-source)-([\w+#-]+)/i.exec(html);
  if (!match) return null;
  const id = match[1].toLowerCase();
  return LANGUAGE_ALIASES[id] ?? id;
}

/**
 * Labels a pasted code block once the block is in. A copy from VS Code or another Electron editor
 * names its language in clipboard metadata that the webview does not show to the page, so the app
 * reads it from the system clipboard. Other code gets the language VS Code would detect for it.
 */
export function labelPastedCode(view: EditorView, text: string) {
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
