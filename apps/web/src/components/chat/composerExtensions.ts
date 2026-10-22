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
import { Plugin } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";
import CodeBlockNodeView from "~/components/chat/CodeBlockNodeView";
import { codeHighlightPlugin } from "~/components/chat/codeHighlight";

const GOAL_CLASS =
  "rounded-[4px] bg-accent/30 px-1 py-0.5 font-[family-name:var(--font-mono)] text-fg";
const MENTION_CLASS = "rounded-[4px] bg-selected-muted px-1 text-accent-text";

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
        find: /\/goal\s$/,
        handler: ({ state, range }) => {
          if (range.from !== 1) return;
          state.tr.replaceWith(range.from, range.to, [type.create(), state.schema.text(" ")]);
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

/** The suggestions behind the `@` trigger: the project's files, filtered as the user types. */
export type MentionSuggestion = MentionOptions["suggestion"];

// The live editor keeps the extensions it was built with, so an edit here needs a full reload
// to take effect.
import.meta.hot?.accept(() => window.location.reload());

/**
 * Everything the composer edits: Markdown blocks (code, lists, task lists), links, the
 * inline command and file mentions, and the placeholder.
 */
export function composerExtensions(placeholder: string, mentionSuggestion: MentionSuggestion) {
  return [
    // Links keep their Markdown form; clicking one in a draft does not open it.
    // Dropped files become attachments (see Chat.tsx), so the drop cursor would point at a
    // place nothing is inserted.
    // Headings and quotes stay as typed text (see MarkdownLine), so they do not become block nodes.
    StarterKit.configure({
      codeBlock: false,
      blockquote: false,
      heading: false,
      dropcursor: false,
      trailingNode: false,
      link: { openOnClick: false, autolink: false },
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
    MarkdownLine,
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
