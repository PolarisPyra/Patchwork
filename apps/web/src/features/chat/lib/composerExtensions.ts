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
import { Plugin, Selection, TextSelection } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "~/api";
import CodeBlockNodeView from "~/features/chat/components/CodeBlockNodeView";
import { codeHighlightPlugin } from "~/features/chat/lib/codeHighlight";
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

/**
 * A reference to a project file, written `@path` in the draft. The agent reads the path as
 * typed, so it serialises to the same text the mention shows.
 */
const FileMention = Mention.extend({
  renderMarkdown: (node: { attrs?: { id?: string; label?: string } }) =>
    `@${node.attrs?.label ?? node.attrs?.id ?? ""}`,
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

/**
 * Styles a line that starts with `#`, `##`, `###` or `>` as soon as the marker is typed. The text
 * stays a plain paragraph with its markers, so the line does not break and the draft keeps its Markdown.
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
) {
  return [
    // Links keep their Markdown form; clicking one in a draft does not open it. Text dropped in
    // only links URLs with a protocol, so file names such as start.sh or main.rs stay plain text.
    // Dropped files become attachments (see Chat.tsx), so the drop cursor would point at a
    // place nothing is inserted.
    // Headings and quotes stay as typed text (see MarkdownLine), so they do not become block nodes.
    StarterKit.configure({
      codeBlock: false,
      blockquote: false,
      heading: false,
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
    SkillCommand.configure({ isSkill }),
    AgentCommand.configure({ isAgent }),
    InlineImage,
    MarkdownLine,
    SelectAllText,
    ...MarkdownTextBlocks,
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
