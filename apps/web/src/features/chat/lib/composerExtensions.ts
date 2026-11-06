import { type Editor, Extension, InputRule, mergeAttributes, Node } from "@tiptap/core";
import CodeBlock from "@tiptap/extension-code-block";
import { TaskItem } from "@tiptap/extension-task-item";
import { TaskList } from "@tiptap/extension-task-list";
import Placeholder from "@tiptap/extension-placeholder";
import { Markdown } from "@tiptap/markdown";
import Mention, { type MentionOptions } from "@tiptap/extension-mention";
import { TableKit } from "@tiptap/extension-table";
import { ReactNodeViewRenderer } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import type { Node as PMNode } from "@tiptap/pm/model";
import { type EditorState, Plugin, Selection, TextSelection } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "~/api";
import CodeBlockNodeView from "~/features/chat/markdown/CodeBlockNodeView";
import { codeHighlightPlugin } from "~/features/chat/lib/codeHighlight";
import { ChipKeys } from "~/features/chat/lib/composerChips";
import { getSigils, type SigilKind } from "~/lib/sigils";

// Chips sit inside a line of text, so they keep its height: no vertical padding, the mono face a
// little smaller than the prose around it, and no line height of their own, since the mono face's
// baseline would otherwise make the line a little taller.
export const GOAL_CLASS =
  "rounded-[4px] bg-accent/30 px-1 font-[family-name:var(--font-mono)] text-[0.9em] leading-none text-fg [box-decoration-break:clone]";
export const SKILL_CLASS =
  "rounded-[4px] bg-success/25 px-1 font-[family-name:var(--font-mono)] text-[0.9em] leading-none text-fg [box-decoration-break:clone]";
export const AGENT_CLASS =
  "rounded-[4px] bg-warning/25 px-1 font-[family-name:var(--font-mono)] text-[0.9em] leading-none text-fg [box-decoration-break:clone]";
export const MENTION_CLASS = "rounded-[4px] bg-selected-muted px-1 text-accent-text";
// The image chip has no line height of its own and is lowered a quarter em, so its middle is the
// text's middle.
const IMAGE_CLASS =
  "inline-flex items-center gap-1 rounded-[4px] bg-selected-muted px-1 align-[-0.25em] leading-none text-accent-text";

/**
 * Finds `<sigil><name> ` just typed, with the sigil the user picked for `kind` (see lib/sigils),
 * and gives the handler the name and the length of the space before the sigil.
 */
function typedSigil(kind: SigilKind, name: string) {
  let cached: { sigil: string; find: RegExp } | null = null;
  return (text: string) => {
    const sigil = getSigils()[kind];
    if (cached?.sigil !== sigil) {
      const escaped = sigil.replace(/[.*+?^${}()|[\]\\/-]/g, "\\$&");
      cached = { sigil, find: new RegExp(`(^|\\s)${escaped}(${name})\\s$`) };
    }
    const match = cached.find.exec(text);
    return match
      ? { index: match.index, text: match[0], data: { name: match[2], lead: match[1].length } }
      : null;
  };
}

