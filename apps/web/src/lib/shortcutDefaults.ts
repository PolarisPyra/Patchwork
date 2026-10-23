import type { CommandInfo, Rule } from "~/lib/shortcuts";

/** The pages of the Shortcuts settings pane, in order. */
export const SHORTCUT_PAGES = ["Main", "Chat", "Sidebar", "Archive", "Composer"] as const;
export type ShortcutPage = (typeof SHORTCUT_PAGES)[number];

/** Every command a key can run, with the page it is listed on. */
export const COMMANDS: CommandInfo[] = [
  { id: "palette.toggle", page: "Main", title: "Search", description: "Open or close search" },
  {
    id: "chat.new",
    page: "Main",
    title: "New chat or task",
    description: "A chat, or a task in the current project",
  },
  {
    id: "chat.newHere",
    page: "Main",
    title: "New task here",
    description: "In the selected chat's project or folder; a chat if none is selected",
  },
  { id: "settings.open", page: "Main", title: "Open settings" },
  {
    id: "composer.focus",
    page: "Main",
    title: "Jump to the composer",
    description: "Outside a text field",
  },
  {
    id: "sidebar.toggle",
    page: "Main",
    title: "Toggle sidebar",
    description: "Closes the sidebar, or reopens its last view; outside a text field",
  },
  { id: "palette.close", page: "Main", fixed: true, title: "Close search" },
  { id: "settings.close", page: "Main", fixed: true, title: "Close settings" },
  { id: "canvas.close", page: "Main", fixed: true, title: "Close the file panel" },
  {
    id: "transcript.selectAll",
    page: "Chat",
    fixed: true,
    title: "Select all",
    description: "Selects the text of the chat shown, once you click in it",
  },
  {
    id: "transcript.clearSelection",
    page: "Chat",
    fixed: true,
    title: "Clear highlight",
    description: "Unselects highlighted chat text",
  },
  {
    id: "sidebar.selectAll",
    page: "Sidebar",
    fixed: true,
    title: "Select all",
    description: "Every chat and task shown, once you click in the list",
  },
  {
    id: "sidebar.clearSelection",
    page: "Sidebar",
    fixed: true,
    title: "Clear selection",
    description: "Unselects the picked chats and tasks",
  },
  {
    id: "workspace.closeChat",
    page: "Sidebar",
    title: "Remove selected",
    description: "The picked chats and tasks, and the open chat while the composer is not active",
  },
  {
    id: "workspace.deleteChat",
    page: "Sidebar",
    title: "Delete",
    description:
      "The picked chats and tasks, or the open chat if none; deletes them in the archive. Outside a text field",
  },
  {
    id: "sidebar.selectAllArchived",
    page: "Archive",
    fixed: true,
    title: "Select all",
    description: "Every archived chat, once you click in the list",
  },
];

/**
 * The built-in keybindings. When several match the same key, the one lower in this list wins, so
 * the more specific rule goes later. A rule the user remaps moves after all of these.
 *
 * `when` is checked against the context where the key was pressed: `textInputFocus` and
 * `hasTextSelection` come from the page, the `...Focus` flags from `shortcutContext(...)` on a
 * region, and the rest from `useContextKey(...)`.
 */
export const DEFAULT_RULES: Rule[] = [
  { key: "mod+k", command: "palette.toggle" },
  { key: "mod+t", command: "chat.new" },
  { key: "mod+n", command: "chat.newHere" },
  { key: "mod+,", command: "settings.open" },
  { key: "tab", command: "composer.focus", when: "!textInputFocus" },
  { key: "space", command: "sidebar.toggle", when: "!textInputFocus" },
  { key: "mod+w", command: "workspace.closeChat" },
  { key: "delete", command: "workspace.deleteChat", when: "!textInputFocus" },
  { key: "mod+a", command: "transcript.selectAll", when: "transcriptFocus && !textInputFocus" },
  { key: "mod+a", command: "sidebar.selectAll", when: "sidebarTreeFocus && !textInputFocus" },
  {
    key: "mod+a",
    command: "sidebar.selectAllArchived",
    when: "archivedListFocus && !textInputFocus",
  },
  { key: "escape", command: "sidebar.clearSelection", when: "hasSelection" },
  { key: "escape", command: "canvas.close", when: "canvasOpen" },
  {
    key: "escape",
    command: "transcript.clearSelection",
    when: "transcriptFocus && hasTextSelection",
  },
  { key: "escape", command: "settings.close", when: "settingsOpen" },
  { key: "escape", command: "palette.close", when: "paletteOpen" },
];
