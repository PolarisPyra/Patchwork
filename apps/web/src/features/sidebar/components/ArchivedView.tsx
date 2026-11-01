import { useState, type MouseEvent, type ReactNode } from "react";
import {
  ArrowCounterClockwiseIcon,
  ArrowsInLineVerticalIcon,
  ExportIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import { AGENT_LABELS, type Thread } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import ContextMenu, { type MenuItem } from "~/components/ContextMenu";
import { setSelectedRows, useSelectedRows } from "~/features/sidebar/lib/selectionState";
import { exportThread } from "~/features/sidebar/lib/exportThread";
import { shortcutContext, useCommand } from "~/lib/shortcuts";
import { displayTitle } from "~/lib/util";
import { Collapse, Twistie } from "~/features/sidebar/components/tree";
import { clearSelectionOnBlank, sameTarget, selectionFill } from "~/features/sidebar/lib/targets";
import type { SidebarProps, Target } from "~/features/sidebar/types";

/**
 * The archive: chats and tasks moved out of the task list, under the project and folder each came
 * from, in their task-list order. Restoring one puts it back where it was.
 */

/** Chats and tasks moved out of the task list, under the project and folder each came from, in
 * their task-list order. Restoring one puts it back where it was. */
export default function ArchivedView({
  archivedThreads,
  projects,
  folders,
  onSelectThread,
  onRestoreThread,
  onDeleteArchived,
}: SidebarProps) {
  const picked = useSelectedRows();
  const setPicked = setSelectedRows;
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const openMenu = (e: MouseEvent, items: MenuItem[]) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, items });
  };
  // Closed groups here are this view's own, so closing one leaves the task list as it was.
  const [collapsed, setCollapsedGroups] = useState<ReadonlySet<string>>(new Set());
  const toggle = (key: string) =>
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  const targetOf = (id: string): Target => ({ kind: "thread", id });
  const isPicked = (id: string) => picked.some((p) => sameTarget(p, targetOf(id)));
  // Ctrl+click picks a thread for deleting together; a plain click clears the picks and opens it.
  const pick = (e: MouseEvent, id: string) => {
    if (!e.ctrlKey) {
      setPicked([]);
      onSelectThread(id);
      return;
    }
    setPicked((prev) =>
      prev.some((p) => sameTarget(p, targetOf(id)))
        ? prev.filter((p) => !sameTarget(p, targetOf(id)))
        : [...prev, targetOf(id)],
    );
  };
  const chats = archivedThreads.filter((t) => t.projectId === null);
  const groups = projects
    .map((p) => {
      const tasks = archivedThreads.filter((t) => t.projectId === p.id);
      const projectFolders = folders
        .filter((f) => f.projectId === p.id)
        .map((f) => ({ folder: f, tasks: tasks.filter((t) => t.folderId === f.id) }))
        .filter((g) => g.tasks.length > 0);
      const topLevel = tasks.filter((t) => !projectFolders.some((g) => g.folder.id === t.folderId));
      return { project: p, folders: projectFolders, topLevel };
    })
    .filter((g) => g.folders.length > 0 || g.topLevel.length > 0);
  const collapseAll = () =>
    setCollapsedGroups(
      (prev) =>
        new Set([
          ...prev,
          ...(chats.length > 0 ? ["chats"] : []),
          ...groups.map((g) => g.project.id),
        ]),
    );
  useCommand("sidebar.selectAllArchived", () =>
    setPicked(
      [
        ...chats,
        ...groups.flatMap((g) => [...g.folders.flatMap((f) => f.tasks), ...g.topLevel]),
      ].map((t) => targetOf(t.id)),
    ),
  );

  /** A project's, folder's or the chats' row, which opens and closes the rows under it.
   * Its menu deletes every archived chat or task under it. */
  const header = (key: string, name: ReactNode, folder: boolean, inside: Thread[]) => (
    <div
      role="treeitem"
      aria-level={folder ? 2 : 1}
      aria-expanded={!collapsed.has(key)}
      className={`group mx-1 flex h-[22px] cursor-pointer items-center rounded-[var(--radius-sm)] select-none pr-1.5 text-left text-fg ${folder ? "pl-3" : "pl-1"} ${selectionFill(false)}`}
      onClick={() => toggle(key)}
      onContextMenu={(e) =>
        openMenu(e, [
          {
            label: "Delete permanently",
            Icon: TrashIcon,
            danger: true,
            onSelect: () => onDeleteArchived(inside.map((t) => t.id)),
          },
        ])
      }
    >
      <span className="mr-1.5 inline-flex w-4 flex-none translate-x-[3px] justify-center">
        <Twistie open={!collapsed.has(key)} />
      </span>
      <span
        className={`min-w-0 flex-1 truncate ${folder ? "text-[length:var(--fs-sm)]" : "font-medium"}`}
      >
        {name}
      </span>
    </div>
  );
  const row = (t: Thread, nested: boolean) => (
    <div
      key={t.id}
      role="treeitem"
      aria-level={nested ? 3 : 2}
      aria-selected={isPicked(t.id)}
      aria-label={displayTitle(t.title)}
      className={`group mr-1 flex h-[22px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] select-none pr-2 pl-2 text-fg ${nested ? "ml-[38px]" : "ml-[30px]"} ${selectionFill(isPicked(t.id))}`}
      onClick={(e) => pick(e, t.id)}
      onContextMenu={(e) =>
        openMenu(e, [
          {
            label: "Restore",
            Icon: ArrowCounterClockwiseIcon,
            onSelect: () => onRestoreThread(t.id),
          },
          { label: "Export to…", Icon: ExportIcon, onSelect: () => void exportThread(t.id) },
          {
            label: "Delete permanently",
            Icon: TrashIcon,
            danger: true,
            onSelect: () => onDeleteArchived([t.id]),
          },
        ])
      }
    >
      <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap">
        {displayTitle(t.title)}
      </span>
      <button
        className="btn-icon hover-reveal h-5 w-5 flex-none hover:bg-border-strong"
        title="Restore"
        aria-label={`Restore ${displayTitle(t.title)}`}
        onClick={(e) => {
          e.stopPropagation();
          onRestoreThread(t.id);
        }}
      >
        <ArrowCounterClockwiseIcon size={16} />
      </button>
      <span
        className="inline-flex h-4 w-4 flex-none items-center justify-center"
        title={AGENT_LABELS[t.agent]}
      >
        <ProviderLogos kind={t.agent} model={t.model} size={14} />
      </span>
    </div>
  );
  /** The rows under a header, with the indent guide that shows on hover. */
  const body = (key: string, guide: string, children: ReactNode) => (
    <Collapse open={!collapsed.has(key)}>
      <div className="relative flex flex-col py-px">
        <span
          aria-hidden
          className={`pointer-events-none absolute top-0 bottom-0 ${guide} w-px bg-[#58585866] opacity-0 transition-opacity group-hover/tree:opacity-100`}
        />
        {children}
      </div>
    </Collapse>
  );

  return (
    <>
      {/* Styled as the Changes header; the list below reserves its scrollbar, so the inset matches. */}
      <div className="flex h-8 flex-none items-center justify-between pr-[18px] pl-3 text-[length:var(--fs-sm)] font-semibold">
        <span>Archived</span>
        {archivedThreads.length > 0 && (
          <button
            className="flex cursor-pointer items-center gap-1 border-0 bg-transparent p-0 font-normal text-[length:var(--fs-xs)] text-fg-muted hover:text-fg"
            onClick={collapseAll}
          >
            <ArrowsInLineVerticalIcon size={15} />
            Collapse all
          </button>
        )}
      </div>
      <div
        tabIndex={-1}
        role="tree"
        aria-label="Archived"
        aria-multiselectable="true"
        {...shortcutContext("archivedListFocus")}
        onClick={clearSelectionOnBlank}
        className="group/tree flex min-h-0 flex-1 flex-col gap-px overflow-y-scroll overflow-x-hidden p-2 *:shrink-0 focus:outline-none"
      >
        {archivedThreads.length === 0 && (
          <div className="-mt-2.5 flex h-[22px] items-center px-1 text-[length:var(--fs-sm)] text-fg-muted">
            Nothing archived
          </div>
        )}
        <div className="-mx-2">
          {chats.length > 0 && (
            <div>
              {header("chats", "Chats", false, chats)}
              {body(
                "chats",
                "left-4",
                chats.map((t) => row(t, false)),
              )}
            </div>
          )}
          {groups.map((g) => (
            <div key={g.project.id}>
              {header(g.project.id, g.project.name, false, [
                ...g.folders.flatMap((f) => f.tasks),
                ...g.topLevel,
              ])}
              {body(g.project.id, "left-4", [
                ...g.folders.map(({ folder, tasks }) => {
                  const key = `folder:${folder.id}`;
                  return (
                    <div key={folder.id}>
                      {header(key, folder.name, true, tasks)}
                      {body(
                        key,
                        "left-6",
                        tasks.map((t) => row(t, true)),
                      )}
                    </div>
                  );
                }),
                ...g.topLevel.map((t) => row(t, false)),
              ])}
            </div>
          ))}
        </div>
        {menu && (
          <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />
        )}
      </div>
    </>
  );
}
