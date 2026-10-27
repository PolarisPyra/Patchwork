import { Children, memo, useEffect, useState, type ReactElement, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { TextAlignLeftIcon } from "@phosphor-icons/react";
import { api } from "~/api";
import { SYNTAX_CLASSES } from "~/components/canvas/syntaxClasses";
import { ACTION_CLASS, CopyButton } from "~/components/chat/CopyButton";
import { CHOICES_LANG } from "~/components/chat/choices";
import { GOAL_CLASS, SKILL_CLASS } from "~/components/chat/composerExtensions";

/** Links open in the system browser only with one of these protocols; others are shown as text. */
const SAFE_LINK = /^(https?:|mailto:)/i;
/** Matches title="...", file=..., or filename='...' in a fence's info string. */
const TITLE_ATTR = /(?:^|\s)(?:title|file|filename)=(?:"([^"]*)"|'([^']*)'|([^\s"']+))/;
/** A bare token that looks like a filename, such as app.tsx or Cargo.toml. */
const FILE_TOKEN = /^[\w@][\w@./-]*\.[A-Za-z0-9]+$/;

/** The block title from a fence's meta, or null when it has none. */
function codeTitle(meta: string | null | undefined): string | null {
  if (!meta) return null;
  const attr = TITLE_ATTR.exec(meta);
  if (attr) return (attr[1] ?? attr[2] ?? attr[3]) || null;
  return meta.split(/\s+/).find((token) => FILE_TOKEN.test(token)) ?? null;
}

const components: Components = {
  h1: ({ children }) => (
    <h3 className="mt-3 mb-1.5 text-[length:var(--fs-lg)] font-semibold">{children}</h3>
  ),
  h2: ({ children }) => (
    <h4 className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">{children}</h4>
  ),
  h3: ({ children }) => (
    <h5 className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">{children}</h5>
  ),
  h4: ({ children }) => (
    <h5 className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">{children}</h5>
  ),
  h5: ({ children }) => (
    <h5 className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">{children}</h5>
  ),
  h6: ({ children }) => (
    <h5 className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">{children}</h5>
  ),
  ul: ({ children }) => <ul className="mt-0 mb-2 pl-[22px]">{children}</ul>,
  ol: ({ children }) => <ol className="mt-0 mb-2 pl-[22px]">{children}</ol>,
  blockquote: ({ children }) => (
    <blockquote className="my-2 border-l-2 border-border pl-3 text-fg-muted">{children}</blockquote>
  ),
  hr: () => <hr className="my-3 border-border" />,
  table: ({ children }) => (
    <div className="my-2 overflow-x-auto">
      <table className="border-collapse text-[length:var(--fs-sm)]">{children}</table>
    </div>
  ),
  th: ({ children }) => (
    <th className="border border-border px-2 py-1 text-left font-semibold">{children}</th>
  ),
  td: ({ children }) => <td className="border border-border px-2 py-1">{children}</td>,
  // Inline code only: fenced blocks are handled by `pre` below.
  code: ({ children }) => (
    <code className="rounded-[4px] bg-selected-muted px-[5px] py-px font-[family-name:var(--font-mono)] text-[12px]">
      {children}
    </code>
  ),
  pre: ({ children, node }) => {
    // remark always gives a fenced block one <code> child, with its language as a class.
    const child = Children.only(children) as ReactElement<{
      className?: string;
      children?: ReactNode;
    }>;
    const lang = /language-([\w+.-]+)/.exec(child.props.className ?? "")?.[1] ?? "";
    // A choices block is shown as a panel above the composer, not as code.
    if (lang === CHOICES_LANG) return null;
    const code = String(child.props.children ?? "").replace(/\n$/, "");
    // The fence's info string after the language is kept on the hast <code> node as data.meta.
    const meta = (node?.children[0] as { data?: { meta?: string | null } } | undefined)?.data?.meta;
    return <CodeBlock lang={lang} code={code} title={codeTitle(meta)} />;
  },
  a: ({ href, children }) =>
    href && SAFE_LINK.test(href) ? (
      <a
        href={href}
        className="text-accent-text underline underline-offset-2"
        title={href}
        onClick={(event) => {
          // The webview must not navigate away; the system browser opens the link instead.
          event.preventDefault();
          void api.openUrl(href);
        }}
      >
        {children}
      </a>
    ) : (
      <span>{children}</span>
    ),
  // Remote images are not loaded from a reply; the alt text stands in for them.
  img: ({ alt }) => <span className="text-fg-muted">{alt || "Image"}</span>,
};

/** Highlighting waits this long after a block last changed, so a block still streaming is not re-highlighted every frame. */
const HIGHLIGHT_DELAY_MS = 150;

