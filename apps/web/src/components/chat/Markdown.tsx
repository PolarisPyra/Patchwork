import { Children, memo, useEffect, useState, type ReactElement, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { TextAlignLeftIcon } from "@phosphor-icons/react";
import { SYNTAX_CLASSES } from "~/components/canvas/syntaxClasses";
import { ACTION_CLASS, CopyButton } from "~/components/chat/CopyButton";
import { CHOICES_LANG } from "~/components/chat/choices";

/** Links are shown as text unless they use one of these protocols. Nothing is opened from a reply. */
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
      <span className="cursor-text text-accent-text" title={href}>
        {children}
      </span>
    ) : (
      <span>{children}</span>
    ),
  // Remote images are not loaded from a reply; the alt text stands in for them.
  img: ({ alt }) => <span className="text-fg-muted">{alt || "Image"}</span>,
};

/** Highlighting waits this long after a block last changed, so a block still streaming is not re-highlighted every frame. */
const HIGHLIGHT_DELAY_MS = 150;

export default memo(function Markdown({ text }: { text: string }) {
  // Each block parses on its own, so a streaming reply re-parses only its last block.
  return (
    <div className="min-w-0 leading-[1.6] [word-break:break-word] selectable [&>:last-child]:mb-0 [&_p]:mt-0 [&_p]:mb-2">
      {splitBlocks(text).map((block, i) => (
        <MarkdownBlock key={i} text={block} />
      ))}
    </div>
  );
});

const REMARK_PLUGINS = [remarkGfm];
/** Blocks whose rendered output is kept, most recently used last. */
const PARSED_LIMIT = 1000;
const parsed = new Map<string, ReactElement>();
/** The block parsed last; a streaming block replaces it as it grows, so its drafts do not crowd the cache. */
let lastParsed = "";

/** A block's rendered output, kept so a row the chat list remounts while scrolling is not parsed again. */
function parseBlock(text: string): ReactElement {
  let element = parsed.get(text);
  if (element) {
    parsed.delete(text);
  } else {
    // react-markdown's synchronous renderer uses no hooks, so its output can be reused.
    element = ReactMarkdown({
      children: text,
      remarkPlugins: REMARK_PLUGINS,
      components,
      skipHtml: true,
    });
    if (lastParsed && text.startsWith(lastParsed)) parsed.delete(lastParsed);
    if (parsed.size >= PARSED_LIMIT) parsed.delete(parsed.keys().next().value!);
    lastParsed = text;
  }
  parsed.set(text, element);
  return element;
}

const MarkdownBlock = memo(function MarkdownBlock({ text }: { text: string }) {
  return parseBlock(text);
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
