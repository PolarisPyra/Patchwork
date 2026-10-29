/**
 * Copying from a rendered reply puts the selection on the clipboard as the Markdown it was
 * rendered from, so headings, lists, code and links survive a paste into the composer or another
 * app. The browser would copy only the rendered text.
 *
 * The HTML is the one `Markdown.tsx` renders, which marks what the tags lose: a heading's level
 * (`data-depth`), a code block's language (`data-code`), a formula's TeX (`data-tex`), and the parts
 * that are only chrome (`data-copy-skip`).
 */

/** The attribute on a copy's HTML that says it came from a reply, as Markdown in its text. */
export const MARKDOWN_COPY_ATTR = "data-patchwork-markdown";

const BLOCK_TAGS = new Set([
  "P",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "UL",
  "OL",
  "LI",
  "BLOCKQUOTE",
  "PRE",
  "HR",
  "TABLE",
  "DIV",
]);

/** A display formula sits among the blocks as a span; it copies as a block of its own. */
const isBlock = (node: Node) =>
  node instanceof HTMLElement &&
  (BLOCK_TAGS.has(node.tagName) || node.classList.contains("katex-display"));

/** Inline content as Markdown: emphasis, code, links and line breaks. */
function inline(nodes: Iterable<Node>): string {
  let out = "";
  for (const node of nodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      out += node.textContent ?? "";
      continue;
    }
    if (!(node instanceof HTMLElement) || node.hasAttribute("data-copy-skip")) continue;
    // KaTeX draws a formula twice, as MathML and as HTML; it copies once, as the TeX it came from.
    const tex = node.getAttribute("data-tex");
    if (tex !== null) {
      out += node.classList.contains("katex-display") ? `$$\n${tex}\n$$` : `$$${tex}$$`;
      continue;
    }
    const inner = () => inline(node.childNodes);
    switch (node.tagName) {
      case "STRONG":
      case "B":
        out += `**${inner()}**`;
        break;
      case "EM":
      case "I":
        out += `*${inner()}*`;
        break;
      case "DEL":
      case "S":
        out += `~~${inner()}~~`;
        break;
      case "CODE": {
        const text = node.textContent ?? "";
        const ticks = "`".repeat(
          (text.match(/`+/g) ?? [""]).reduce((n, t) => Math.max(n, t.length), 0) + 1,
        );
        out += ticks.length > 1 ? `${ticks} ${text} ${ticks}` : `\`${text}\``;
        break;
      }
      case "A": {
        const href = node.getAttribute("href");
        out += href ? `[${inner()}](${href})` : inner();
        break;
      }
      case "BR":
        out += "  \n";
        break;
      case "INPUT":
      case "BUTTON":
        break;
      default:
        out += inner();
    }
  }
  return out;
}

/** Block content as Markdown, blocks apart by `gap`: a blank line, or a newline inside a tight
 * list item. Inline nodes between blocks, as in a
 * tight list item, form a paragraph of their own. */
function blocks(nodes: Iterable<Node>, gap = "\n\n"): string {
  const out: string[] = [];
  let run: Node[] = [];
  const flush = () => {
    const text = inline(run).trim();
    if (text) out.push(text);
    run = [];
  };
  for (const node of nodes) {
    if (!isBlock(node)) {
      run.push(node);
      continue;
    }
    flush();
    const text = block(node as HTMLElement);
    if (text.trim()) out.push(text);
  }
  flush();
  return out.join(gap);
}

const indent = (text: string, by: string) =>
  text
    .split("\n")
    .map((line, i) => (i === 0 || !line ? line : by + line))
    .join("\n");

/** `code` in a fence longer than any run of backticks inside it. */
function fence(code: string, info: string): string {
  const ticks = "`".repeat(Math.max(3, ...(code.match(/`{3,}/g) ?? []).map((f) => f.length + 1)));
  return `${ticks}${info}\n${code}\n${ticks}`;
}

function block(el: HTMLElement): string {
  if (el.hasAttribute("data-copy-skip")) return "";
  if (el.hasAttribute("data-tex")) return inline([el]);
  if (el.hasAttribute("data-code")) {
    const code = (el.querySelector("pre")?.textContent ?? "").replace(/\n$/, "");
    return fence(code, el.getAttribute("data-code") ?? "");
  }
  // A drawn diagram or artifact copies as its source.
  if (el.hasAttribute("data-artifact")) {
    const source = el.getAttribute("data-artifact-source") ?? "";
    return fence(source, el.getAttribute("data-artifact") ?? "");
  }
  switch (el.tagName) {
    case "P":
      return inline(el.childNodes).trim();
    case "H1":
    case "H2":
    case "H3":
    case "H4":
    case "H5":
    case "H6": {
      const depth = Number(el.getAttribute("data-depth")) || Number(el.tagName[1]);
      return `${"#".repeat(depth)} ${inline(el.childNodes).trim()}`;
    }
    case "UL":
    case "OL": {
      const start = Number(el.getAttribute("start")) || 1;
      const items = [...el.children].filter((c): c is HTMLElement => c.tagName === "LI");
      // Items with a blank line between them, as the source had, render with paragraphs.
      const loose = items.some((li) => [...li.children].some((c) => c.tagName === "P"));
      return items
        .map((li, i) => {
          const marker = el.tagName === "OL" ? `${start + i}. ` : "- ";
          const box = li.querySelector(":scope > input[type=checkbox]") as HTMLInputElement | null;
          const task = box ? (box.checked ? "[x] " : "[ ] ") : "";
          return (
            marker +
            task +
            indent(blocks(li.childNodes, loose ? "\n\n" : "\n"), " ".repeat(marker.length))
          );
        })
        .join(loose ? "\n\n" : "\n");
    }
    case "LI":
      return blocks(el.childNodes);
    case "BLOCKQUOTE":
      return blocks(el.childNodes)
        .split("\n")
        .map((line) => (line ? `> ${line}` : ">"))
        .join("\n");
    case "PRE":
      return `\`\`\`\n${(el.textContent ?? "").replace(/\n$/, "")}\n\`\`\``;
    case "HR":
      return "---";
    case "TABLE":
    case "THEAD":
    case "TBODY":
    case "TR": {
      const trs = el.tagName === "TR" ? [el] : [...el.querySelectorAll("tr")];
      const rows = trs.map(
        (tr) =>
          `| ${[...tr.children].map((cell) => inline(cell.childNodes).trim().replace(/\|/g, "\\|")).join(" | ")} |`,
      );
      if (rows.length === 0) return "";
      const width = trs[0].children.length;
      rows.splice(1, 0, `|${" --- |".repeat(width)}`);
      return rows.join("\n");
    }
    default:
      return blocks(el.childNodes);
  }
}

const isCodeBlock = (node: Node): node is HTMLElement =>
  node instanceof HTMLElement && node.hasAttribute("data-code");

/** The Markdown of the selection inside `root`, a rendered reply, or null when the browser's own
 * copy is better: a selection that leaves the reply, or one inside a code block, whose text is
 * already exact. */
export function selectionMarkdown(root: HTMLElement): { markdown: string; html: string } | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  const common = range.commonAncestorContainer;
  const container = common instanceof HTMLElement ? common : common.parentElement;
  if (!container || !root.contains(container) || container.closest("pre, [data-code]")) {
    return null;
  }
  // Part of a formula copies as the whole formula.
  const formula = container.closest("[data-tex]");
  if (formula) {
    return {
      markdown: inline([formula]),
      html: `<div ${MARKDOWN_COPY_ATTR}="">${formula.outerHTML}</div>`,
    };
  }
  // The selection keeps the element it sits in, so a list item keeps its list and a cell its table.
  const wrapper = container.cloneNode(false) as HTMLElement;
  wrapper.append(range.cloneContents());
  // A selection that reaches a code block from outside it (a triple-click, say) and holds only
  // code copies as that code, with no fence.
  const nodes = [...wrapper.childNodes];
  const codes = nodes.filter(isCodeBlock);
  if (codes.length > 0 && nodes.every((node) => isCodeBlock(node) || !node.textContent?.trim())) {
    const code = codes
      .map((el) => (el.querySelector("pre")?.textContent ?? "").replace(/\n$/, ""))
      .join("\n\n");
    const pre = document.createElement("pre");
    pre.textContent = code;
    return { markdown: code, html: pre.outerHTML };
  }
  // Part of one paragraph, heading or item is copied as its text, without the block's marker.
  const markdown = /^(P|H\d|LI|SPAN|STRONG|EM|A|CODE|DEL)$/.test(wrapper.tagName)
    ? inline(wrapper.childNodes)
    : block(wrapper);
  if (!markdown.trim()) return null;
  return { markdown, html: `<div ${MARKDOWN_COPY_ATTR}="">${wrapper.outerHTML}</div>` };
}

/** A formula as a copy writes it, `$$tex$$`, inline or over lines of its own. */
const FORMULA = /\$\$[\s\S]+?\$\$/g;
/** A backslash before ASCII punctuation, which Markdown reads as an escape. */
const ESCAPE = /\\(?=[!-/:-@[-`{-~])/g;
/** An opening or closing code fence: its marker, then the rest of the line. */
const FENCE_LINE = /^ {0,3}(`{3,}|~{3,})(.*)$/;

/**
 * A copy's Markdown for a parser that has no math, as the composer's: a formula's backslashes
 * before punctuation, as in `\,` or `\{`, are doubled, so the parser keeps them rather than
 * reading them as escapes. Code fences are left as they are.
 */
export function keepTexEscapes(markdown: string): string {
  const out: string[] = [];
  let prose: string[] = [];
  let fence: string | null = null;
  const flush = () => {
    if (prose.length === 0) return;
    out.push(prose.join("\n").replace(FORMULA, (formula) => formula.replace(ESCAPE, "\\\\")));
    prose = [];
  };
  for (const line of markdown.split("\n")) {
    const marker = FENCE_LINE.exec(line);
    if (fence !== null) {
      if (
        marker &&
        marker[1][0] === fence[0] &&
        marker[1].length >= fence.length &&
        !marker[2].trim()
      ) {
        fence = null;
      }
      out.push(line);
    } else if (marker) {
      flush();
      fence = marker[1];
      out.push(line);
    } else {
      prose.push(line);
    }
  }
  flush();
  return out.join("\n");
}