export default memo(function Markdown({
  text,
  typed = false,
  chips = false,
}: {
  text: string;
  /** The user wrote the text, so an indented line is pasted text rather than a code block. */
  typed?: boolean;
  /** Shows `/goal` and `$skill` as the chips the composer showed for them. */
  chips?: boolean;
}) {
  // Each block parses on its own, so a streaming reply re-parses only its last block.
  return (
    <div className="min-w-0 leading-[1.6] [word-break:break-word] selectable [&>:last-child]:mb-0 [&_p]:mt-0 [&_p]:mb-2">
      {splitBlocks(text).map((block, i) => (
        <MarkdownBlock key={i} text={block} typed={typed} chips={chips} />
      ))}
    </div>
  );
});

const REMARK_PLUGINS = [remarkGfm];

/** Turns off indented code blocks; fenced blocks still render as code. */
function remarkNoIndentedCode(this: { data: () => object }) {
  const data = this.data() as { micromarkExtensions?: object[] };
  (data.micromarkExtensions ??= []).push({ disable: { null: ["codeIndented"] } });
}
const TYPED_REMARK_PLUGINS = [remarkGfm, remarkNoIndentedCode];

type HastNode = {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: HastNode[];
};

/**
 * A hard break renders as `<br>` followed by the newline that ended its line. Where paragraphs
 * keep their newlines, as in the user's bubble, that newline would show as a second break, so it
 * is dropped.
 */
function rehypeTrimBreaks() {
  const walk = (node: HastNode) => {
    node.children?.forEach((child, i, siblings) => {
      const next = siblings[i + 1];
      if (child.tagName === "br" && next?.type === "text" && next.value?.startsWith("\n")) {
        next.value = next.value.slice(1);
      }
      walk(child);
    });
  };
  return walk;
}
const REHYPE_PLUGINS = [rehypeTrimBreaks];

/** A `$skill` anywhere in the text, after a space; skill names are lowercase words joined by
 * hyphens, so `$HOME` or `$5` stays text. */
const SKILL_TOKEN = /(^|\s)(\$[a-z][a-z0-9]*(?:[-_:][a-z0-9]+)*)(?=[\s.,!?;:)]|$)/g;

function chip(label: string, className: string): HastNode {
  return {
    type: "element",
    tagName: "span",
    properties: { className },
    children: [{ type: "text", value: label }],
  };
}

/** A text node cut into text and chips: `/goal` when the text starts with it, and each `$skill`. */
function chipPieces(value: string, start: boolean): HastNode[] {
  const pieces: HastNode[] = [];
  let last = 0;
  const goal = start ? /^(\s*)\/goal(?=\s|$)/.exec(value) : null;
  if (goal) {
    if (goal[1]) pieces.push({ type: "text", value: goal[1] });
    pieces.push(chip("/goal", GOAL_CLASS));
    last = goal[0].length;
  }
  for (const match of value.matchAll(SKILL_TOKEN)) {
    const at = match.index + match[1].length;
    if (at < last) continue;
    if (at > last) pieces.push({ type: "text", value: value.slice(last, at) });
    pieces.push(chip(match[2], SKILL_CLASS));
    last = at + match[2].length;
  }
  if (last < value.length) pieces.push({ type: "text", value: value.slice(last) });
  return pieces;
}

/** Puts the chips into the text, outside code and links. */
function rehypeChips() {
  return (tree: HastNode) => {
    let start = true;
    const walk = (node: HastNode) => {
      node.children = node.children?.flatMap((child) => {
        if (child.type === "text") {
          const pieces = chipPieces(child.value ?? "", start);
          if (child.value?.trim()) start = false;
          return pieces;
        }
        if (child.tagName === "code" || child.tagName === "pre" || child.tagName === "a") {
          start = false;
        } else {
          walk(child);
        }
        return [child];
      });
    };
    walk(tree);
  };
}
const CHIP_REHYPE_PLUGINS = [rehypeTrimBreaks, rehypeChips];
/** Blocks whose rendered output is kept, most recently used last. */
const PARSED_LIMIT = 1000;
const parsed = new Map<string, ReactElement>();
/** The block parsed last; a streaming block replaces it as it grows, so its drafts do not crowd the cache. */
let lastParsed = "";

