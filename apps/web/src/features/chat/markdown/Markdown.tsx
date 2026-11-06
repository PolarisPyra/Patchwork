import {
  Children,
  cloneElement,
  isValidElement,
  memo,
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
  type Ref,
} from "react";
import ReactMarkdown, { type Components, type Options } from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import "katex/dist/katex.min.css";
import { TextAlignLeftIcon } from "@phosphor-icons/react";
import { api } from "~/api";
import { highlightBlock, MAX_HIGHLIGHT_LINES, SYNTAX_CLASSES } from "~/lib/highlight";
import Artifact, { isArtifact } from "~/features/chat/Artifact";
import { ACTION_CLASS, CopyButton } from "~/features/chat/markdown/CopyButton";
import { CHOICES_LANG } from "~/features/chat/lib/choices";
import {
  AGENT_CLASS,
  GOAL_CLASS,
  MENTION_CLASS,
  SKILL_CLASS,
} from "~/features/chat/lib/composerExtensions";
import { ANIMATED_LIMIT, wordDelay } from "~/features/chat/lib/streamReveal";

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
    <h3 data-depth={1} className="mt-3 mb-1.5 text-[length:var(--fs-lg)] font-semibold">
      {children}
    </h3>
  ),
  h2: ({ children }) => (
    <h4 data-depth={2} className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">
      {children}
    </h4>
  ),
  h3: ({ children }) => (
    <h5 data-depth={3} className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">
      {children}
    </h5>
  ),
  h4: ({ children }) => (
    <h5 data-depth={4} className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">
      {children}
    </h5>
  ),
  h5: ({ children }) => (
    <h5 data-depth={5} className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">
      {children}
    </h5>
  ),
  h6: ({ children }) => (
    <h5 data-depth={6} className="mt-3 mb-1.5 text-[length:var(--fs-md)] font-semibold">
      {children}
    </h5>
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
    const title = codeTitle(meta);
    // A diagram or an artifact is drawn rather than shown, whichever provider wrote it.
    if (isArtifact(lang)) return <Artifact lang={lang} code={code} title={title} />;
    return <CodeBlock lang={lang} code={code} title={title} />;
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
  streamed = false,
  animate = true,
  containerRef,
}: {
  text: string;
  /** The user wrote the text, so an indented line is pasted text rather than a code block. */
  typed?: boolean;
  /** Shows `/goal`, `$skill`, `&agent` and `@path` as the chips the composer showed for them. */
  chips?: boolean;
  /** The text is still arriving, so words it has just gained fade in. */
  streamed?: boolean;
  /** The words this render adds fade in; false shows them at once, for a step that is a burst. */
  animate?: boolean;
  /** The block's own element, for a caller that has to measure the text it grows. */
  containerRef?: Ref<HTMLDivElement>;
}) {
  // Where the last render ended, so only the words past it fade: the whole text is not re-shown,
  // and a row the chat list remounts mid-reply does not replay the fade for words already read.
  const revealed = useRef(-1);
  useEffect(() => {
    revealed.current = streamed ? text.length : 0;
  });
  const from = !streamed || revealed.current < 0 ? text.length : revealed.current;
  const blocks = splitBlocks(text);
  // Each block parses on its own, so a streaming reply re-parses only its last block.
  // `data-markdown` marks where a copy can read the Markdown back (`lib/selectionMarkdown.ts`).
  return (
    <div
      ref={containerRef}
      data-markdown
      className="md-blocks min-w-0 leading-[1.6] [word-break:break-word] selectable [&>:last-child]:mb-0 [&_p]:mt-0 [&_p]:mb-2 [&_.katex-display]:my-2 [&_.katex-display]:overflow-x-auto [&_.katex-display]:overflow-y-hidden [&_.katex-display]:py-1"
    >
      {blocks.map((block, i) => (
        <MarkdownBlock
          key={i}
          text={block.text}
          typed={typed}
          chips={chips}
          // Only the last block grows while a reply streams; the ones before it are settled.
          animateFrom={i === blocks.length - 1 ? Math.max(0, from - block.at) : block.text.length}
          animate={animate}
        />
      ))}
    </div>
  );
});