/** A slash command, such as `/goal`. It is an atom, so the whole command selects and deletes together. */
export const GoalCommand = Node.create({
  name: "goalCommand",
  group: "inline",
  inline: true,
  atom: true,
  selectable: false,
  parseHTML() {
    return [{ tag: "span[data-goal-command]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return [
      "span",
      mergeAttributes(HTMLAttributes, { "data-goal-command": "", class: GOAL_CLASS }),
      "/goal",
    ];
  },
  renderMarkdown: () => "/goal",
  // Typing "/goal " at the start of the draft gives the same chip as choosing it from the menu.
  // The agent only reads a goal there, so a "/goal " later in the draft stays plain text.
  addInputRules() {
    const type = this.type;
    return [
      new InputRule({
        find: typedSigil("commands", "goal"),
        handler: ({ state, range, match }) => {
          const from = range.from + (match.data?.lead ?? 0);
          if (from !== 1) return;
          state.tr.replaceWith(from, range.to, [type.create(), state.schema.text(" ")]);
        },
      }),
    ];
  },
});

/**
 * One of the agent's own slash commands, picked from the menu or typed as `/name ` at the start of
 * the draft, sent as `/name`. Like the goal, it only counts at the start, so a command with text
 * typed before it turns back into text (see composerChips).
 */
export const SlashCommand = Node.create<{ isCommand: (name: string) => boolean }>({
  name: "slashCommand",
  addOptions() {
    return { isCommand: () => false };
  },
  group: "inline",
  inline: true,
  atom: true,
  selectable: false,
  addAttributes() {
    return {
      name: {
        default: "",
        parseHTML: (element) => element.getAttribute("data-slash-command") ?? "",
        renderHTML: (attributes) => ({ "data-slash-command": attributes.name }),
      },
    };
  },
  parseHTML() {
    return [{ tag: "span[data-slash-command]" }];
  },
  renderHTML({ node, HTMLAttributes }) {
    return ["span", mergeAttributes(HTMLAttributes, { class: GOAL_CLASS }), `/${node.attrs.name}`];
  },
  renderMarkdown: (node: { attrs?: { name?: string } }) => `/${node.attrs?.name ?? ""}`,
  addInputRules() {
    const type = this.type;
    const isCommand = this.options.isCommand;
    return [
      new InputRule({
        find: typedSigil("commands", "[A-Za-z][A-Za-z0-9]*(?:[-_:.][A-Za-z0-9]+)*"),
        handler: ({ state, range, match }) => {
          const { name, lead } = match.data ?? {};
          const from = range.from + lead;
          if (from !== 1 || name === "goal" || !isCommand(name)) return null;
          state.tr.replaceWith(from, range.to, [type.create({ name }), state.schema.text(" ")]);
        },
      }),
    ];
  },
});

/**
 * A skill picked from the `$` menu, or typed as `$name ` anywhere in the draft, sent as `$name`.
 * An atom, like the goal command.
 */
export const SkillCommand = Node.create<{ isSkill: (name: string) => boolean }>({
  name: "skillCommand",
  addOptions() {
    return { isSkill: () => false };
  },
  group: "inline",
  inline: true,
  atom: true,
  selectable: false,
  addAttributes() {
    return {
      name: {
        default: "",
        parseHTML: (element) => element.getAttribute("data-skill-command") ?? "",
        renderHTML: (attributes) => ({ "data-skill-command": attributes.name }),
      },
    };
  },
  parseHTML() {
    return [{ tag: "span[data-skill-command]" }];
  },
  renderHTML({ node, HTMLAttributes }) {
    return ["span", mergeAttributes(HTMLAttributes, { class: SKILL_CLASS }), `$${node.attrs.name}`];
  },
  renderMarkdown: (node: { attrs?: { name?: string } }) => `$${node.attrs?.name ?? ""}`,
  addInputRules() {
    const type = this.type;
    const isSkill = this.options.isSkill;
    return [
      new InputRule({
        find: typedSigil("skills", "[a-z][a-z0-9]*(?:[-_:][a-z0-9]+)*"),
        handler: ({ state, range, match }) => {
          const { name, lead } = match.data ?? {};
          if (!isSkill(name)) return null;
          // The match can start with the space before the sigil, which stays.
          const from = range.from + lead;
          state.tr.replaceWith(from, range.to, [type.create({ name }), state.schema.text(" ")]);
        },
      }),
    ];
  },
});

/**
 * A custom agent picked from the `&` menu, or typed as `&name ` anywhere in the draft, sent as
 * `&name`. An atom, like a skill.
 */
export const AgentCommand = Node.create<{ isAgent: (name: string) => boolean }>({
  name: "agentCommand",
  addOptions() {
    return { isAgent: () => false };
  },
  group: "inline",
  inline: true,
  atom: true,
  selectable: false,
  addAttributes() {
    return {
      name: {
        default: "",
        parseHTML: (element) => element.getAttribute("data-agent-command") ?? "",
        renderHTML: (attributes) => ({ "data-agent-command": attributes.name }),
      },
    };
  },
  parseHTML() {
    return [{ tag: "span[data-agent-command]" }];
  },
  renderHTML({ node, HTMLAttributes }) {
    return ["span", mergeAttributes(HTMLAttributes, { class: AGENT_CLASS }), `&${node.attrs.name}`];
  },
  renderMarkdown: (node: { attrs?: { name?: string } }) => `&${node.attrs?.name ?? ""}`,
  addInputRules() {
    const type = this.type;
    const isAgent = this.options.isAgent;
    return [
      new InputRule({
        find: typedSigil("agents", "[A-Za-z][A-Za-z0-9]*(?:[-_][A-Za-z0-9]+)*"),
        handler: ({ state, range, match }) => {
          const { name, lead } = match.data ?? {};
          if (!isAgent(name)) return null;
          // The match can start with the space before the sigil, which stays.
          const from = range.from + lead;
          state.tr.replaceWith(from, range.to, [type.create({ name }), state.schema.text(" ")]);
        },
      }),
    ];
  },
});

/** A file mention as Markdown: `@path`, or `@<path>` when the path has spaces, so it stays one word. */
export function mentionMarkdown(path: string) {
  return /\s/.test(path) ? `@<${path}>` : `@${path}`;
}

/**
 * A reference to a project file, written `@path` in the draft. The agent reads the path as
 * typed, so it serialises to the same text the mention shows.
 */
const FileMention = Mention.extend({
  renderMarkdown: (node: { attrs?: { id?: string; label?: string } }) =>
    mentionMarkdown(node.attrs?.label ?? node.attrs?.id ?? ""),
});

/** An image written into a message: Markdown of its path, in angle brackets so spaces stay in it. */
export function imageMarkdown(name: string, path: string) {
  return `![${name.replace(/[\\[\]]/g, "\\$&")}](<${path}>)`;
}

const IMAGE_MARKDOWN = /!\[(?:\\.|[^\]\\])*\]\(<(\/[^<>\n]+)>\)/g;