/** A block's rendered output, kept so a row the chat list remounts while scrolling is not parsed again. */
function parseBlock(text: string, typed: boolean, chips: boolean): ReactElement {
  // Typed text and text with chips parse differently, so each is cached under its own key.
  const key = `${chips ? "chips:" : ""}${typed ? "typed:" : ""}${text}`;
  let element = parsed.get(key);
  if (element) {
    parsed.delete(key);
  } else {
    // react-markdown's synchronous renderer uses no hooks, so its output can be reused.
    element = ReactMarkdown({
      children: text,
      remarkPlugins: typed ? TYPED_REMARK_PLUGINS : REMARK_PLUGINS,
      rehypePlugins: chips ? CHIP_REHYPE_PLUGINS : REHYPE_PLUGINS,
      components,
      skipHtml: true,
    });
    if (lastParsed && key.startsWith(lastParsed)) parsed.delete(lastParsed);
    if (parsed.size >= PARSED_LIMIT) parsed.delete(parsed.keys().next().value!);
    lastParsed = key;
  }
  parsed.set(key, element);
  return element;
}

const MarkdownBlock = memo(function MarkdownBlock({
  text,
  typed,
  chips,
}: {
  text: string;
  typed: boolean;
  chips: boolean;
}) {
  return parseBlock(text, typed, chips);
});

/** An opening or closing code fence: its marker, then the rest of the line. */
const FENCE = /^ {0,3}(`{3,}|~{3,})(.*)$/;
/** A link reference or footnote definition, which can be used from anywhere in the text. */
const DEFINITION = /^ {0,3}\[[^\]\n]+\]:/m;
/** A line that starts a new top-level block after a blank line and cannot join the one before it:
 * not indented, and not a list item or quote, which would merge with a list or quote above. */
const BLOCK_START = /^(?![-*+>]|\d{1,9}[.)])\S/;

/** Cuts Markdown into pieces that render the same parsed apart as together: at blank lines outside
 * code fences, before a line that `BLOCK_START` allows. Text with definitions stays whole. */
function splitBlocks(text: string): string[] {
  if (DEFINITION.test(text)) return [text];
  const lines = text.split("\n");
  const blocks: string[] = [];
  let start = 0;
  let fence: string | null = null;
  for (let i = 0; i < lines.length; i++) {
    const marker = FENCE.exec(lines[i]);
    if (fence !== null) {
      const closes =
        marker &&
        marker[1][0] === fence[0] &&
        marker[1].length >= fence.length &&
        !marker[2].trim();
      if (closes) fence = null;
    } else if (marker) {
      fence = marker[1];
    } else if (
      i > start &&
      !lines[i].trim() &&
      i + 1 < lines.length &&
      BLOCK_START.test(lines[i + 1])
    ) {
      blocks.push(lines.slice(start, i).join("\n"));
      start = i + 1;
    }
  }
  blocks.push(lines.slice(start).join("\n"));
  return blocks;
}

function CodeBlock({ lang, code, title }: { lang: string; code: string; title: string | null }) {
  // Highlighting loads on demand, so the chat does not pull every grammar up front.
  // The highlighted HTML is kept with the code it was made from; newer code shows plain until highlighted.
  const [highlighted, setHighlighted] = useState<{ code: string; html: string | null } | null>(
    null,
  );
  // Long lines wrap by default; the toggle switches back to horizontal scroll.
  const [wrap, setWrap] = useState(true);
  const upToDate = highlighted?.code === code;
  const html = upToDate ? highlighted.html : null;
  // A block is highlighted at once when it first shows, then only once it stops changing.
  const delay = highlighted === null ? 0 : HIGHLIGHT_DELAY_MS;
  useEffect(() => {
    if (upToDate) return;
    let current = true;
    const timer = setTimeout(
      () =>
        void import("~/components/canvas/highlight").then(({ highlightBlock }) => {
          if (current) setHighlighted({ code, html: highlightBlock(code, lang) });
        }),
      delay,
    );
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [code, lang, upToDate, delay]);
  return (
    <div className="my-2 overflow-hidden rounded-[var(--radius)] border border-border bg-panel">
      <div className="flex items-center justify-between border-b border-border py-0.5 pr-1 pl-2.5 text-[length:var(--fs-xs)] text-fg-subtle">
        <span className="min-w-0 truncate">{title || lang || "text"}</span>
        <div className="flex items-center">
          <button
            className={`${ACTION_CLASS} aria-pressed:text-fg`}
            aria-pressed={wrap}
            onClick={() => setWrap(!wrap)}
            title="Wrap lines"
          >
            <TextAlignLeftIcon size={13} />
          </button>
          <CopyButton text={code} />
        </div>
      </div>
      <pre
        className={`m-0 px-3 py-2.5 text-[12px] leading-[1.5] ${wrap ? "break-words whitespace-pre-wrap" : "overflow-x-auto"}`}
      >
        {html === null ? (
          <code>{code}</code>
        ) : (
          // hljs output escapes the source text, so this HTML is safe.
          <code className={SYNTAX_CLASSES} dangerouslySetInnerHTML={{ __html: html }} />
        )}
      </pre>
    </div>
  );
}
