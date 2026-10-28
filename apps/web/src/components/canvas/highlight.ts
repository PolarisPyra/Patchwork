// Syntax highlighting and file-type helpers for the Canvas panel and chat code blocks.
//
// Code is coloured with Shiki's TextMate grammars. highlight.js stays a dependency for scoring
// only: `grammarRelevance` is the language detector's confidence test, and Shiki has no such
// measure.
import { createHighlighterCoreSync, type LanguageRegistration } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import shikiBash from "shiki/langs/bash.mjs";
import shikiC from "shiki/langs/c.mjs";
import shikiConsole from "shiki/langs/console.mjs";
import shikiCpp from "shiki/langs/cpp.mjs";
import shikiCsharp from "shiki/langs/csharp.mjs";
import shikiCss from "shiki/langs/css.mjs";
import shikiDiff from "shiki/langs/diff.mjs";
import shikiDockerfile from "shiki/langs/dockerfile.mjs";
import shikiGo from "shiki/langs/go.mjs";
import shikiGraphql from "shiki/langs/graphql.mjs";
import shikiIni from "shiki/langs/ini.mjs";
import shikiJava from "shiki/langs/java.mjs";
import shikiJavascript from "shiki/langs/javascript.mjs";
import shikiJson from "shiki/langs/json.mjs";
import shikiKotlin from "shiki/langs/kotlin.mjs";
import shikiLua from "shiki/langs/lua.mjs";
import shikiMakefile from "shiki/langs/makefile.mjs";
import shikiMarkdown from "shiki/langs/markdown.mjs";
import shikiPhp from "shiki/langs/php.mjs";
import shikiPowershell from "shiki/langs/powershell.mjs";
import shikiPython from "shiki/langs/python.mjs";
import shikiRuby from "shiki/langs/ruby.mjs";
import shikiRust from "shiki/langs/rust.mjs";
import shikiScss from "shiki/langs/scss.mjs";
import shikiShell from "shiki/langs/shell.mjs";
import shikiSql from "shiki/langs/sql.mjs";
import shikiSwift from "shiki/langs/swift.mjs";
import shikiTsx from "shiki/langs/tsx.mjs";
import shikiTypescript from "shiki/langs/typescript.mjs";
import shikiXml from "shiki/langs/xml.mjs";
import shikiYaml from "shiki/langs/yaml.mjs";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import graphql from "highlight.js/lib/languages/graphql";
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
import scss from "highlight.js/lib/languages/scss";
import shell from "highlight.js/lib/languages/shell";
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
hljs.registerLanguage("diff", diff);
hljs.registerLanguage("dockerfile", dockerfile);
hljs.registerLanguage("go", go);
hljs.registerLanguage("graphql", graphql);
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
hljs.registerLanguage("scss", scss);
hljs.registerLanguage("shell", shell);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("swift", swift);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("yaml", yaml);

/** Shiki tokenises with this theme; its colours are never used, so it has none. Shiki skips
 * tokenising for a theme named "none". */
const THEME = "scopes";

const shiki = createHighlighterCoreSync({
  themes: [{ name: THEME, settings: [] }],
  langs: [],
  engine: createJavaScriptRegexEngine(),
});

/**
 * Each grammar by every name and alias it has, with the grammars it embeds, which its module lists
 * before it. A grammar is loaded the first time code needs it: loading all of them takes about
 * 100 ms, one about 2 ms.
 */
const GRAMMARS = new Map<string, LanguageRegistration[]>();
for (const module of [
  shikiBash,
  shikiC,
  shikiConsole,
  shikiCpp,
  shikiCsharp,
  shikiCss,
  shikiDiff,
  shikiDockerfile,
  shikiGo,
  shikiGraphql,
  shikiIni,
  shikiJava,
  shikiJavascript,
  shikiJson,
  shikiKotlin,
  shikiLua,
  shikiMakefile,
  shikiMarkdown,
  shikiPhp,
  shikiPowershell,
  shikiPython,
  shikiRuby,
  shikiRust,
  shikiScss,
  shikiShell,
  shikiSql,
  shikiSwift,
  shikiTsx,
  shikiTypescript,
  shikiXml,
  shikiYaml,
]) {
  module.forEach((grammar, i) => {
    for (const name of [grammar.name, ...(grammar.aliases ?? [])]) {
      if (!GRAMMARS.has(name)) GRAMMARS.set(name, module.slice(0, i + 1));
    }
  });
}