/** The paths of the images written into a message with `imageMarkdown`. */
export function inlineImagePaths(text: string): string[] {
  return Array.from(text.matchAll(IMAGE_MARKDOWN), (m) => m[1]);
}

/** A message without the images written into it, which show above it with its attachments. */
export function withoutImageMarkdown(text: string): string {
  return text.replace(IMAGE_MARKDOWN, "").replace(/ {2,}/g, " ").trim();
}

/**
 * An image dropped, pasted or picked into the draft, shown where it went as a thumbnail and its
 * name. It is written as a Markdown image of its path, and goes with the message as an image.
 */
const InlineImage = Node.create({
  name: "inlineImage",
  group: "inline",
  inline: true,
  atom: true,
  addAttributes() {
    return { id: { default: "" }, label: { default: "" } };
  },
  parseHTML() {
    return [
      {
        tag: "span[data-inline-image]",
        getAttrs: (el) => ({ id: el.getAttribute("data-inline-image"), label: el.textContent }),
      },
    ];
  },
  renderHTML({ node }) {
    return ["span", { "data-inline-image": node.attrs.id, class: IMAGE_CLASS }, node.attrs.label];
  },
  renderText: ({ node }) => node.attrs.label,
  // The thumbnail loads once the backend lets the webview read the file.
  addNodeView() {
    return ({ node }) => {
      const dom = document.createElement("span");
      dom.className = IMAGE_CLASS;
      dom.title = node.attrs.id;
      const img = document.createElement("img");
      img.alt = "";
      img.className = "h-4 w-4 flex-none rounded-[2px] object-cover";
      dom.append(img, node.attrs.label);
      api.allowPreview(node.attrs.id).then(
        () => (img.src = convertFileSrc(node.attrs.id)),
        () => img.remove(),
      );
      return { dom };
    };
  },
  // Only images of local files are attachments; any other Markdown image stays as typed.
  markdownTokenName: "image",
  parseMarkdown: (token, helpers) =>
    token.href?.startsWith("/")
      ? helpers.createNode("inlineImage", { id: token.href, label: token.text ?? "" })
      : helpers.createTextNode(token.raw ?? ""),
  renderMarkdown: (node: { attrs?: { id?: string; label?: string } }) =>
    imageMarkdown(node.attrs?.label ?? "", node.attrs?.id ?? ""),
});