/**
 * Math is written $$tex$$, inline or on lines of its own. Single-dollar math stays off: a coding
 * chat is full of dollar signs, and remark-math reads "costs $5 and $10" or "set $HOME and $PATH"
 * as a formula, since it does not check for a space before the closing dollar. So $x$ shows as
 * typed.
 */
const REMARK_PLUGINS: Options["remarkPlugins"] = [
  remarkGfm,
  [remarkMath, { singleDollarTextMath: false }],
];

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
/** The text of the first descendant that is the TeX annotation KaTeX writes into its MathML. */
function annotationTex(node: HastNode): string | null {
  for (const child of node.children ?? []) {
    if (child.tagName === "annotation") return child.children?.[0]?.value ?? null;
    const tex = annotationTex(child);
    if (tex !== null) return tex;
  }
  return null;
}

/**
 * Puts each formula's TeX on its outermost KaTeX element as `data-tex`, which a copy takes in
 * place of the two renderings KaTeX draws, even when only part of the formula is selected.
 */
function rehypeMathSource() {
  const walk = (node: HastNode) => {
    for (const child of node.children ?? []) {
      const classes = child.properties?.className;
      if (
        Array.isArray(classes) &&
        (classes.includes("katex-display") || classes.includes("katex"))
      ) {
        const tex = annotationTex(child);
        if (tex !== null) child.properties = { ...child.properties, dataTex: tex };
      } else {
        walk(child);
      }
    }
  };
  return walk;
}

/** A combining mark with no letter before it: at the start of a text, or after a space. */
const BARE_MARK = /(^|\s)(\p{M})/gu;
/** An enclosing mark, such as the slashed circle U+20E0, alone in the same way. */
const BARE_ENCLOSING_MARK = /(^|\s)(\p{Me})/u;

/** How far each enclosing mark's ink reaches left and right of where it is placed, in em. */
const markReach = new Map<string, [number, number]>();

/** Measures once how far `mark` reaches, in the window's own fonts. Fonts differ: one centers a
 * slashed circle on its place, another draws a circle wholly to the left of it. */
function reachOf(mark: string): [number, number] {
  let reach = markReach.get(mark);
  if (!reach) {
    const context = document.createElement("canvas").getContext("2d");
    reach = [0, 0];
    if (context) {
      context.font = `100px ${getComputedStyle(document.body).fontFamily}`;
      const ink = context.measureText(mark);
      reach = [
        Math.max(0, ink.actualBoundingBoxLeft) / 100,
        Math.max(0, ink.actualBoundingBoxRight) / 100,
      ];
    }
    markReach.set(mark, reach);
  }
  return reach;
}

/**
 * A combining mark is drawn over the character before it and takes no room of its own, so one
 * written alone after a space overlaps the words beside it. An enclosing mark, such as the slashed
 * circle U+20E0, gets a box padded out to the ink it draws. Any other mark gets the dotted circle
 * U+25CC to sit on, which is how Unicode shows a mark alone. Code and formulas stay as written.
 */
function rehypeMarkBases() {
  const split = (value: string): HastNode[] => {
    const found = BARE_ENCLOSING_MARK.exec(value);
    if (!found) return [{ type: "text", value: value.replace(BARE_MARK, "$1\u25CC$2") }];
    const end = found.index + found[0].length;
    const before = value.slice(0, found.index) + found[1];
    const [left, right] = reachOf(found[2]);
    return [
      ...(before ? [{ type: "text", value: before }] : []),
      {
        type: "element",
        tagName: "span",
        properties: {
          className: ["inline-block"],
          style: `padding-left:${left}em;padding-right:${right}em`,
        },
        children: [{ type: "text", value: found[2] }],
      },
      ...split(value.slice(end)),
    ];
  };
  const walk = (node: HastNode): void => {
    if (!node.children) return;
    node.children = node.children.flatMap((child) => {
      if (child.type === "text") return split(child.value ?? "");
      if (!unwrapped(child)) walk(child);
      return [child];
    });
  };
  return walk;
}
const REHYPE_PLUGINS = [rehypeKatex, rehypeMathSource, rehypeTrimBreaks, rehypeMarkBases];