/** Names highlight.js took for these grammars that Shiki does not, so a fence keeps its colour. */
const ALIASES: Record<string, string | undefined> = {
  toml: "ini",
  h: "c",
  hpp: "cpp",
  hh: "cpp",
  hxx: "cpp",
  cc: "cpp",
  cxx: "cpp",
  "h++": "cpp",
  golang: "go",
  jsonc: "json",
  json5: "json",
  ktm: "kotlin",
  ktx: "kotlin",
  pluto: "lua",
  mk: "makefile",
  mak: "makefile",
  mkd: "markdown",
  mkdown: "markdown",
  gyp: "python",
  ipython: "python",
  gemspec: "ruby",
  podspec: "ruby",
  thor: "ruby",
  irb: "ruby",
  jsp: "java",
  patch: "diff",
  svg: "xml",
  xhtml: "xml",
  rss: "xml",
  atom: "xml",
  xsd: "xml",
  xsl: "xml",
  xjb: "xml",
  plist: "xml",
  wsf: "xml",
};

/** A language name as a fence or file kind gives it; the info string is model output. */
const LANGUAGE_NAME = /^[a-z0-9+#._-]{1,24}$/i;

/** The Shiki grammar for a language name, loaded, or null when none is registered. */
function shikiLanguage(name: string): string | null {
  if (!LANGUAGE_NAME.test(name)) return null;
  const lower = name.toLowerCase();
  const lang = ALIASES[lower] ?? lower;
  const grammar = GRAMMARS.get(lang);
  if (!grammar) return null;
  if (!shiki.getLoadedLanguages().includes(lang)) shiki.loadLanguageSync(grammar);
  return lang;
}

/**
 * The highlight.js class each TextMate scope is shown with; SYNTAX_CLASSES colours these. A token
 * takes the class of its innermost scope that has a rule here. `null` stops the search, so the
 * token is plain even inside a coloured scope, such as code inside a template string. Scopes with
 * no rule, or `undefined`, punctuation among them, pass to the scope around them. A class missing here, or one
 * SYNTAX_CLASSES has no rule for, shows as plain text.
 */
const SCOPE_CLASSES: [RegExp, string | null | undefined][] = [
  // Rust's format placeholders, as in "{:?}", are part of the string.
  [/^meta\.interpolation\.rust/, undefined],
  // Embedded code and interpolations are coloured as code, not as the string around them.
  [/^(source|meta\.(embedded|template\.expression|interpolation))\./, null],
  [/^comment\./, "comment"],
  [/^string\.regexp\./, "regexp"],
  [/^string\.unquoted\./, null],
  [/^storage\.type\.string\.python/, "string"],
  [/^string\./, "string"],
  [/^markup\.inserted\./, "addition"],
  [/^markup\.deleted\./, "deletion"],
  [/^(markup\.heading|entity\.name\.section)\./, "title"],
  [/^(markup\.inline\.raw|markup\.underline\.link)\./, "string"],
  [/^constant\.numeric\./, "number"],
  [/^constant\.other\.color\./, "number"],
  [/^(constant\.language|support\.constant)\./, "literal"],
  // Units after a number, as in 4px, are part of the number.
  [/^keyword\.other\.unit\./, undefined],
  [/^keyword\.other\.definition\.ini/, "attr"],
  [/^keyword\.control\.directive\./, "meta"],
  // Word operators such as new, typeof and await are keywords; symbols such as = and && are plain.
  [/^keyword\.operator\.(new|expression|sizeof|logical\.python|wordlike)/, "keyword"],
  [/^keyword\.operator\./, null],
  // C#'s int and string are types; Go's `type` keyword is keyword.type.go.
  [/^keyword\.type\.[\w-]+\.\w+$/, "type"],
  [/^keyword\./, "keyword"],
  [/^storage\.type\.annotation\./, "meta"],
  [/^storage\.type\.(primitive|numeric|string|boolean|byte|rune|error|built-in)/, "type"],
  // Java marks type names, as in Map<String, T>, as storage.type.
  [/^storage\.type\.(generic\.)?java$/, "type"],
  [/^storage\./, "keyword"],
  [/^variable\.language\./, "keyword"],
  [/^entity\.name\.function\.decorator\./, "meta"],
  [/^entity\.name\.function\./, "title"],
  [/^entity\.name\.(type\.)?(namespace|package)\./, null],
  [/^entity\.name\.tag\.yaml/, "attr"],
  [/^entity\.name\.tag\.(css|scss)/, "selector-tag"],
  [/^(entity\.name\.tag|support\.class\.component)\./, "name"],
  [/^punctuation\.definition\.tag\./, "tag"],
  [/^entity\.name\.(type|class|struct|enum|union|trait|interface)\./, "type"],
  [/^entity\.other\.inherited-class\./, "type"],
  [/^punctuation\.definition\.lifetime\./, "type"],
  [
    /^entity\.other\.attribute-name\.(class|id|pseudo-class|pseudo-element)\.(css|scss)/,
    "selector-class",
  ],
  [/^entity\.other\.attribute-name\./, "attr"],
  [/^support\.type\.property-name\./, "attr"],
  [/^(support\.type|support\.class)\./, "type"],
  [/^(support\.function|support\.variable)\./, "built_in"],
  [/^(variable\.other\.property|variable\.object\.property|variable\.other\.member)\./, "property"],
  [/^meta\.object-literal\.key\./, "property"],
  // Variables are coloured where they are marked with a sigil, as $HOME and --gap are.
  [
    /^(variable|punctuation\.definition\.variable|punctuation\.section\.bracket\.curly\.variable)\.([\w.-]*\.)?(shell|css|scss|php|powershell|makefile)$/,
    "variable",
  ],
  [
    /^(meta\.attribute\.rust|meta\.shebang|meta\.preprocessor|meta\.declaration\.annotation)/,
    "meta",
  ],
  [/^punctuation\.definition\.(annotation|decorator)\./, "meta"],
  [/^(meta\.diff\.|meta\.header\.|meta\.toc-list\.line-number\.)/, "meta"],
];

/** Each scope's rule, looked up once: a class, null to stop, or undefined to pass on. */
const scopeRules = new Map<string, string | null | undefined>();

function scopeRule(scope: string): string | null | undefined {
  if (scopeRules.has(scope)) return scopeRules.get(scope);
  const rule = SCOPE_CLASSES.find(([re]) => re.test(scope))?.[1];
  scopeRules.set(scope, rule);
  return rule;
}

/** The class for a token's scopes, outermost first, or "" for plain text. */
function tokenClass(scopes: { scopeName: string }[]): string {
  for (let i = scopes.length - 1; i >= 0; i--) {
    const rule = scopeRule(scopes[i].scopeName);
    if (rule === null) return "";
    if (rule !== undefined) return rule;
  }
  return "";
}

const escapeHtml = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/**
 * One HTML string per line of the code. Each token is one flat <span class="hljs-…">, never
 * nested and never across a line break, with its text escaped; plain text has no span.
 */
function shikiLines(code: string, lang: string): string[] {
  const { tokens } = shiki.codeToTokens(code, {
    lang,
    theme: THEME,
    includeExplanation: "scopeName",
  });
  return tokens.map((line) => {
    let html = "";
    let open = "";
    let text = "";
    const flush = () => {
      if (!text) return;
      html += open ? `<span class="hljs-${open}">${escapeHtml(text)}</span>` : escapeHtml(text);
      text = "";
    };
    for (const token of line) {
      for (const part of token.explanation ?? [{ content: token.content, scopes: [] }]) {
        // Whitespace joins the token before it, so a run of one class stays one span.
        const cls = part.content.trim() ? tokenClass(part.scopes) : open;
        if (cls !== open) {
          flush();
          open = cls;
        }
        text += part.content;
      }
    }
    flush();
    return html;
  });
}

/** Files longer than this are shown without highlighting. */
export const MAX_HIGHLIGHT_LINES = 5000;

export interface FileKind {
  label: string;
  /** Grammar the file is highlighted with, if any. */
  grammar?: string;
}

const KINDS: Record<string, FileKind | undefined> = {
  css: { label: "CSS", grammar: "css" },
  ts: { label: "TypeScript", grammar: "typescript" },
  tsx: { label: "TypeScript", grammar: "tsx" },
  js: { label: "JavaScript", grammar: "javascript" },
  jsx: { label: "JavaScript", grammar: "javascript" },
  mjs: { label: "JavaScript", grammar: "javascript" },
  cjs: { label: "JavaScript", grammar: "javascript" },
  rs: { label: "Rust", grammar: "rust" },
  py: { label: "Python", grammar: "python" },
  json: { label: "JSON", grammar: "json" },
  md: { label: "Markdown", grammar: "markdown" },
  html: { label: "HTML", grammar: "xml" },
  toml: { label: "TOML", grammar: "ini" },
  yml: { label: "YAML", grammar: "yaml" },
  yaml: { label: "YAML", grammar: "yaml" },
  sh: { label: "Shell", grammar: "bash" },
  bash: { label: "Shell", grammar: "bash" },
  zsh: { label: "Shell", grammar: "bash" },
  go: { label: "Go", grammar: "go" },
  c: { label: "C", grammar: "c" },
  h: { label: "C", grammar: "c" },
  cpp: { label: "C++", grammar: "cpp" },
  hpp: { label: "C++", grammar: "cpp" },
  cs: { label: "C#", grammar: "csharp" },
  java: { label: "Java", grammar: "java" },
  kt: { label: "Kotlin", grammar: "kotlin" },
  swift: { label: "Swift", grammar: "swift" },
  rb: { label: "Ruby", grammar: "ruby" },
  php: { label: "PHP", grammar: "php" },
  lua: { label: "Lua", grammar: "lua" },
  sql: { label: "SQL", grammar: "sql" },
  ps1: { label: "PowerShell", grammar: "powershell" },
  xml: { label: "XML", grammar: "xml" },
  svg: { label: "SVG", grammar: "xml" },
  scss: { label: "SCSS", grammar: "scss" },
  graphql: { label: "GraphQL", grammar: "graphql" },
  diff: { label: "Diff", grammar: "diff" },
  patch: { label: "Diff", grammar: "diff" },
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

/**
 * Highlights the lines and returns one HTML string per line. The source text is escaped, so the
 * output is safe to inject. A grammar that is not registered leaves the lines plain.
 */
export function highlightLines(lines: string[], grammar: string): string[] {
  const lang = shikiLanguage(grammar);
  if (!lang) return lines.map(escapeHtml);
  return shikiLines(lines.join("\n"), lang);
}

/** Highlighted HTML for a whole code block, or null when the language is not registered. */
export function highlightBlock(code: string, lang: string): string | null {
  const grammar = shikiLanguage(lang);
  if (!grammar) return null;
  return shikiLines(code, grammar).join("\n");
}

/** How many of a grammar's keywords and patterns the code matches, as highlight.js scores it. */
export function grammarRelevance(code: string, lang: string): number {
  if (!hljs.getLanguage(lang)) return 0;
  return hljs.highlight(code, { language: lang, ignoreIllegals: true }).relevance;
}