// Inline Markdown spans: group 1 is the opening marker, which the span also closes with. Code
// comes first, since nothing inside a code span is formatting.
const INLINE_SPANS: { find: RegExp; className: string }[] = [
  { find: /(`)[^`\n]+`/g, className: "composer-md-code" },
  { find: /(\*\*|__)(?=\S).+?(?<=\S)\1/g, className: "composer-md-bold" },
  {
    find: /(?<![*\w])([*_])(?=[^\s*_]).*?(?<=[^\s*_])\1(?![*\w])/g,
    className: "composer-md-italic",
  },
  { find: /(~~)(?=\S).+?(?<=\S)~~/g, className: "composer-md-strike" },
];

/** Styles the bold, italic, strikethrough and code spans in a paragraph, markers kept and faint. */
function inlineSpanDecorations(paragraph: PMNode, pos: number): Decoration[] {
  // One character per inline atom, so an offset in the text is an offset in the paragraph.
  const text = paragraph.textBetween(0, paragraph.content.size, undefined, "￼");
  const decorations: Decoration[] = [];
  const code: [number, number][] = [];
  for (const { find, className } of INLINE_SPANS) {
    for (const match of text.matchAll(find)) {
      const start = match.index;
      const end = start + match[0].length;
      if (code.some(([a, b]) => start < b && end > a)) continue;
      if (className === "composer-md-code") code.push([start, end]);
      const marker = match[1].length;
      const at = pos + 1;
      decorations.push(
        Decoration.inline(at + start, at + start + marker, { class: "composer-md-marker" }),
        Decoration.inline(at + start + marker, at + end - marker, { class: className }),
        Decoration.inline(at + end - marker, at + end, { class: "composer-md-marker" }),
      );
    }
  }
  return decorations;
}

/**
 * Styles Markdown as it is typed, keeping every marker: a line that starts with `#`, `##`, `###`
 * or `>`, and `**bold**`, `*italic*`, `~~strikethrough~~` and `` `code` `` spans. The text stays a
 * plain paragraph with its markers, so the line does not break and the draft is sent as typed.
 */
const MarkdownLine = Extension.create({
  name: "markdownLine",
  addProseMirrorPlugins() {
    return [
      new Plugin({
        props: {
          decorations(state) {
            const decorations: Decoration[] = [];
            state.doc.descendants((node, pos) => {
              if (node.type.name !== "paragraph") return true;
              const heading = /^(#{1,3})(?=\s|$)/.exec(node.textContent);
              const quote = /^>\s?/.exec(node.textContent);
              const className = heading
                ? `composer-heading-${heading[1].length}`
                : quote
                  ? "composer-quote"
                  : null;
              if (className) {
                decorations.push(Decoration.node(pos, pos + node.nodeSize, { class: className }));
              }
              // Hide the `>` marker on screen; the text keeps it so the draft stays Markdown.
              if (quote) {
                decorations.push(
                  Decoration.inline(pos + 1, pos + 1 + quote[0].length, {
                    class: "composer-quote-marker",
                  }),
                );
              }
              decorations.push(...inlineSpanDecorations(node, pos));
              return false;
            });
            return DecorationSet.create(state.doc, decorations);
          },
        },
      }),
    ];
  },
});

// These blocks deliberately stay plain paragraphs in the composer. Register their Markdown
// tokens too, so recalling a heading or quote does not try to create a disabled schema node.
const MarkdownTextBlocks = ["heading", "blockquote"].map((tokenName) =>
  Extension.create({
    name: `composerText${tokenName}`,
    markdownTokenName: tokenName,
    parseMarkdown: (token, helpers) =>
      (token.raw ?? token.text ?? "")
        .replace(/\n+$/, "")
        .split("\n")
        .map((line) =>
          helpers.createNode("paragraph", undefined, line ? [helpers.createTextNode(line)] : []),
        ),
  }),
);

// Bold, italic, strikethrough and code also stay as typed, markers and all, so a recalled draft
// shows what was written. Text inside them is still read, so a link or image in one keeps working.
const MarkdownTextSpans = ["strong", "em", "del", "codespan"].map((tokenName) =>
  Extension.create({
    name: `composerText${tokenName}`,
    markdownTokenName: tokenName,
    parseMarkdown: (token, helpers) => {
      const raw = token.raw ?? "";
      if (tokenName === "codespan" || !token.tokens) return helpers.createTextNode(raw);
      const length =
        tokenName === "strong" ? 2 : tokenName === "em" ? 1 : (/^~+/.exec(raw)?.[0].length ?? 0);
      return [
        helpers.createTextNode(raw.slice(0, length)),
        ...helpers.parseInline(token.tokens),
        helpers.createTextNode(raw.slice(raw.length - length)),
      ];
    },
  }),
);

/** A Markdown marker the format bar and its shortcuts write around the selection. */
export type InlineMarker = "**" | "*" | "~~" | "`";

/**
 * Whether the selection sits between a pair of `marker`. Inside `***`, it is both bold and italic;
 * inside `**`, it is bold and not italic.
 */
export function hasMarker(state: EditorState, marker: InlineMarker): boolean {
  const { from, to, $from, $to } = state.selection;
  const before = state.doc.textBetween(Math.max($from.start(), from - 3), from, undefined, "￼");
  const after = state.doc.textBetween(to, Math.min($to.end(), to + 3), undefined, "￼");
  const run = (text: string) =>
    text.length - text.replace(new RegExp(`^\\${marker[0]}+`), "").length;
  const pair = Math.min(run([...before].reverse().join("")), run(after));
  return marker === "*" ? pair % 2 === 1 : pair >= marker.length;
}

/**
 * Writes `marker` around the selection, or takes it away when the selection already has it. The
 * draft is the Markdown that is sent, so formatting is the text it is written with.
 */
export function toggleMarker(editor: Editor, marker: InlineMarker) {
  const { state } = editor;
  if (state.selection.$from.parent.type.spec.code) return;
  const { from, to } = state.selection;
  const n = marker.length;
  const on = hasMarker(state, marker);
  editor
    .chain()
    .focus()
    .command(({ tr }) => {
      if (on) {
        tr.delete(to, to + n).delete(from - n, from);
      } else {
        tr.insertText(marker, to).insertText(marker, from);
        tr.setSelection(TextSelection.create(tr.doc, from + n, to + n));
      }
      return true;
    })
    .run();
}

/** The usual shortcuts for bold, italic, strikethrough and code, writing their markers. */
const MarkerShortcuts = Extension.create({
  name: "markerShortcuts",
  addKeyboardShortcuts() {
    const toggle = (marker: InlineMarker) => () => {
      toggleMarker(this.editor, marker);
      return true;
    };
    return {
      "Mod-b": toggle("**"),
      "Mod-i": toggle("*"),
      "Mod-Shift-s": toggle("~~"),
      "Mod-e": toggle("`"),
    };
  },
});

/**
 * Ctrl+A selects the draft as text from its first character to its last. WebKitGTK does not paint
 * ProseMirror's select-all selection when the draft starts with a code block, so the draft looked
 * unselected.
 */
const SelectAllText = Extension.create({
  name: "selectAllText",
  addKeyboardShortcuts() {
    return {
      "Mod-a": ({ editor }) => {
        const { doc, tr } = editor.state;
        const from = Selection.atStart(doc).from;
        const to = Selection.atEnd(doc).to;
        editor.view.dispatch(tr.setSelection(TextSelection.create(doc, from, to)));
        return true;
      },
    };
  },
});

/** The suggestions behind the `@` trigger: the project's files, filtered as the user types. */
export type MentionSuggestion = MentionOptions["suggestion"];

// The live editor keeps the extensions it was built with, so an edit here needs a full reload
// to take effect.
import.meta.hot?.accept(() => window.location.reload());

/**
 * Everything the composer edits: Markdown blocks (code, lists, task lists), links, the
 * inline command and file mentions, and the placeholder.
 */
export function composerExtensions(
  placeholder: string,
  mentionSuggestion: MentionSuggestion,
  isSkill: (name: string) => boolean,
  isAgent: (name: string) => boolean,
  isCommand: (name: string) => boolean,
) {
  return [
    // Links keep their Markdown form; clicking one in a draft does not open it. Text dropped in
    // only links URLs with a protocol, so file names such as start.sh or main.rs stay plain text.
    // Dropped files become attachments (see Chat.tsx), so the drop cursor would point at a
    // place nothing is inserted.
    // Headings, quotes, bold, italic, strikethrough and inline code stay as typed text (see
    // MarkdownLine), so they do not become nodes or marks that hide their markers.
    StarterKit.configure({
      codeBlock: false,
      blockquote: false,
      heading: false,
      bold: false,
      italic: false,
      strike: false,
      code: false,
      dropcursor: false,
      trailingNode: false,
      link: {
        openOnClick: false,
        autolink: false,
        shouldAutoLink: (url) => /^[a-z][a-z0-9+.-]*:\/\//i.test(url),
      },
    }),
    // Code blocks show their language in a header that can be changed.
    CodeBlock.extend({
      addNodeView() {
        return ReactNodeViewRenderer(CodeBlockNodeView);
      },
      // Keeps the stock plugin (it handles pasting from VS Code) and adds the highlighter.
      addProseMirrorPlugins() {
        return [...(this.parent?.() ?? []), codeHighlightPlugin()];
      },
    }),
    TaskList,
    TaskItem.configure({ nested: true }),
    TableKit,
    Placeholder.configure({ placeholder }),
    Markdown,
    GoalCommand,
    SlashCommand.configure({ isCommand }),
    ChipKeys,
    SkillCommand.configure({ isSkill }),
    AgentCommand.configure({ isAgent }),
    InlineImage,
    MarkdownLine,
    SelectAllText,
    MarkerShortcuts,
    ...MarkdownTextBlocks,
    ...MarkdownTextSpans,
    FileMention.configure({
      HTMLAttributes: { class: MENTION_CLASS },
      renderText: ({ node }) => `@${node.attrs.label ?? node.attrs.id}`,
      suggestion: mentionSuggestion,
    }),
  ];
}

/**
 * Shift+Enter inside a list starts the next item, or leaves the list when the item is empty.
 * Returns false elsewhere, so Shift+Enter keeps its hard break.
 */
export function continueList(editor: Editor): boolean {
  const { $from } = editor.state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    const name = $from.node(depth).type.name;
    if (name !== "listItem" && name !== "taskItem") continue;
    if ($from.parent.textContent === "") return editor.commands.liftListItem(name);
    return editor.commands.splitListItem(
      name,
      name === "taskItem" ? { checked: false } : undefined,
    );
  }
  return false;
}

/**
 * A draft is the Markdown the user writes, so its plain text is sent exactly as typed. The
 * serializer escapes `*`, `_`, `<` and similar for rich text, which would send backslashes
 * and entities the user never wrote. Marks such as bold and code still render as Markdown.
 * This overrides a method of @tiptap/markdown's manager; re-check it when that package updates.
 */
export function keepDraftText(manager: object) {
  // The declaration marks this method private; it is public at runtime.
  (manager as { encodeTextForMarkdown: (text: string) => string }).encodeTextForMarkdown = (text) =>
    text;
}
