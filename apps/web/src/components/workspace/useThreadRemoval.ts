import { useEffect, useState, type RefObject } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { api, type Folder, type Project, type Thread } from "~/api";
import type { UpdateSettings } from "~/app/App";
import type { Target } from "~/components/sidebar/Sidebar";
import { setSelectedRows } from "~/components/sidebar/selectionState";
import { syncProjects } from "~/lib/projects";
import { displayTitle } from "~/lib/util";

// How long an archived row takes to collapse out of the list. Matches the sidebar's Collapse.
const ARCHIVE_COLLAPSE_MS = 120;

interface ThreadRemoval {
  threads: Thread[];
  folders: Folder[];
  projects: Project[];
  selected: Thread | null;
  /** The sidebar's tree rows in on-screen order, written by the sidebar after each render. */
  treeRowsRef: RefObject<Target[]>;
  updateSettings: UpdateSettings;
  reload: () => Promise<unknown>;
  removeFolder: (id: string) => Promise<unknown>;
  setThreadArchived: (id: string, archived: boolean) => Promise<unknown>;
  deleteThread: (id: string) => Promise<unknown>;
  selectThread: (id: string) => void;
  /** Runs an action, showing its error if it fails. */
  attempt: (fn: () => Promise<unknown>) => void;
}

/** Removing things from the sidebar: archiving chats and tasks (their rows collapse out first),
 * removing projects and folders after asking, and deleting archived chats for good. */
export function useThreadRemoval({
  threads,
  folders,
  projects,
  selected,
  treeRowsRef,
  updateSettings,
  reload,
  removeFolder,
  setThreadArchived,
  deleteThread,
  selectThread,
  attempt,
}: ThreadRemoval) {
  // Chats and tasks being archived: their rows stay mounted, collapsing, until the archive lands.
  const [leavingIds, setLeavingIds] = useState<ReadonlySet<string>>(new Set());
  // Forgets a row once its chat is no longer live, so restoring it later opens at full height.
  useEffect(() => {
    setLeavingIds((prev) => {
      const next = new Set(
        [...prev].filter((id) => threads.some((t) => t.id === id && t.archivedAt === null)),
      );
      return next.size === prev.size ? prev : next;
    });
  }, [threads]);

  /** Asks before removing projects or folders. The native dialog is async in Tauri; window.confirm returns before the user answers. */
  const confirmRemoval = (targets: Target[]): Promise<boolean> => {
    if (targets.length > 1) {
      return confirm(`Remove ${targets.length} items? Removing a project also removes its tasks.`, {
        title: "Remove",
        kind: "warning",
      });
    }
    const [target] = targets;
    if (target.kind === "folder") {
      const name = folders.find((f) => f.id === target.id)?.name ?? "this folder";
      return confirm(`Remove ${name}? Its tasks move to the top of the project.`, {
        title: "Remove folder",
        kind: "warning",
      });
    }
    const name = projects.find((p) => p.id === target.id)?.name ?? "this project";
    return confirm(`Remove ${name} and its tasks?`, { title: "Remove project", kind: "warning" });
  };

  /** The thread row after `id` in the tree, or before it when it is last, skipping `ids`. */
  const nextThreadAfter = (id: string, ids: string[]) => {
    const rowIds = treeRowsRef.current.flatMap((r) => (r.kind === "thread" ? [r.id] : []));
    const at = rowIds.indexOf(id);
    if (at < 0) return null;
    const candidates = [...rowIds.slice(at + 1), ...rowIds.slice(0, at).reverse()];
    return candidates.find((x) => !ids.includes(x)) ?? null;
  };

  /** Collapses the chats' and tasks' rows out of the list, then archives them, so the rows below glide up instead of jumping. */
  const archiveThreads = async (ids: string[]) => {
    if (ids.length === 0) return;
    // Archiving the open task opens the next one in the tree, so Ctrl+W can run down a list.
    const next = selected && ids.includes(selected.id) ? nextThreadAfter(selected.id, ids) : null;
    // The next task is selected at once, so another Ctrl+W during the collapse acts on it.
    if (next) selectThread(next);
    setLeavingIds((prev) => new Set([...prev, ...ids]));
    try {
      await new Promise((resolve) => setTimeout(resolve, ARCHIVE_COLLAPSE_MS));
      for (const id of ids) await setThreadArchived(id, true);
    } catch (e) {
      setLeavingIds((prev) => new Set([...prev].filter((id) => !ids.includes(id))));
      throw e;
    }
  };

  /** Archives the chats and tasks among the targets, and asks once before removing projects and folders. Folders and chats inside a removed project are left out, since the project takes them too. */
  const removeTargets = async (targets: Target[]) => {
    const projectIds = targets.filter((t) => t.kind === "project").map((t) => t.id);
    const idsOf = (kind: Target["kind"]) => targets.filter((t) => t.kind === kind).map((t) => t.id);
    const folderIds = idsOf("folder").filter(
      (id) => !projectIds.includes(folders.find((f) => f.id === id)?.projectId ?? ""),
    );
    const threadIds = idsOf("thread").filter(
      (id) => !projectIds.includes(threads.find((t) => t.id === id)?.projectId ?? ""),
    );
    const removed: Target[] = [
      ...projectIds.map((id): Target => ({ kind: "project", id })),
      ...folderIds.map((id): Target => ({ kind: "folder", id })),
    ];
    if (removed.length > 0 && !(await confirmRemoval(removed))) return;
    setSelectedRows([]);
    // Projects go first: the backend deletes their tasks, so the thread list is reloaded after them.
    attempt(async () => {
      if (projectIds.length > 0) {
        for (const id of projectIds) await api.removeProject(id);
        await syncProjects(updateSettings);
        await reload();
      }
      for (const id of folderIds) await removeFolder(id);
      await archiveThreads(threadIds);
    });
  };

  /** Asks, then deletes an archived chat or task for good. */
  const deleteArchived = async (id: string) => {
    const title = threads.find((t) => t.id === id)?.title ?? "";
    const ok = await confirm(
      `Delete ${displayTitle(title)} permanently? Its messages cannot be recovered.`,
      { title: "Delete", kind: "warning" },
    );
    if (ok) attempt(() => deleteThread(id));
  };

  /** Asks once, then deletes the picked archived chats or tasks for good. */
  const deleteArchivedMany = async (ids: string[]) => {
    if (ids.length === 1) return deleteArchived(ids[0]);
    const ok = await confirm(
      `Delete ${ids.length} items permanently? Their messages cannot be recovered.`,
      { title: "Delete", kind: "warning" },
    );
    if (!ok) return;
    setSelectedRows([]);
    attempt(async () => {
      for (const id of ids) await deleteThread(id);
    });
  };

  return { leavingIds, removeTargets, deleteArchivedMany };
}
