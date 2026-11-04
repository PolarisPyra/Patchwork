import type { Folder, Project, Thread } from "~/api";
import { collapseKey, rowKey } from "~/features/sidebar/lib/targets";
import type { Target } from "~/features/sidebar/types";

/**
 * The task tree's rows in on-screen order, each with the row it sits under, for the arrow keys and
 * Shift+click. Pure: it is built from the projects, folders and threads as they are, and the set of
 * rows that are closed, so the order can be read — and tested — without the tree around it.
 */

export interface TreeSource {
  projects: Project[];
  folders: Folder[];
  threads: Thread[];
  /** Chats and tasks on their way to the archive, drawn in the tree but no longer a row of it. */
  leavingIds: ReadonlySet<string>;
}

export interface TreeRows {
  rows: Target[];
  /** Each row's parent, by `rowKey`. A project has none; a task in a folder has its folder. */
  parent: Map<string, Target>;
}

/** A project's rows: the project, its pinned tasks, then each folder with its tasks, then the tasks
 * outside them. Favorites are listed above the projects, so they are not rows of one. */
export function buildTreeRows(
  { projects, folders, threads, leavingIds }: TreeSource,
  hidden: ReadonlySet<string>,
): TreeRows {
  const rows: Target[] = [];
  const parent = new Map<string, Target>();
  const addRow = (row: Target, of?: Target) => {
    rows.push(row);
    if (of) parent.set(rowKey(row), of);
  };
  for (const p of projects) {
    const projectRow: Target = { kind: "project", id: p.id };
    addRow(projectRow);
    if (hidden.has(p.id)) continue;
    const tasks = threads.filter(
      (x) => x.projectId === p.id && !x.favorite && !leavingIds.has(x.id),
    );
    for (const t of tasks.filter((x) => x.pinned)) addRow({ kind: "thread", id: t.id }, projectRow);
    for (const f of folders.filter((x) => x.projectId === p.id)) {
      const folderRow: Target = { kind: "folder", id: f.id };
      addRow(folderRow, projectRow);
      if (hidden.has(collapseKey(folderRow))) continue;
      for (const t of tasks.filter((x) => !x.pinned && x.folderId === f.id))
        addRow({ kind: "thread", id: t.id }, folderRow);
    }
    for (const t of tasks.filter((x) => !x.pinned && x.folderId === null))
      addRow({ kind: "thread", id: t.id }, projectRow);
  }
  return { rows, parent };
}
