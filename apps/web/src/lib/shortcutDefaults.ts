import type { CommandInfo, Rule } from "~/lib/shortcuts";

/** Every command a key can run. The Shortcuts settings pane lists these, grouped by category. */
export const COMMANDS: CommandInfo[] = [
  { id: "palette.toggle", title: "Open or close search", category: "General" },
  { id: "chat.new", title: "New chat, or task in the current project", category: "General" },
  {
    id: "chat.newHere",
    title: "New chat, or task in the selected chat's project or folder",
    category: "General",
  },
  { id: "settings.open", title: "Open settings", category: "General" },
  { id: "composer.focus", title: "Jump to the composer", category: "General" },
  { id: "sidePanel.toggle", title: "Open or close the side panel", category: "General" },
  { id: "palette.close", title: "Close search", category: "Dismiss" },
  { id: "settings.close", title: "Close settings", category: "Dismiss" },
  { id: "canvas.close", title: "Close the file panel", category: "Dismiss" },
  { id: "sidebar.clearPicked", title: "Clear the selected chats and tasks", category: "Dismiss" },
  { id: "transcript.clearSelection", title: "Clear highlighted chat text", category: "Dismiss" },
  { id: "transcript.selectAll", title: "Select all text in the chat", category: "Chat" },
  { id: "sidebar.selectAll", title: "Select every chat and task shown", category: "Sidebar" },
  { id: "sidebar.selectAllArchived", title: "Select every archived chat", category: "Sidebar" },
  {
    id: "workspace.closeChat",
    title: "Remove selected chats and tasks, or the open one",
    category: "Sidebar",
  },
  {
    id: "workspace.deleteChat",
    title: "Remove selected chats and tasks, or the open one outside a text field",
    category: "Sidebar",
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
  { key: "space", command: "sidePanel.toggle", when: "!textInputFocus" },
  { key: "mod+w", command: "workspace.closeChat" },
  { key: "delete", command: "workspace.deleteChat", when: "!textInputFocus" },
  { key: "mod+a", command: "transcript.selectAll", when: "transcriptFocus && !textInputFocus" },
  { key: "mod+a", command: "sidebar.selectAll", when: "sidebarTreeFocus && !textInputFocus" },
  {
    key: "mod+a",
    command: "sidebar.selectAllArchived",
    when: "archivedListFocus && !textInputFocus",
  },
  { key: "escape", command: "sidebar.clearPicked", when: "hasPickedRows" },
  { key: "escape", command: "canvas.close", when: "canvasOpen" },
  {
    key: "escape",
    command: "transcript.clearSelection",
    when: "transcriptFocus && hasTextSelection",
  },
  { key: "escape", command: "settings.close", when: "settingsOpen" },
  { key: "escape", command: "palette.close", when: "paletteOpen" },
];
