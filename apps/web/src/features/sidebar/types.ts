import type { AgentKind, FileChange, Folder, Project, Thread } from "~/api";

/**
 * The sidebar's own vocabulary: which view it shows, what a row is, and what it is given. The
 * views themselves live beside this file, one each.
 */

export type SidebarView = "tasks" | "changes" | "archived";

/** A row a context menu acts on: a project, a folder, or a task or chat. */
export interface Target {
  kind: "project" | "folder" | "thread";
  id: string;
}

/** Drag groups: rows reorder only within their own group. */
export const CHATS_GROUP = "chats";
export const PROJECTS_GROUP = "projects";
export const TASKS_GROUP_PREFIX = "tasks:";

export interface SidebarProps {
  view: SidebarView;
  /** The agents switched on in Settings → Providers; the usage footer lists only these. */
  connectedAgents: AgentKind[];
  /** Project of the current context (null: a chat without a project). */
  project: Project | null;
  projects: Project[];
  /** Every thread; chats without a project and each project's tasks are grouped here. */
  threads: Thread[];
  /** Chats and tasks collapsing out of the list, on their way to the archive. */
  leavingIds: ReadonlySet<string>;
  /** The tree's rows in on-screen order, kept current so archiving the open task can move on to the next one. */
  treeRowsRef: { current: Target[] };
  selectedId: string | null;
  changes: FileChange[];
  /** Starts a new task in the given project. */
  onNewTask: (projectId: string) => void;
  /** Starts a chat without a project. */
  onNewChat: () => void;
  onSelectThread: (id: string) => void;
  onRenameThread: (id: string, title: string) => void;
  /** Lists chats and tasks under Favorites at the top of the tree, or back in their usual spot. */
  onSetFavorite: (ids: string[], favorite: boolean) => void;
  /** Lists tasks first in their project, above its folders. */
  onSetPinned: (ids: string[], pinned: boolean) => void;
  /** Opens the picker of a chat's own background. */
  onChatBackground: (id: string) => void;
  onRenameProject: (id: string, name: string) => void;
  /** Picks another folder for the project. */
  onRelocateProject: (id: string) => void;
  /** Project ids in their new order. */
  onReorderProjects: (ids: string[]) => void;
  /** Thread ids of one list (chats, a project's top level, or a folder) in their new order. */
  onReorderThreads: (ids: string[], folderId: string | null) => void;
  folders: Folder[];
  /** Creates a folder in a project; null if that failed. */
  onNewFolder: (projectId: string) => Promise<Folder | null>;
  onRenameFolder: (id: string, name: string) => void;
  /** Moves a project's tasks into one folder per agent, named for it. */
  onSortByAgent: (projectId: string) => void;
  /** Takes a project's tasks back out of the folders named for agents. */
  onUndoSortByAgent: (projectId: string) => void;
  /** Archives chats and tasks; asks, then removes projects and folders. */
  onRemoveTargets: (targets: Target[]) => void;
  /** Chats and tasks in the archive, in sidebar order. */
  archivedThreads: Thread[];
  onRestoreThread: (id: string) => void;
  /** Asks once, then deletes archived chats or tasks for good. */
  onDeleteArchived: (ids: string[]) => void;
  /** Starts a task at the top of a folder; `threadIds` are the folder's tasks in order. */
  onNewTaskInFolder: (projectId: string, folderId: string, threadIds: string[]) => void;
  onOpenFolder: () => void;
  /** Adds folders dropped on the Projects section as projects. */
  onDropFolders: (paths: string[]) => void;
  /** A project whose folder was added again; its row is scrolled to and flashes. */
  flashId: string | null;
  onRefreshChanges: () => void;
  onOpenChange: (path: string) => void;
  /** Files of the reply the Changes view was opened from, if any. */
  changeFocus: string[] | null;
  /** Shows all changes again, not just the reply's. */
  onClearChangeFocus: () => void;
  /** Opens the command palette. */
  onOpenPalette: () => void;
  /** Chat menus have Open logs (Settings → Logging). */
  threadLogsMenu: boolean;
}
