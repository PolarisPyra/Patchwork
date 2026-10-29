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

/** Characters a line of prose is made of: letters, digits, spaces and sentence punctuation. */
const PROSE_CHAR = /[\p{L}\p{N}\s.,!?'"’“”:()\-–—/]/u;

/**
 * Whether plain text reads as sentences rather than code or output: no line is indented, symbol
 * heavy, or terminal output, and most lines are long or end like a sentence.
 */
export function isProse(text: string): boolean {
  if (isTerminalOutput(text)) return false;
  const lines = text.split("\n").filter((line) => line.trim());
  let sentences = 0;
  let words = 0;
  for (const line of lines) {
    if (/^\s/.test(line)) return false;
    const chars = [...line];
    if (chars.filter((c) => PROSE_CHAR.test(c)).length < chars.length * 0.97) return false;
    const count = line.split(/\s+/).filter((word) => /\p{L}/u.test(word)).length;
    words += count;
    if (count >= 8 || /[.!?:,]$/.test(line)) sentences++;
  }
  return words >= 8 && sentences * 2 >= lines.length;
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
