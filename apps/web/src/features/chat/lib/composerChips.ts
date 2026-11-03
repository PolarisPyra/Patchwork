import { type Editor, Extension } from "@tiptap/core";
import type { Node as PMNode } from "@tiptap/pm/model";
import { NodeSelection, Plugin, type Transaction } from "@tiptap/pm/state";
import { getSigils } from "~/lib/sigils";

/** Chips: inline atoms that stand for a command, a skill, an agent, a file or an image. */
const CHIPS = new Set([
  "goalCommand",
  "slashCommand",
  "skillCommand",
  "agentCommand",
  "mention",
  "inlineImage",
]);

/** Commands the agent reads only at the very start of the draft. */
const LEADING = new Set(["goalCommand", "slashCommand"]);

const commandText = (node: PMNode) =>
  node.type.name === "goalCommand" ? "/goal" : `/${node.attrs.name as string}`;

/**
 * Backspace or Delete beside a chip selects it first, so it is not lost to a stray key; the next
 * press removes it. A command chip that is no longer at the start of the draft, such as one with
 * text typed before it, turns back into the text it stands for.
 */
export const ChipKeys = Extension.create({
  name: "chipKeys",
  // Ahead of the mention's own Backspace, which would turn a file chip back into "@".
  priority: 1000,
  addKeyboardShortcuts() {
    const select = (editor: Editor, back: boolean) => {
      const { selection, doc } = editor.state;
      if (!selection.empty) return false;
      const node = back ? selection.$from.nodeBefore : selection.$from.nodeAfter;
      if (!node || !CHIPS.has(node.type.name)) return false;
      const at = back ? selection.from - node.nodeSize : selection.from;
      editor.view.dispatch(editor.state.tr.setSelection(NodeSelection.create(doc, at)));
      return true;
    };
    return {
      Backspace: ({ editor }) => select(editor, true),
      Delete: ({ editor }) => select(editor, false),
    };
  },
  addProseMirrorPlugins() {
    return [
      new Plugin({
        appendTransaction(transactions, _old, state) {
          if (!transactions.some((tr) => tr.docChanged)) return null;
          const tr = state.tr;
          state.doc.descendants((node, pos) => {
            if (!LEADING.has(node.type.name) || pos === 1) return;
            const from = tr.mapping.map(pos);
            tr.replaceWith(from, from + node.nodeSize, state.schema.text(commandText(node)));
          });
          return tr.docChanged ? tr : null;
        },
      }),
    ];
  },
});

/** What the composer knows by name, to turn typed text back into chips. */
export interface ChipCatalog {
  isCommand: (name: string) => boolean;
  isSkill: (name: string) => boolean;
  isAgent: (name: string) => boolean;
  isFile: (path: string) => boolean;
}

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\/-]/g, "\\$&");

/**
 * Turns `/command` at the start of the draft, and `$skill`, `&agent` and `@path` anywhere, into
 * chips once the composer knows the name, such as a draft written or restored before the lists
 * loaded. The word the caret is still at the end of is left alone, and the change is kept out of
 * the undo history, since the user did not make it.
 */
export function rematchChips(editor: Editor, catalog: ChipCatalog) {
  const { state } = editor;
  const sigils = getSigils();
  const kinds = [
    {
      sigil: sigils.commands,
      make: (name: string) => chip(name === "goal" ? "goalCommand" : "slashCommand", name),
      leading: true,
    },
    { sigil: sigils.skills, make: (name: string) => chip("skillCommand", name), leading: false },
    { sigil: sigils.agents, make: (name: string) => chip("agentCommand", name), leading: false },
    { sigil: sigils.files, make: (path: string) => chip("mention", path), leading: false },
  ];
  const known = [catalog.isCommand, catalog.isSkill, catalog.isAgent, catalog.isFile];
  const find = new RegExp(
    `(^|\\s)(${kinds.map((kind) => escape(kind.sigil)).join("|")})([^\\s\\ufffc]+)`,
    "g",
  );
  function chip(type: string, name: string) {
    const nodeType = state.schema.nodes[type];
    if (!nodeType) return null;
    if (type === "goalCommand") return nodeType.create();
    return type === "mention"
      ? nodeType.create({ id: name, label: name })
      : nodeType.create({ name });
  }
  const caret = state.selection.empty ? state.selection.from : -1;
  const tr: Transaction = state.tr;
  state.doc.descendants((block, blockPos) => {
    if (!block.isTextblock) return true;
    if (block.type.spec.code) return false;
    const text = block.textBetween(0, block.content.size, undefined, "￼");
    for (const match of text.matchAll(find)) {
      const kind = kinds.findIndex((k) => k.sigil === match[2]);
      const name = match[3];
      const from = blockPos + 1 + (match.index ?? 0) + match[1].length;
      const to = from + match[2].length + name.length;
      if (kind < 0 || to === caret || !known[kind](name)) continue;
      if (kinds[kind].leading && from !== 1) continue;
      const node = kinds[kind].make(name);
      if (node) tr.replaceWith(tr.mapping.map(from), tr.mapping.map(to), node);
    }
    return false;
  });
  if (tr.docChanged) editor.view.dispatch(tr.setMeta("addToHistory", false));
}
