import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import {
  ArchiveIcon,
  ArrowCounterClockwiseIcon,
  ArrowsInLineVerticalIcon,
  ExportIcon,
  FileTextIcon,
  FolderIcon,
  FolderOpenIcon,
  FolderPlusIcon,
  ImageIcon,
  MagicWandIcon,
  PencilSimpleIcon,
  PlusIcon,
  PushPinIcon,
  PushPinSlashIcon,
  StarIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import { api, type Thread } from "~/api";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import ContextMenu, { type MenuItem } from "~/components/ContextMenu";
import { useRowReorder } from "~/features/sidebar/useRowReorder";
import { useStableCallback } from "~/hooks/useStableHandlers";
import { buildTreeRows } from "~/features/sidebar/lib/treeRows";
import { getSelectedRows, setSelectedRows } from "~/features/sidebar/lib/selectionState";
import { getCollapsed, loadCollapsed, onCollapsedChange, setCollapsed } from "~/lib/collapsedState";
import { exportThread } from "~/features/sidebar/lib/exportThread";
import { shortcutContext, useCommand } from "~/lib/shortcuts";
import { AGENT_FOLDER_NAMES } from "~/lib/util";
import { InlineName, ThreadRow, type RowActions } from "~/features/sidebar/components/ThreadRow";
import type { RowDrag } from "~/features/sidebar/useRowReorder";
import {
  Collapse,
  PickRow,
  SectionLabel,
  TreeCollapse,
  TreeTwistie,
  telltalePaths,
} from "~/features/sidebar/components/tree";
import {
  clearSelectionOnBlank,
  collapseKey,
  isPlainPress,
  rowKey,
  sameTarget,
  type PickModifiers,
} from "~/features/sidebar/lib/targets";
import {
  CHATS_GROUP,
  PROJECTS_GROUP,
  TASKS_GROUP_PREFIX,
  type SidebarProps,
  type Target,
} from "~/features/sidebar/types";

/** Favorites and pinned tasks keep the sidebar's order among themselves and are not dragged: a
 * drop would move them into the list they were dropped in. */
const NO_DRAG: RowDrag = {
  attrs: { "data-drop": "", onPointerDown: () => {}, onClickCapture: () => {} },
  edge: "",
  dragging: false,
};

export default function TasksView({
  projects,
  threads,
  leavingIds,
  treeRowsRef,
  folders,
  selectedId,
  onNewTask,
  onNewChat,
  onSelectThread,
  onRenameThread,
  onSetFavorite,
  onSetPinned,
  onChatBackground,
  onRenameProject,
  onRelocateProject,
  onRemoveTargets,
  onReorderProjects,
  onReorderThreads,
  onNewFolder,
  onRenameFolder,
  onSortByAgent,
  onUndoSortByAgent,
  onNewTaskInFolder,
  onOpenFolder,
  onDropFolders,
  flashId,
  threadLogsMenu,
}: SidebarProps) {
  // Collapsed state is restored before the lists show, so nothing animates shut on launch.
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    void loadCollapsed().then(() => setLoaded(true));
  }, []);
  const [menu, setMenu] = useState<{ x: number; y: number; targets: Target[] } | null>(null);
  const [renaming, setRenaming] = useState<Target | null>(null);
  // The keyboard focus in the tree: the row the arrow keys move from.
  const cursorRef = useRef<Target | null>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  const telltales = useMemo(() => telltalePaths(projects), [projects]);
  const projectsLabelRef = useRef<HTMLDivElement>(null);
  // True while folders from the file manager are dragged over the Projects section.
  const [folderDrop, setFolderDrop] = useState(false);
  // Tauri hands dropped files over as paths (HTML drop events carry none), so drops are listened
  // for on the webview and kept only when they land on the Projects section: from its label down.
  useEffect(() => {
    function overProjects(position: { x: number; y: number }) {
      const tree = treeRef.current;
      const label = projectsLabelRef.current;
      if (!tree || !label) return false;
      // Positions are physical pixels; the rects are in CSS pixels.
      const scale = window.devicePixelRatio;
      const x = position.x / scale;
      const y = position.y / scale;
      const r = tree.getBoundingClientRect();
      const top = Math.max(r.top, label.getBoundingClientRect().top);
      return x >= r.left && x <= r.right && y >= top && y <= r.bottom;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "leave") {
          setFolderDrop(false);
        } else if (payload.type === "drop") {
          setFolderDrop(false);
          if (overProjects(payload.position)) onDropFolders(payload.paths);
        } else {
          setFolderDrop(overProjects(payload.position));
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onDropFolders]);
  // Set when a chat opens from the tree, so its composer does not take the focus from the tree;
  // false when the tree hands the focus to the chat, null when the tree has not said.
  // Only set when the selection changes, since the effect that clears it runs on that change.
  const keepTreeFocus = useRef<boolean | null>(null);
  /** Also marks the tree, so the composer leaves the focus there instead of taking it and handing
   * it back: each move restyles every row that styles on the tree's focus. */
  const holdTreeFocus = (on: boolean) => {
    keepTreeFocus.current = on;
    treeRef.current?.toggleAttribute("data-keeps-focus", on);
  };
  const setPicked = setSelectedRows;
  const reorder = useRowReorder((group, ids, folderId) =>
    group === PROJECTS_GROUP ? onReorderProjects(ids) : onReorderThreads(ids, folderId),
  );
  /** Changes which projects and folders are collapsed, and saves it. The rows animate from the
   * collapsed set itself, so this does not re-render the tree. */
  const changeCollapsed = (update: (prev: ReadonlySet<string>) => ReadonlySet<string>) => {
    setCollapsed(update(getCollapsed()));
  };
  const toggle = (id: string) =>
    changeCollapsed((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const collapseAll = () =>
    changeCollapsed((prev) => new Set([...prev, ...projects.map((p) => p.id)]));
  // A favorite is listed only under Favorites, not also in Chats or its project.
  const favorites = threads.filter((t) => t.favorite);
  const chats = threads.filter((t) => t.projectId === null && !t.favorite);
  const chatIds = chats.map((t) => t.id);

  /** Ctrl+click adds or drops a row from the picked rows and returns true. A plain click clears them and returns false, so the row acts as usual. */
  const pickRow = (e: PickModifiers, target: Target) => {
    const { rows: treeRows } = buildTree(getCollapsed());
    const cursor = cursorRef.current;
    const anchor = cursor ? treeRows.findIndex((r) => sameTarget(r, cursor)) : -1;
    const at = treeRows.findIndex((r) => sameTarget(r, target));
    cursorRef.current = target;
    // Shift+click picks every tree row from the cursor to the clicked row.
    if (e.shiftKey && anchor >= 0 && at >= 0) {
      const [from, to] = anchor < at ? [anchor, at] : [at, anchor];
      setPicked(treeRows.slice(from, to + 1));
      return true;
    }
    // A plain click selects only this row, and a chat opens there but keeps the tree's focus.
    if (!e.ctrlKey) {
      setPicked([target]);
      if (target.kind === "thread" && target.id !== selectedId) holdTreeFocus(true);
      return false;
    }
    setPicked((prev) =>
      prev.some((p) => sameTarget(p, target))
        ? prev.filter((p) => !sameTarget(p, target))
        : [...prev, target],
    );
    return true;
  };
  const openMenu = (e: MouseEvent, target: Target) => {
    e.preventDefault();
    // A right-click on a picked row acts on all picked rows; on any other row, on that row alone.
    const picked = getSelectedRows();
    const isPicked = picked.some((p) => sameTarget(p, target));
    const targets = isPicked ? picked : [target];
    if (!isPicked) setPicked([]);
    setMenu({ x: e.clientX, y: e.clientY, targets });
  };
  const menuItems = (targets: Target[]): MenuItem[] => {
    const archiving = targets.every((t) => t.kind === "thread");
    const count = targets.length > 1 ? ` ${targets.length} items` : "";
    const picked = archiving ? threads.filter((t) => targets.some((x) => x.id === t.id)) : [];
    const ids = picked.map((t) => t.id);
    // Some not yet a favorite, or not yet pinned: the item adds them all; otherwise it takes them all off.
    const favorite = picked.some((t) => !t.favorite);
    const pin = picked.some((t) => !t.pinned);
    return [
      ...(targets.length === 1
        ? [{ label: "Rename", Icon: PencilSimpleIcon, onSelect: () => setRenaming(targets[0]) }]
        : []),
      ...(picked.length > 0
        ? [
            {
              label: favorite ? "Add to favorites" : "Remove from favorites",
              Icon: StarIcon,
              onSelect: () => {
                reveal.current = { id: ids[0], moved: (t) => t.favorite === favorite };
                onSetFavorite(ids, favorite);
              },
            },
          ]
        : []),
      ...(picked.length > 0 && picked.every((t) => t.projectId !== null)
        ? [
            {
              label: pin ? "Pin" : "Unpin",
              Icon: pin ? PushPinIcon : PushPinSlashIcon,
              onSelect: () => {
                reveal.current = { id: ids[0], moved: (t) => t.pinned === pin };
                onSetPinned(ids, pin);
              },
            },
          ]
        : []),
      ...(targets.length === 1 && targets[0].kind === "thread"
        ? [
            {
              label: "Background…",
              Icon: ImageIcon,
              onSelect: () => onChatBackground(targets[0].id),
            },
            {
              label: "Export to…",
              Icon: ExportIcon,
              onSelect: () => void exportThread(targets[0].id),
            },
            ...(threadLogsMenu
              ? [
                  {
                    label: "Open logs",
                    Icon: FileTextIcon,
                    onSelect: () => void api.openThreadLogs(targets[0].id).catch(() => {}),
                  },
                ]
              : []),
          ]
        : []),
      ...(targets.length === 1 && targets[0].kind === "project"
        ? [
            {
              label: "Change folder…",
              Icon: FolderIcon,
              onSelect: () => onRelocateProject(targets[0].id),
            },
            {
              label: "Open folder",
              Icon: FolderOpenIcon,
              onSelect: () => {
                const project = projects.find((p) => p.id === targets[0].id);
                if (project) void api.revealFile(null, project.path).catch(() => {});
              },
            },
          ]
        : []),
      {
        label: archiving ? `Archive${count}` : `Remove${count}`,
        Icon: archiving ? ArchiveIcon : TrashIcon,
        danger: !archiving,
        onSelect: () => onRemoveTargets(targets),
      },
    ];
  };
  /** Saves a new name, unless it is blank or unchanged. */
  const commitRename = (target: Target, name: string, current: string) => {
    setRenaming(null);
    const trimmed = name.trim();
    if (!trimmed || trimmed === current) return;
    if (target.kind === "thread") onRenameThread(target.id, trimmed);
    else if (target.kind === "folder") onRenameFolder(target.id, trimmed);
    else onRenameProject(target.id, trimmed);
  };
  const onSelect = useStableCallback(onSelectThread);
  const onArchive = useStableCallback((thread: Thread) =>
    onRemoveTargets([{ kind: "thread", id: thread.id }]),
  );
  const onPick = useStableCallback(pickRow);
  const onContextMenu = useStableCallback(openMenu);
  const onCommitRename = useStableCallback(commitRename);
  const onCancelRename = useStableCallback(() => setRenaming(null));
  const actions = useMemo<RowActions>(
    () => ({
      onSelectThread: onSelect,
      onArchiveThread: onArchive,
      onPick,
      onContextMenu,
      onCommitRename,
      onCancelRename,
    }),
    [onSelect, onArchive, onPick, onContextMenu, onCommitRename, onCancelRename],
  );
  const minute = Math.floor(Date.now() / 60_000);
  /** What a row shows that depends on the tree's state. */
  const rowState = (t: Thread) => ({
    selected: t.id === selectedId,
    editing: renaming?.kind === "thread" && renaming.id === t.id,
    minute,
    actions,
  });
  /** New folders open for renaming, in an expanded project so they show. */
  const addFolder = async (projectId: string) => {
    const folder = await onNewFolder(projectId);
    if (!folder) return;
    changeCollapsed((prev) => {
      const next = new Set(prev);
      next.delete(projectId);
      return next;
    });
    setRenaming({ kind: "folder", id: folder.id });
  };
  /** Opens a project, and the folder a task goes into, so the new task shows in the tree. */
  const revealTask = (projectId: string, folderId?: string) =>
    changeCollapsed((prev) => {
      const next = new Set(prev);
      next.delete(projectId);
      if (folderId) next.delete(`folder:${folderId}`);
      return next;
    });

  // Ctrl+A picks the chats and tasks on screen: those in collapsed projects or folders are hidden.
  useCommand("sidebar.selectAll", () => {
    setPicked(buildTree(getCollapsed()).rows.filter((r) => r.kind === "thread"));
  });

  // Built when a key or click needs it, from the collapsed set as it is then. The favorites and
  // chats come first, as on screen, so the arrow keys and Shift+click reach them too.
  const buildTree = (hidden: ReadonlySet<string>) => {
    const tree = buildTreeRows({ projects, folders, threads, leavingIds }, hidden);
    return { ...tree, rows: [...listedAbove(), ...tree.rows] };
  };
  /** The favorites and chats, which sit above the projects. */
  function listedAbove(): Target[] {
    return [...favorites, ...chats]
      .filter((t) => !leavingIds.has(t.id))
      .map((t) => ({ kind: "thread", id: t.id }));
  }
  // The favorites and chats come first on screen, then the projects' rows, so archiving can step through them in that order.
  const syncTreeRows = () => {
    treeRowsRef.current = buildTree(getCollapsed()).rows;
  };
  useEffect(syncTreeRows);
  // A row pinned or favorited from the menu moves to the top of its list, often above the part on
  // screen, so it is scrolled to once it is there.
  const reveal = useRef<{ id: string; moved: (t: Thread) => boolean } | null>(null);
  useEffect(() => {
    const r = reveal.current;
    const thread = r && threads.find((t) => t.id === r.id);
    if (!r || !thread || !r.moved(thread)) return;
    reveal.current = null;
    treeRef.current
      ?.querySelector(`[data-thread-id="${r.id}"]`)
      ?.scrollIntoView({ block: "nearest" });
  });
  // Opening or closing a folder from outside the sidebar, such as the palette, must refresh the list too.
  useEffect(() => onCollapsedChange(syncTreeRows));

  // The selected chat is the keyboard focus until the arrow keys or a click move it.
  // Switching chats replaces the selection with the new chat, or clears it when no chat is open.
  // The open chat stays the selection until a folder or project is picked after it.
  useEffect(() => {
    cursorRef.current = selectedId ? { kind: "thread", id: selectedId } : null;
    setPicked(selectedId ? [{ kind: "thread", id: selectedId }] : []);
  }, [selectedId]);

  // Opening a chat from the tree previews it: the composer's autofocus runs after this, so the tree takes the focus back.
  // The chat that opens in place of one leaving the list keeps the tree's focus too, such as the
  // next one after Ctrl+W: its composer took the focus, so a held Ctrl+W stopped partway.
  const lastSelectedId = useRef(selectedId);
  useLayoutEffect(() => {
    const left = lastSelectedId.current;
    lastSelectedId.current = selectedId;
    const gone = left !== null && (leavingIds.has(left) || !threads.some((t) => t.id === left));
    if (gone && keepTreeFocus.current === null && document.activeElement === treeRef.current)
      holdTreeFocus(true);
  }, [selectedId]);
  useEffect(() => {
    const keep = keepTreeFocus.current;
    keepTreeFocus.current = null;
    if (!keep) return;
    setTimeout(() => {
      treeRef.current?.focus();
      treeRef.current?.removeAttribute("data-keeps-focus");
    }, 0);
  }, [selectedId]);

  /** The explorer's keys: arrows move the focus, Left and Right collapse and expand, Enter opens, F2 renames. */
  const onTreeKey = (e: KeyboardEvent<HTMLDivElement>) => {
    // Rename fields and the row buttons keep their own keys.
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLButtonElement) return;
    const keys = ["ArrowDown", "ArrowUp", "Home", "End", "ArrowLeft", "ArrowRight", "Enter", "F2"];
    if (!keys.includes(e.key)) return;
    e.preventDefault();
    const hidden = getCollapsed();
    const { rows: treeRows, parent: treeParent } = buildTree(hidden);
    const cursor = cursorRef.current;
    const at = cursor ? treeRows.findIndex((r) => sameTarget(r, cursor)) : -1;
    const current = at >= 0 ? treeRows[at] : null;
    const parent = current ? treeParent.get(rowKey(current)) : undefined;
    const expanded = !!current && current.kind !== "thread" && !hidden.has(collapseKey(current));
    // Arrows select the row they land on, and a chat opens there but keeps the tree's focus.
    const go = (t: Target | undefined) => {
      if (!t) return;
      cursorRef.current = t;
      setPicked([t]);
      if (t.kind !== "thread") return;
      if (t.id !== selectedId) holdTreeFocus(true);
      onSelectThread(t.id);
    };
    switch (e.key) {
      case "ArrowDown":
        go(treeRows[Math.min(at + 1, treeRows.length - 1)]);
        break;
      case "ArrowUp":
        go(treeRows[Math.max(at - 1, 0)]);
        break;
      case "Home":
        go(treeRows[0]);
        break;
      case "End":
        go(treeRows[treeRows.length - 1]);
        break;
      case "ArrowRight": {
        if (!current || current.kind === "thread") break;
        if (!expanded) {
          toggle(collapseKey(current));
          break;
        }
        const first = treeRows[at + 1];
        const firstParent = first ? treeParent.get(rowKey(first)) : undefined;
        if (firstParent && sameTarget(firstParent, current)) go(first);
        break;
      }
      case "ArrowLeft":
        if (current && expanded) toggle(collapseKey(current));
        else go(parent);
        break;
      case "Enter":
        if (!current) break;
        // Enter opens the chat and moves the focus into it.
        if (current.kind === "thread") {
          holdTreeFocus(false);
          onSelectThread(current.id);
          setTimeout(
            () => document.querySelector<HTMLElement>(".composer-editor .ProseMirror")?.focus(),
            0,
          );
        } else toggle(collapseKey(current));
        break;
      case "F2":
        if (current) setRenaming(current);
        break;
    }
  };

  if (!loaded) return null;

  // Rows keep their full height and the list scrolls, rather than squeezing rows to fit.
  return (
    <div
      ref={treeRef}
      role="tree"
      aria-label="Tasks"
      aria-multiselectable="true"
      className="group/tree flex min-h-0 flex-1 flex-col gap-px overflow-y-scroll overflow-x-hidden p-2 [&::-webkit-scrollbar]:w-1 [&::-webkit-scrollbar-thumb]:border-0 *:shrink-0 focus:outline-none"
      tabIndex={-1}
      {...shortcutContext("sidebarTreeFocus")}
      onKeyDown={onTreeKey}
      onClick={clearSelectionOnBlank}
    >
      {favorites.length > 0 && (
        <>
          <SectionLabel>Favorites</SectionLabel>
          {favorites.map((t) => (
            <Collapse key={t.id} open={!leavingIds.has(t.id)} overflowWhenOpen>
              <ThreadRow thread={t} {...rowState(t)} depth={0} drag={NO_DRAG} />
            </Collapse>
          ))}
        </>
      )}
      <SectionLabel
        action={
          <button className="btn-icon" title="New chat" aria-label="New chat" onClick={onNewChat}>
            <PlusIcon size={18} />
          </button>
        }
      >
        Chats
      </SectionLabel>
      {chats.length === 0 && (
        <div className="flex h-[28px] items-center px-1.5 text-[length:var(--fs-sm)] text-fg-muted">
          No chats yet
        </div>
      )}
      {chats.map((t) => (
        // Chat rows reach 4px past the list's edge (see ThreadRow), so an open one is not clipped.
        <Collapse key={t.id} open={!leavingIds.has(t.id)} overflowWhenOpen>
          <ThreadRow
            thread={t}
            {...rowState(t)}
            depth={0}
            drag={reorder.row(CHATS_GROUP, t.id, chatIds, null)}
          />
        </Collapse>
      ))}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems(menu.targets)}
          onClose={() => setMenu(null)}
        />
      )}

      <SectionLabel
        ref={projectsLabelRef}
        action={
          <div className="flex items-center gap-1">
            <button
              className="flex cursor-pointer items-center gap-1 border-0 bg-transparent p-0 normal-case tracking-normal text-fg-muted hover:text-fg"
              onClick={collapseAll}
            >
              <ArrowsInLineVerticalIcon size={15} />
              Collapse all
            </button>
            <button
              className="btn-icon"
              title="Open folder…"
              aria-label="Open folder"
              onClick={onOpenFolder}
            >
              <FolderPlusIcon size={18} />
            </button>
          </div>
        }
      >
        Projects
      </SectionLabel>
      {projects.length === 0 && !folderDrop && (
        <div className="flex h-[28px] items-center px-1.5 text-[length:var(--fs-sm)] text-fg-muted">
          No projects yet
        </div>
      )}
      <div className="-mx-2">
        {projects.map((p) => {
          const editing = renaming?.kind === "project" && renaming.id === p.id;
          const drag = reorder.row(
            PROJECTS_GROUP,
            p.id,
            projects.map((x) => x.id),
            null,
          );
          const row: Target = { kind: "project", id: p.id };
          const tasks = threads.filter((t) => t.projectId === p.id && !t.favorite);
          // Pinned tasks come first, above the folders, and go back into theirs when unpinned.
          const pinned = tasks.filter((t) => t.pinned);
          const unpinned = tasks.filter((t) => !t.pinned);
          const projectFolders = folders.filter((f) => f.projectId === p.id);
          const group = `${TASKS_GROUP_PREFIX}${p.id}`;
          const topLevel = unpinned.filter((t) => t.folderId === null);
          const topLevelIds = topLevel.map((t) => t.id);
          const taskRow = (t: Thread, folderId: string | null, ids: string[] | null) => (
            <Collapse key={t.id} open={!leavingIds.has(t.id)} overflowWhenOpen>
              <ThreadRow
                thread={t}
                {...rowState(t)}
                depth={folderId === null ? 1 : 2}
                drag={ids ? reorder.row(group, t.id, ids, folderId) : NO_DRAG}
              />
            </Collapse>
          );
          return (
            <div key={p.id}>
              <PickRow
                target={row}
                expandId={p.id}
                aria-level={1}
                aria-label={p.name}
                {...drag.attrs}
                ref={
                  flashId === p.id ? (el) => el?.scrollIntoView({ block: "nearest" }) : undefined
                }
                onPointerDown={(e) => {
                  drag.attrs.onPointerDown(e);
                  if (!editing && isPlainPress(e)) pickRow(e, row);
                }}
                className={`group mx-1 flex h-[28px] cursor-pointer items-center rounded-[var(--radius-sm)] select-none border-none pr-1.5 pl-1 text-left text-fg ${drag.edge} ${drag.dragging ? "opacity-50" : ""} ${flashId === p.id ? "project-flash" : ""}`}
                onClick={(e) => {
                  if (!pickRow(e, row)) toggle(p.id);
                }}
                onContextMenu={(e) => openMenu(e, { kind: "project", id: p.id })}
              >
                <span className="mr-1.5 inline-flex w-4 flex-none translate-x-[3px] justify-center">
                  <TreeTwistie id={p.id} />
                </span>
                {editing ? (
                  <InlineName
                    initial={p.name}
                    onCommit={(name) => commitRename({ kind: "project", id: p.id }, name, p.name)}
                    onCancel={() => setRenaming(null)}
                  />
                ) : (
                  <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap [mask-image:linear-gradient(to_right,#000_calc(100%-28px),rgb(0_0_0/0.45))] font-medium">
                    {p.name}
                    {telltales.has(p.id) && (
                      <span className="ml-1.5 font-normal text-fg-muted">
                        {telltales.get(p.id)}
                      </span>
                    )}
                  </span>
                )}
                {projectFolders.some((f) => AGENT_FOLDER_NAMES.has(f.name)) && (
                  <button
                    className="btn-icon hover-reveal h-5 w-5 flex-none"
                    title={`Undo sorting tasks in ${p.name} by provider`}
                    aria-label={`Undo sorting tasks in ${p.name} by provider`}
                    onClick={(e) => {
                      e.stopPropagation();
                      onUndoSortByAgent(p.id);
                    }}
                  >
                    <ArrowCounterClockwiseIcon size={17} />
                  </button>
                )}
                <button
                  className="btn-icon hover-reveal h-5 w-5 flex-none"
                  title={`Sort tasks in ${p.name} into folders by provider`}
                  aria-label={`Sort tasks in ${p.name} into folders by provider`}
                  onClick={(e) => {
                    e.stopPropagation();
                    onSortByAgent(p.id);
                  }}
                >
                  <MagicWandIcon size={17} />
                </button>
                <button
                  className="btn-icon hover-reveal h-5 w-5 flex-none"
                  title={`New folder in ${p.name}`}
                  aria-label={`New folder in ${p.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    void addFolder(p.id);
                  }}
                >
                  <FolderPlusIcon size={17} />
                </button>
                <button
                  className="btn-icon hover-reveal h-5 w-5 flex-none"
                  title={`New task in ${p.name}`}
                  aria-label={`New task in ${p.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    revealTask(p.id);
                    onNewTask(p.id);
                  }}
                >
                  <PlusIcon size={17} />
                </button>
              </PickRow>
              <TreeCollapse id={p.id}>
                <div className="relative flex flex-col py-px">
                  {/* Indent guide from the project's chevron down through its rows. */}
                  <span
                    aria-hidden
                    className="pointer-events-none absolute top-0 bottom-0 left-4 w-px bg-[#58585866] opacity-0 transition-opacity group-hover/tree:opacity-100"
                  />
                  {tasks.length === 0 && projectFolders.length === 0 && (
                    <div className="flex h-[28px] items-center pl-[30px] text-[length:var(--fs-sm)] text-fg-muted">
                      No tasks yet
                    </div>
                  )}
                  {pinned.map((t) => taskRow(t, null, null))}
                  {projectFolders.map((f) => {
                    const key = `folder:${f.id}`;
                    const inside = unpinned.filter((t) => t.folderId === f.id);
                    const insideIds = inside.map((t) => t.id);
                    const folderEditing = renaming?.kind === "folder" && renaming.id === f.id;
                    const folderRow: Target = { kind: "folder", id: f.id };
                    const target = reorder.folder(group, key, insideIds, f.id);
                    return (
                      <div key={f.id}>
                        <PickRow
                          target={folderRow}
                          expandId={key}
                          aria-level={2}
                          aria-label={f.name}
                          {...target.attrs}
                          onPointerDown={(e) => {
                            if (!folderEditing && isPlainPress(e)) pickRow(e, folderRow);
                          }}
                          className={`group mx-1 flex h-[28px] cursor-pointer items-center rounded-[var(--radius-sm)] select-none pr-1.5 pl-3 text-fg ${target.edge}`}
                          onClick={(e) => {
                            if (!pickRow(e, folderRow)) toggle(key);
                          }}
                          onContextMenu={(e) => openMenu(e, { kind: "folder", id: f.id })}
                        >
                          <span className="mr-1.5 inline-flex w-4 flex-none translate-x-[3px] justify-center">
                            <TreeTwistie id={key} />
                          </span>
                          {folderEditing ? (
                            <InlineName
                              initial={f.name}
                              onCommit={(name) =>
                                commitRename({ kind: "folder", id: f.id }, name, f.name)
                              }
                              onCancel={() => setRenaming(null)}
                            />
                          ) : (
                            <span className="min-w-0 flex-1 truncate text-[length:var(--fs-sm)]">
                              {f.name}
                            </span>
                          )}
                          <button
                            className="btn-icon hover-reveal h-5 w-5 flex-none"
                            aria-label={`New task in ${f.name}`}
                            onClick={(e) => {
                              e.stopPropagation();
                              revealTask(p.id, f.id);
                              onNewTaskInFolder(
                                p.id,
                                f.id,
                                inside.map((t) => t.id),
                              );
                            }}
                          >
                            <PlusIcon size={17} />
                          </button>
                        </PickRow>
                        <TreeCollapse id={key}>
                          <div className="relative py-px">
                            {/* Indent guide from the folder's chevron down through its tasks. */}
                            <span
                              aria-hidden
                              className="pointer-events-none absolute top-0 bottom-0 left-6 w-px bg-[#58585866] opacity-0 transition-opacity group-hover/tree:opacity-100"
                            />
                            {inside.map((t) => taskRow(t, f.id, insideIds))}
                          </div>
                        </TreeCollapse>
                      </div>
                    );
                  })}
                  {topLevel.map((t) => taskRow(t, null, topLevelIds))}
                </div>
              </TreeCollapse>
            </div>
          );
        })}
      </div>
      {folderDrop && (
        <div
          ref={(el) => el?.scrollIntoView({ block: "nearest" })}
          className="mx-1 flex h-[28px] items-center gap-1.5 rounded-[var(--radius-sm)] border border-dashed border-accent px-2 text-[length:var(--fs-sm)] text-fg-muted"
        >
          <FolderPlusIcon size={15} />
          Add Folder
        </div>
      )}
    </div>
  );
}