/** A `$skill` anywhere in the text, after a space; skill names are lowercase words joined by
 * hyphens, so `$HOME` or `$5` stays text. */
const SKILL_TOKEN = /(^|\s)(\$[a-z][a-z0-9]*(?:[-_:][a-z0-9]+)*)(?=[\s.,!?;:)]|$)/g;

/** A summoned agent, `&name` after a space, as the composer writes it. */
const AGENT_TOKEN = /(^|\s)(&[A-Za-z][A-Za-z0-9]*(?:[-_][A-Za-z0-9]+)*)(?=[\s.,!?;:)]|$)/g;

/** A file mention, `@path` or `@<path with spaces>` after a space, as the composer writes it;
 * punctuation after a bare one stays text. Only a bare path with a slash or a dot counts, so
 * `@name` stays text. */
const MENTION_TOKEN = /(^|\s)(@<[^<>\n]+>|@\S*[^\s.,!?;:)])/g;

/** The path a mention token names, without its `@` and any angle brackets. */
const mentionPath = (token: string) =>
  token.startsWith("@<") ? token.slice(2, -1) : token.slice(1);

function chip(
  label: string,
  className: string,
  properties: Record<string, unknown> = {},
): HastNode {
  return {
    type: "element",
    tagName: "span",
    properties: { className, ...properties },
    children: [{ type: "text", value: label }],
  };
}

/** A text node cut into text and chips: `/goal` when the text starts with it, each `$skill` and
 * `&agent`, and each `@path`, which carries its path for the click that opens it. */
