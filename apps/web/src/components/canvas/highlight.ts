// Syntax highlighting and file-type helpers for the Canvas panel.
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import lua from "highlight.js/lib/languages/lua";
import makefile from "highlight.js/lib/languages/makefile";
import markdown from "highlight.js/lib/languages/markdown";
import php from "highlight.js/lib/languages/php";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

hljs.registerLanguage("bash", bash);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("csharp", csharp);
hljs.registerLanguage("css", css);
hljs.registerLanguage("dockerfile", dockerfile);
hljs.registerLanguage("go", go);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("java", java);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("kotlin", kotlin);
hljs.registerLanguage("lua", lua);
hljs.registerLanguage("makefile", makefile);
hljs.registerLanguage("markdown", markdown);
hljs.registerLanguage("php", php);
hljs.registerLanguage("powershell", powershell);
hljs.registerLanguage("python", python);
hljs.registerLanguage("ruby", ruby);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("swift", swift);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("yaml", yaml);

/** Files longer than this are shown without highlighting. */
export const MAX_HIGHLIGHT_LINES = 5000;

export interface FileKind {
  label: string;
  /** Registered highlight.js grammar, if any. */
  grammar?: string;
}

const KINDS: Record<string, FileKind | undefined> = {
  css: { label: "CSS", grammar: "css" },
  ts: { label: "TypeScript", grammar: "typescript" },
  tsx: { label: "TypeScript", grammar: "typescript" },
  js: { label: "JavaScript", grammar: "javascript" },
  jsx: { label: "JavaScript", grammar: "javascript" },
  rs: { label: "Rust", grammar: "rust" },
  py: { label: "Python", grammar: "python" },
  json: { label: "JSON", grammar: "json" },
  md: { label: "Markdown", grammar: "markdown" },
  html: { label: "HTML", grammar: "xml" },
  toml: { label: "TOML", grammar: "ini" },
  yml: { label: "YAML", grammar: "yaml" },
  yaml: { label: "YAML", grammar: "yaml" },
  sh: { label: "Shell", grammar: "bash" },
  go: { label: "Go", grammar: "go" },
};

export function describeFile(path: string): FileKind {
  const name = path.split("/").pop() ?? path;
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
  return KINDS[ext] ?? { label: ext ? ext.toUpperCase() : "Text" };
}

/** Lines of a file; a single trailing newline does not start a new line. */
export function splitLines(content: string): string[] {
  if (content === "") return [];
  const text = content.endsWith("\n") ? content.slice(0, -1) : content;
  return text.split("\n");
}

const TOKEN_RE = /<[^>]+>|[^<]+/g;

/**
 * Highlights the lines with highlight.js and returns one HTML string per line.
 * hljs escapes the source text, so the output is safe to inject. Spans that
 * cross a line break are closed at the break and re-opened on the next line.
 */
export function highlightLines(lines: string[], grammar: string): string[] {
  const html = hljs.highlight(lines.join("\n"), { language: grammar, ignoreIllegals: true }).value;
  const out: string[] = [];
  const open: string[] = [];
  let current = "";
  for (const token of html.match(TOKEN_RE) ?? []) {
    if (token.startsWith("</")) {
      open.pop();
      current += token;
    } else if (token.startsWith("<")) {
      open.push(token);
      current += token;
    } else {
      token.split("\n").forEach((part, i) => {
        if (i > 0) {
          out.push(current + "</span>".repeat(open.length));
          current = open.join("");
        }
        current += part;
      });
    }
  }
  out.push(current + "</span>".repeat(open.length));
  return out;
}

/** Highlighted HTML for a whole code block, or null when the language is not registered. */
export function highlightBlock(code: string, lang: string): string | null {
  if (!hljs.getLanguage(lang)) return null;
  return hljs.highlight(code, { language: lang, ignoreIllegals: true }).value;
}

/** How many of a grammar's keywords and patterns the code matches, as highlight.js scores it. */
export function grammarRelevance(code: string, lang: string): number {
  if (!hljs.getLanguage(lang)) return 0;
  return hljs.highlight(code, { language: lang, ignoreIllegals: true }).relevance;
}