function chipPieces(value: string, start: boolean): HastNode[] {
  const pieces: HastNode[] = [];
  let last = 0;
  const goal = start ? /^(\s*)\/goal(?=\s|$)/.exec(value) : null;
  if (goal) {
    if (goal[1]) pieces.push({ type: "text", value: goal[1] });
    pieces.push(chip("/goal", GOAL_CLASS));
    last = goal[0].length;
  }
  const tokens = [
    ...[...value.matchAll(SKILL_TOKEN)].map((match) => ({
      match,
      node: chip(match[2], SKILL_CLASS),
    })),
    ...[...value.matchAll(AGENT_TOKEN)].map((match) => ({
      match,
      node: chip(match[2], AGENT_CLASS),
    })),
    ...[...value.matchAll(MENTION_TOKEN)]
      .filter((match) => match[2].startsWith("@<") || /[./]/.test(mentionPath(match[2])))
      .map((match) => {
        const path = mentionPath(match[2]);
        return {
          match,
          node: chip(`@${path}`, MENTION_CLASS, {
            dataMention: path,
            title: path,
            role: "button",
            tabIndex: 0,
            ariaLabel: `Reveal ${path}`,
          }),
        };
      }),
  ].sort((a, b) => a.match.index - b.match.index);
  for (const { match, node } of tokens) {
    const at = match.index + match[1].length;
    if (at < last) continue;
    if (at > last) pieces.push({ type: "text", value: value.slice(last, at) });
    pieces.push(node);
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
const CHIP_REHYPE_PLUGINS = [rehypeTrimBreaks, rehypeChips, rehypeMarkBases];

/** Elements whose text arrives whole, so a word inside one is never wrapped or faded. */
const UNWRAPPED = new Set(["code", "pre", "a", "kbd", "samp", "button", "input", "svg", "math"]);

/** The characters an element renders as text, which is what a word's offset counts. */
function textLength(node: HastNode): number {
  if (node.type === "text") return (node.value ?? "").length;
  return (node.children ?? []).reduce((total, child) => total + textLength(child), 0);
}

/** A formula, or any other element whose text must be left as it is. */
function unwrapped(node: HastNode): boolean {
  if (node.type !== "element") return false;
  if (UNWRAPPED.has(node.tagName ?? "")) return true;
  if (node.properties?.dataTex !== undefined) return true;
  const classes = node.properties?.className;
  return Array.isArray(classes) && classes.includes("katex");
}

/**
 * Wraps each word of prose in a span, so a word that has just been revealed can fade in.
 *
 * Words before `from` characters into the block are wrapped too, so the elements keep their place
 * from one render to the next, but only the ones after it animate: a word already on screen must
 * not fade a second time. `from` counts the block's text as written, which is a few characters
 * longer than what renders once emphasis markers are gone, so the boundary can land a word early;
 * that is not visible.
 *
 * It also collects the block's rendered text and whether it wrapped anything, which the block uses
 * for the copy a screen reader reads instead of the animated one.
 */
function rehypeWordFade(from: number) {
  const state: WordFadeState = { wrapped: false, plain: "" };
  const plugin = () => (tree: HastNode) => {
    let offset = 0;
    /** One text node's worth of prose, each word in its own span. */
    const words = (value: string): HastNode[] => {
      const out: HastNode[] = [];
      state.plain += value;
      for (const piece of value.split(/(\s+)/)) {
        if (!piece) continue;
        const animate = offset >= from;
        const delay = wordDelay(offset - from);
        offset += piece.length;
        if (!piece.trim()) {
          out.push({ type: "text", value: piece });
          continue;
        }
        if (animate) state.wrapped = true;
        out.push({
          type: "element",
          tagName: "span",
          properties: animate
            ? { className: ["stream-word", "stream-word-in"], style: `animation-delay:${delay}ms` }
            : { className: ["stream-word"] },
          children: [{ type: "text", value: piece }],
        });
      }
      return out;
    };
    const walk = (node: HastNode): void => {
      if (!node.children) return;
      node.children = node.children.flatMap((child) => {
        if (child.type === "text") return words(child.value ?? "");
        if (unwrapped(child)) {
          offset += textLength(child);
          state.plain += plainText(child);
          return [child];
        }
        walk(child);
        return [child];
      });
    };
    walk(tree);
  };
  return { plugin, state };
}

/** What a block's word fade wrapped, and the text it rendered, for the screen-reader copy. */
interface WordFadeState {
  /** A word was wrapped and will animate. */
  wrapped: boolean;
  /** The block's text as it renders, without the Markdown around it. */
  plain: string;
}

/** The text a node renders, as a screen reader would read it out. */
function plainText(node: HastNode): string {
  if (node.type === "text") return node.value ?? "";
  const tex = node.properties?.dataTex;
  if (tex !== undefined) return String(tex);
  // A drawing or a formula's MathML carries its own text in glyph order, which reads as nonsense.
  if (node.tagName === "svg" || node.tagName === "math") return "";
  return (node.children ?? []).map(plainText).join("");
}

/** Blocks whose rendered output is kept, most recently used last. */
const PARSED_LIMIT = 1000;
const parsed = new Map<string, ReactElement>();
/** The block parsed last; a streaming block replaces it as it grows, so its drafts do not crowd the cache. */
let lastParsed = "";

/** A block's rendered output, kept so a row the chat list remounts while scrolling is not parsed again. */
function parseBlock(
  text: string,
  typed: boolean,
  chips: boolean,
  animateFrom: number,
  animate: boolean,
): ReactElement {
  // A block that is still growing is parsed every time it grows, and each of those drafts differs,
  // so it is not kept: the cache would hold every step of the reply.
  const animated = animate && animateFrom < text.length && text.length <= ANIMATED_LIMIT;
  // Typed text and text with chips parse differently, so each is cached under its own key.
  const key = `${chips ? "chips:" : ""}${typed ? "typed:" : ""}${text}`;
  let element = animated ? undefined : parsed.get(key);
  const fade = animated ? rehypeWordFade(animateFrom) : null;
  if (element) {
    parsed.delete(key);
  } else {
    // react-markdown's synchronous renderer uses no hooks, so its output can be reused.
    element = ReactMarkdown({
      children: text,
      remarkPlugins: typed ? TYPED_REMARK_PLUGINS : REMARK_PLUGINS,
      rehypePlugins: [
        ...(chips ? CHIP_REHYPE_PLUGINS : REHYPE_PLUGINS),
        ...(fade ? [fade.plugin] : []),
      ],
      components,
      skipHtml: true,
    });
    if (!animated) {
      if (lastParsed && key.startsWith(lastParsed)) parsed.delete(lastParsed);
      if (parsed.size >= PARSED_LIMIT) parsed.delete(parsed.keys().next().value!);
      lastParsed = key;
      parsed.set(key, element);
      return element;
    }
  }
  // Words fading in one at a time are drawn for the eye: a screen reader gets the block's text
  // once, whole, and the animated copy is hidden from it, so it is not read a word at a time.
  // react-markdown hands back a fragment, so the attribute goes on each node it holds, which
  // leaves the elements the block rendered — and the margin rules that reach for them — as they are.
  if (!fade?.state.wrapped) return element;
  return (
    <>
      <span className="sr-only">{fade.state.plain}</span>
      {Children.map((element as ReactElement<{ children?: ReactNode }>).props.children, (child) =>
        isValidElement(child)
          ? cloneElement(child as ReactElement<{ "aria-hidden"?: boolean }>, {
              "aria-hidden": true,
            })
          : child,
      )}
    </>
  );
}

const MarkdownBlock = memo(function MarkdownBlock({
  text,
  typed,
  chips,
  animateFrom,
  animate,
}: {
  text: string;
  typed: boolean;
  chips: boolean;
  /** Characters of the block that were on screen before this render; words after them fade in. */
  animateFrom: number;
  /** The words past `animateFrom` fade in; false shows them at once. */
  animate: boolean;
}) {
  return parseBlock(text, typed, chips, animateFrom, animate);
});

/** An opening or closing code fence: its marker, then the rest of the line. */
const FENCE = /^ {0,3}(`{3,}|~{3,})(.*)$/;
/** A link reference or footnote definition, which can be used from anywhere in the text. */
const DEFINITION = /^ {0,3}\[[^\]\n]+\]:/m;
/** A line that starts a new top-level block after a blank line and cannot join the one before it:
 * not indented, and not a list item or quote, which would merge with a list or quote above. */
const BLOCK_START = /^(?![-*+>]|\d{1,9}[.)])\S/;

/** A block of Markdown, and where it starts in the text it was cut from. */
interface Block {
  text: string;
  /** Its offset in the source text, which places the words that have just arrived. */
  at: number;
}

/** Cuts Markdown into pieces that render the same parsed apart as together: at blank lines outside
 * code fences, before a line that `BLOCK_START` allows. Text with definitions stays whole. */
function splitBlocks(text: string): Block[] {
  if (DEFINITION.test(text)) return [{ text, at: 0 }];
  const lines = text.split("\n");
  const blocks: Block[] = [];
  let start = 0;
  let at = 0;
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
      blocks.push({ text: lines.slice(start, i).join("\n"), at });
      // The blank line that ends the block is dropped, so the next block starts past it.
      at += lines.slice(start, i + 1).join("\n").length + 1;
      start = i + 1;
    }
  }
  blocks.push({ text: lines.slice(start).join("\n"), at });
  return blocks;
}

function CodeBlock({ lang, code, title }: { lang: string; code: string; title: string | null }) {
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
    const timer = setTimeout(
      () =>
        setHighlighted({
          code,
          // A block too long to highlight stays plain, as it does in the composer and the Canvas.
          html: code.split("\n").length > MAX_HIGHLIGHT_LINES ? null : highlightBlock(code, lang),
        }),
      delay,
    );
    return () => clearTimeout(timer);
  }, [code, lang, upToDate, delay]);
  return (
    <div
      data-code={lang}
      className="my-2 overflow-hidden rounded-[var(--radius)] border border-border bg-panel"
    >
      <div
        data-text-exclude
        className="flex items-center justify-between border-b border-border py-0.5 pr-1 pl-2.5 text-[length:var(--fs-xs)] text-fg-subtle"
      >
        <span className="min-w-0 truncate">{title || lang || "text"}</span>
        <div className="flex items-center">
          <button
            className={`${ACTION_CLASS} aria-pressed:text-fg`}
            aria-pressed={wrap}
            onClick={() => setWrap(!wrap)}
            title="Wrap lines"
            aria-label="Wrap lines"
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
          // The highlighted HTML escapes the source text, so it is safe.
          <code className={SYNTAX_CLASSES} dangerouslySetInnerHTML={{ __html: html }} />
        )}
      </pre>
    </div>
  );
}
