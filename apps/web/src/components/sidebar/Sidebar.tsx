import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type HTMLAttributes,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
  type ReactNode,
  type Ref,
} from "react";
import {
  ArchiveIcon,
  ArrowCounterClockwiseIcon,
  ArrowLeftIcon,
  ArrowsClockwiseIcon,
  ArrowsInLineVerticalIcon,
  ChatCircleIcon,
  CircleNotchIcon,
  FolderIcon,
  FolderPlusIcon,
  GitDiffIcon,
  MagicWandIcon,
  MagnifyingGlassIcon,
  PencilSimpleIcon,
  PlusIcon,
  ShieldWarningIcon,
  TrashIcon,
} from "@phosphor-icons/react";
import {
  AGENT_LABELS,
  api,
  type AgentKind,
  type FileChange,
  type Folder,
  type Project,
  type Thread,
} from "~/api";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { confirm } from "@tauri-apps/plugin-dialog";
import ProviderLogos from "~/components/ProviderLogos";
import ContextMenu, { type MenuItem } from "~/components/sidebar/ContextMenu";
import UsagePopover from "~/components/usage/UsagePopover";
import { useRowReorder, type RowDrag } from "~/hooks/useRowReorder";
import {
  getSelectedRows,
  setSelectedRows,
  useIsRowSelected,
  useSelectedRows,
} from "./selectionState";
import {
  getCollapsed,
  loadCollapsed,
  onCollapsedChange,
  setCollapsed,
  useIsCollapsed,
} from "./collapsedState";
import { isWorking } from "~/hooks/useThreads";
import { formatKey, keyFor, shortcutContext, useCommand, useKeybindings } from "~/lib/shortcuts";
import {
  AGENT_FOLDER_NAMES,
  baseName,
  dirName,
  displayTitle,
  errorText,
  formatRelative,
} from "~/lib/util";

export type SidebarView = "tasks" | "changes" | "archived";

/** Drag groups: rows reorder only within their own group. */
const CHATS_GROUP = "chats";
const PROJECTS_GROUP = "projects";
const TASKS_GROUP_PREFIX = "tasks:";

interface SidebarProps {
  view: SidebarView;
  /** Connected agents, whose usage the footer shows. */
  agents: AgentKind[];
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
  /** Asks, then deletes an archived chat or task for good. */
  onDeleteArchived: (id: string) => void;
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
}

/** A row a context menu acts on: a project, a folder, or a task or chat. */
export interface Target {
  kind: "project" | "folder" | "thread";
  id: string;
}

const sameTarget = (a: Target, b: Target) => a.kind === b.kind && a.id === b.id;
/** The modifier keys a row click was made with. */
type PickModifiers = Pick<MouseEvent, "ctrlKey" | "shiftKey">;
/**
 * A plain left press on a row, not on one of its buttons. It selects the row at once, as VS Code's list does,
 * so a click that the browser swallows as a drag still selects.
 */
const isPlainPress = (e: PointerEvent<HTMLElement>) =>
  e.button === 0 &&
  !e.ctrlKey &&
  !e.shiftKey &&
  !(e.target instanceof Element && e.target.closest("button"));
const rowKey = (t: Target) => `${t.kind}:${t.id}`;
/** A click on the list's own blank space, not on a row or its label, clears the selection. */
const clearSelectionOnBlank = (e: MouseEvent<HTMLElement>) => {
  if (e.target === e.currentTarget) setSelectedRows([]);
};
/** The collapsed-set key for a project or folder row. */
const collapseKey = (t: Target) => (t.kind === "folder" ? `folder:${t.id}` : t.id);
/** A tree row's fill: the selection colors while the tree is focused, a hover tint otherwise. */
const selectionFill = (selected: boolean) =>
  selected
    ? "bg-[var(--tree-inactive-selected,var(--bg-selected-muted))] hover:bg-[var(--tree-selected,var(--bg-selected))] group-focus-within/tree:bg-[var(--tree-selected,var(--bg-selected))] outline-transparent group-focus-within/tree:outline group-focus-within/tree:outline-1 group-focus-within/tree:-outline-offset-1 group-focus-within/tree:outline-[#3994bcb3]"
    : "bg-transparent hover:bg-[var(--tree-hover,var(--bg-hover))]";

export default memo(function Sidebar(props: SidebarProps) {
  const { rules } = useKeybindings();
  const searchKey = keyFor(rules, "palette.toggle");
  return (
    <aside
      className="flex min-h-0 flex-none flex-col border-r border-border bg-panel text-[length:var(--fs-md)] [--fs-xs:13px] [--fs-sm:14px] [--fs-md:15px]"
      style={{ width: 256, flex: "0 0 256px" }}
    >
      <button
        className="mx-2 mt-3 mb-0.5 flex h-8 flex-none cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border border-border bg-input px-2 text-[length:var(--fs-sm)] text-fg-muted hover:border-border-strong hover:text-fg"
        onClick={props.onOpenPalette}
      >
        <MagnifyingGlassIcon size={18} />
        Search…
        {searchKey && <kbd className="kbd ml-auto">{formatKey(searchKey).join(" ")}</kbd>}
      </button>
      {props.view === "tasks" && <TasksView {...props} />}
      {props.view === "changes" && <ChangesView {...props} />}
      {props.view === "archived" && <ArchivedView {...props} />}
      <UsagePopover agents={props.agents} />
    </aside>
  );
});

/** A function with one identity for the life of the component that always calls the latest `fn`. */
function useStable<A extends unknown[], R>(fn: (...args: A) => R): (...args: A) => R {
  const latest = useRef(fn);
  latest.current = fn;
  return useCallback((...args: A) => latest.current(...args), []);
}

/** Animates its children open and closed. Stays mounted so the height can transition. */
function Collapse({
  open,
  children,
  overflowWhenOpen = false,
}: {
  open: boolean;
  children: ReactNode;
  /** Lets an open row's drop line reach past the row's edge. Closing still clips. */
  overflowWhenOpen?: boolean;
}) {
  const clip = open && overflowWhenOpen ? "overflow-visible" : "overflow-hidden";
  return (
    <div
      className={`grid grid-cols-[minmax(0,1fr)] transition-[grid-template-rows] duration-150 ease-[cubic-bezier(0.2,0,0,1)] ${open ? "grid-rows-[1fr]" : "grid-rows-[0fr]"}`}
      inert={!open}
    >
      <div className={`min-h-0 ${clip}`}>{children}</div>
    </div>
  );
}

/** VS Code's tree twistie: the chevron-down codicon, turned to point right while a row is collapsed. */
function Twistie({ open }: { open: boolean }) {
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 16 16"
      aria-hidden
      className={`transition-transform duration-150 ease-[cubic-bezier(0.2,0,0,1)] will-change-transform ${open ? "" : "-rotate-90"}`}
    >
      <path
        fill="currentColor"
        transform="translate(3 5.25) scale(0.0533333)"
        d="M3.0 16.0 87.0 100.0Q90.0 103.0 94.0 103.0Q98.0 103.0 101.0 100.0L185.0 16.0Q188.0 13.0 188.0 9.0Q188.0 5.0 185.0 2.5Q182.0 0.0 178.0 0.0Q174.0 0.0 172.0 3.0L94.0 80.0L16.0 3.0Q14.0 0.0 10.0 0.0Q6.0 0.0 3.0 2.5Q0.0 5.0 0.0 9.0Q0.0 13.0 3.0 16.0Z"
      />
    </svg>
  );
}

/** The twistie and the collapsible region of a project or folder follow the collapsed set themselves,
 * so opening or closing one re-renders these two and not every row in the tree. */
function TreeTwistie({ id }: { id: string }) {
  const collapsed = useIsCollapsed(id);
  return <Twistie open={!collapsed} />;
}

function TreeCollapse({ id, children }: { id: string; children: ReactNode }) {
  const collapsed = useIsCollapsed(id);
  return <Collapse open={!collapsed}>{children}</Collapse>;
}

/** A tree row's box, filled by whether it is picked. It follows the picked rows itself, so a pick
 * re-renders the rows it changes and not the whole tree. */
function PickRow({
  target,
  className,
  ...rest
}: HTMLAttributes<HTMLDivElement> & {
  target: Target;
  className: string;
  ref?: Ref<HTMLDivElement>;
}) {
  const picked = useIsRowSelected(target);
  return <div {...rest} className={`${className} ${selectionFill(picked)}`} />;
}

/** For projects that share a name, the end of each one's path that tells them apart, by project id.
 * A folder named like its project is left out, since the name already shows it. */
function telltalePaths(projects: Project[]): Map<string, string> {
  const labels = new Map<string, string>();
  const byName = new Map<string, Project[]>();
  for (const p of projects) byName.set(p.name, [...(byName.get(p.name) ?? []), p]);
  for (const group of byName.values()) {
    if (group.length < 2) continue;
    const parts = group.map((p) => {
      const segs = p.path.split(/[\\/]/).filter(Boolean);
      return segs.at(-1) === p.name ? segs.slice(0, -1) : segs;
    });
    const longest = Math.max(...parts.map((s) => s.length));
    for (let depth = 1; depth <= longest; depth++) {
      const tails = parts.map((s) => s.slice(-depth).join("/"));
      if (new Set(tails).size === tails.length || depth === longest) {
        group.forEach((p, i) => labels.set(p.id, tails[i]));
        break;
      }
    }
  }
  return labels;
}

function SectionLabel({
  children,
  action,
  ref,
}: {
  children: ReactNode;
  action?: ReactNode;
  ref?: Ref<HTMLDivElement>;
}) {
  return (
    <div
      ref={ref}
      className="flex items-center justify-between px-1.5 pt-2.5 pb-1 text-[length:var(--fs-xs)] tracking-[0.06em] text-fg-muted uppercase"
    >
      <span>{children}</span>
      {action}
    </div>
  );
}

function TasksView({
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
  // Set when a chat opens from the tree, so its composer does not take the focus from the tree.
  // Only set when the selection changes, since the effect that clears it runs on that change.
  const keepTreeFocus = useRef(false);
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
  const chats = threads.filter((t) => t.projectId === null);
  const chatIds = chats.map((t) => t.id);

  /** Ctrl+click adds or drops a row from the picked rows and returns true. A plain click clears them and returns false, so the row acts as usual. */
  const pickRow = (e: PickModifiers, target: Target) => {
    const { rows: treeRows } = buildTree(getCollapsed());
    const cursor = cursorRef.current;
    const anchor = cursor ? treeRows.findIndex((r) => sameTarget(r, cursor)) : -1;
    const at = treeRows.findIndex((r) => sameTarget(r, target));
    cursorRef.current = target;
    // Shift+click picks every tree row from the cursor to the clicked row, as VS Code's explorer does.
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
    return [
      ...(targets.length === 1
        ? [{ label: "Rename", Icon: PencilSimpleIcon, onSelect: () => setRenaming(targets[0]) }]
        : []),
      ...(targets.length === 1 && targets[0].kind === "project"
        ? [
            {
              label: "Change folder…",
              Icon: FolderIcon,
              onSelect: () => onRelocateProject(targets[0].id),
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
  const onSelect = useStable(onSelectThread);
  const onArchive = useStable((thread: Thread) =>
    onRemoveTargets([{ kind: "thread", id: thread.id }]),
  );
  const onPick = useStable(pickRow);
  const onContextMenu = useStable(openMenu);
  const onCommitRename = useStable(commitRename);
  const onCancelRename = useStable(() => setRenaming(null));
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
    const hidden = getCollapsed();
    setPicked(
      [
        ...chats,
        ...projects
          .filter((p) => !hidden.has(p.id))
          .flatMap((p) =>
            threads.filter(
              (t) =>
                t.projectId === p.id &&
                (t.folderId === null || !hidden.has(`folder:${t.folderId}`)),
            ),
          ),
      ]
        .filter((t) => !leavingIds.has(t.id))
        .map((t): Target => ({ kind: "thread", id: t.id })),
    );
  });

  // The tree's rows in on-screen order, each with its parent, for the arrow keys and Shift+click.
  // Built when a key or click needs it, from the collapsed set as it is then.
  const buildTree = (hidden: ReadonlySet<string>) => {
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
      for (const f of folders.filter((x) => x.projectId === p.id)) {
        const folderRow: Target = { kind: "folder", id: f.id };
        addRow(folderRow, projectRow);
        if (hidden.has(collapseKey(folderRow))) continue;
        for (const t of threads.filter((x) => x.folderId === f.id && !leavingIds.has(x.id)))
          addRow({ kind: "thread", id: t.id }, folderRow);
      }
      for (const t of threads.filter(
        (x) => x.projectId === p.id && x.folderId === null && !leavingIds.has(x.id),
      ))
        addRow({ kind: "thread", id: t.id }, projectRow);
    }
    return { rows, parent };
  };
  // The chats come first on screen, then the projects' rows, so archiving can step through them in that order.
  const syncTreeRows = () => {
    const chatRows: Target[] = chats
      .filter((t) => !leavingIds.has(t.id))
      .map((t) => ({ kind: "thread", id: t.id }));
    treeRowsRef.current = [...chatRows, ...buildTree(getCollapsed()).rows];
  };
  useEffect(syncTreeRows);
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
  useEffect(() => {
    if (!keepTreeFocus.current) return;
    keepTreeFocus.current = false;
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
        // Enter opens the chat and moves the focus into it, as Enter does in VS Code's explorer.
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
      className="group/tree flex min-h-0 flex-1 flex-col gap-px overflow-y-scroll overflow-x-hidden p-2 *:shrink-0 focus:outline-none"
      tabIndex={-1}
      {...shortcutContext("sidebarTreeFocus")}
      onKeyDown={onTreeKey}
      onClick={clearSelectionOnBlank}
    >
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
        <div className="px-3 py-2 text-[length:var(--fs-sm)] text-fg-muted">No chats yet</div>
      )}
      {chats.map((t) => (
        <Collapse key={t.id} open={!leavingIds.has(t.id)}>
          <ThreadRow
            thread={t}
            {...rowState(t)}
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
        <div className="px-3 py-2 text-[length:var(--fs-sm)] text-fg-muted">No projects yet</div>
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
          const tasks = threads.filter((t) => t.projectId === p.id);
          const projectFolders = folders.filter((f) => f.projectId === p.id);
          const group = `${TASKS_GROUP_PREFIX}${p.id}`;
          const topLevel = tasks.filter((t) => t.folderId === null);
          const topLevelIds = topLevel.map((t) => t.id);
          const taskRow = (t: Thread, folderId: string | null, ids: string[]) => (
            <Collapse key={t.id} open={!leavingIds.has(t.id)} overflowWhenOpen>
              <ThreadRow
                thread={t}
                {...rowState(t)}
                tree
                nested={folderId !== null}
                drag={reorder.row(group, t.id, ids, folderId)}
              />
            </Collapse>
          );
          return (
            <div key={p.id}>
              <PickRow
                target={row}
                {...drag.attrs}
                ref={
                  flashId === p.id ? (el) => el?.scrollIntoView({ block: "nearest" }) : undefined
                }
                onPointerDown={(e) => {
                  drag.attrs.onPointerDown(e);
                  if (!editing && isPlainPress(e)) pickRow(e, row);
                }}
                className={`group mx-1 flex h-7 cursor-pointer items-center rounded-[var(--radius-sm)] select-none border-none pr-1.5 pl-1 text-left text-fg ${drag.edge} ${drag.dragging ? "opacity-50" : ""} ${flashId === p.id ? "project-flash" : ""}`}
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
                  <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap font-medium">
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
                    className="btn-icon hidden h-5 w-5 flex-none group-hover:inline-flex"
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
                  className="btn-icon hidden h-5 w-5 flex-none group-hover:inline-flex"
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
                  className="btn-icon hidden h-5 w-5 flex-none group-hover:inline-flex"
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
                  className="btn-icon hidden h-5 w-5 flex-none group-hover:inline-flex"
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
                    <div className="px-5 py-2 text-[length:var(--fs-sm)] text-fg-muted">
                      No tasks yet
                    </div>
                  )}
                  {projectFolders.map((f) => {
                    const key = `folder:${f.id}`;
                    const inside = tasks.filter((t) => t.folderId === f.id);
                    const insideIds = inside.map((t) => t.id);
                    const folderEditing = renaming?.kind === "folder" && renaming.id === f.id;
                    const folderRow: Target = { kind: "folder", id: f.id };
                    const target = reorder.folder(group, key, insideIds, f.id);
                    return (
                      <div key={f.id}>
                        <PickRow
                          target={folderRow}
                          {...target.attrs}
                          onPointerDown={(e) => {
                            if (!folderEditing && isPlainPress(e)) pickRow(e, folderRow);
                          }}
                          className={`group mx-1 flex h-7 cursor-pointer items-center rounded-[var(--radius-sm)] select-none pr-1.5 pl-3 text-fg ${target.edge}`}
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
                            className="btn-icon hidden h-5 w-5 flex-none group-hover:inline-flex"
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
          className="mx-1 flex h-7 items-center gap-1.5 rounded-[var(--radius-sm)] border border-dashed border-accent px-2 text-[length:var(--fs-sm)] text-fg-muted"
        >
          <FolderPlusIcon size={15} />
          Add Folder
        </div>
      )}
    </div>
  );
}

/** Name field shown in place of a title while renaming. Commits once, on Enter or blur. */
function InlineName({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (name: string) => void;
  onCancel: () => void;
}) {
  // Enter, Escape and the blur that follows them must not each act.
  const done = useRef(false);
  const finish = (action: () => void) => {
    if (done.current) return;
    done.current = true;
    action();
  };
  const keyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === "Enter") finish(() => onCommit(e.currentTarget.value));
    else if (e.key === "Escape") finish(onCancel);
  };
  return (
    <input
      className="h-[22px] min-w-0 flex-1 border border-border-focus rounded-[var(--radius-sm)] bg-input px-1 text-fg outline-none"
      aria-label="Name"
      defaultValue={initial}
      autoFocus
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.stopPropagation()}
      onKeyDown={keyDown}
      onBlur={(e) => finish(() => onCommit(e.currentTarget.value))}
    />
  );
}

/** What a row does when used. These keep one identity across renders, so a row is not re-rendered
 * just because the tree around it was. */
interface RowActions {
  onSelectThread: (id: string) => void;
  onArchiveThread: (thread: Thread) => void;
  onPick: (e: PickModifiers, target: Target) => boolean;
  onContextMenu: (e: MouseEvent, target: Target) => void;
  onCommitRename: (target: Target, name: string, current: string) => void;
  onCancelRename: () => void;
}

interface ThreadRowProps {
  thread: Thread;
  /** The open chat. */
  selected: boolean;
  /** Being renamed. */
  editing: boolean;
  /** The current minute, so the row's relative time is redrawn when it changes. */
  minute: number;
  actions: RowActions;
  drag: RowDrag;
  nested?: boolean;
  /** In the Tasks tree: fixed-height rows that line up with the folders and projects. */
  tree?: boolean;
}

/** A row redraws only when something it shows changes. `drag.attrs` is left out of the comparison:
 * its handlers are rebuilt every render but act on refs and fixed ids, so an older copy works. */
function sameRow(a: ThreadRowProps, b: ThreadRowProps): boolean {
  return (
    a.thread === b.thread &&
    a.selected === b.selected &&
    a.editing === b.editing &&
    a.minute === b.minute &&
    a.actions === b.actions &&
    a.nested === b.nested &&
    a.tree === b.tree &&
    a.drag.edge === b.drag.edge &&
    a.drag.dragging === b.drag.dragging
  );
}

const ThreadRow = memo(function ThreadRow({
  thread,
  selected,
  editing,
  actions,
  drag,
  nested = false,
  tree = false,
}: ThreadRowProps) {
  const { onSelectThread, onArchiveThread, onPick, onContextMenu, onCommitRename, onCancelRename } =
    actions;
  const target: Target = { kind: "thread", id: thread.id };
  const picked = useIsRowSelected(target);
  const goalActive = thread.goal?.status === "active";
  const last = thread.messages[thread.messages.length - 1];
  const needsApproval = last?.approvals.some((a) => a.status === "pending") ?? false;
  // Tree rows are 28px tall. The indent and the empty chevron column sit in the margin, so the highlight starts just before the title and the titles still line up with folder names.
  const spacing = tree
    ? `h-7 rounded-[var(--radius-sm)] pr-2 pl-2 mr-1 ${nested ? "ml-[38px]" : "ml-[30px]"}`
    : `${nested ? "mb-1" : "my-0.5"} ${nested ? "ml-6 mr-1.5" : "mx-1.5"} px-2.5 py-1.5 rounded-[var(--radius-sm)]`;
  // The picked rows are the selection, and the open chat is always among them.
  const fill = picked
    ? tree
      ? selectionFill(true)
      : "bg-selected"
    : tree
      ? selectionFill(false)
      : "bg-[color-mix(in_srgb,var(--bg-hover)_40%,transparent)] hover:bg-hover";
  return (
    <div
      {...drag.attrs}
      data-thread-id={thread.id}
      onPointerDown={(e) => {
        drag.attrs.onPointerDown(e);
        if (!editing && isPlainPress(e)) onPick(e, target);
      }}
      className={`group flex cursor-pointer items-center gap-1.5 border-none select-none ${spacing} text-left text-fg ${fill} ${goalActive ? "goal-ring" : ""} ${drag.edge} ${drag.dragging ? "opacity-50" : ""}`}
      onClick={(e) => {
        if (!onPick(e, target)) onSelectThread(thread.id);
      }}
      onContextMenu={(e) => onContextMenu(e, { kind: "thread", id: thread.id })}
    >
      {editing ? (
        <InlineName
          initial={thread.title}
          onCommit={(name) => onCommitRename({ kind: "thread", id: thread.id }, name, thread.title)}
          onCancel={onCancelRename}
        />
      ) : (
        <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap">
          {displayTitle(thread.title)}
        </span>
      )}
      {/* A fixed, right-aligned slot so the time sits next to the archive button. It shows the time, or a spinner or approval shield. */}
      <span className="flex w-9 flex-none items-center justify-end">
        {needsApproval ? (
          <span title="Waiting for approval" className="inline-flex flex-none text-warning">
            <ShieldWarningIcon size={15} />
          </span>
        ) : isWorking(thread) ? (
          <span title="Working" className="inline-flex flex-none">
            <CircleNotchIcon size={15} className="spin text-accent-text" />
          </span>
        ) : (
          <span
            className={`text-[length:var(--fs-xs)] ${selected ? "text-fg-muted" : "text-fg-subtle"}`}
          >
            {formatRelative(last?.createdAt ?? thread.createdAt)}
          </span>
        )}
      </span>
      <button
        className="btn-icon h-5 w-5 flex-none hover:bg-border-strong"
        title="Archive"
        aria-label="Archive"
        onClick={(e) => {
          e.stopPropagation();
          onArchiveThread(thread);
        }}
      >
        <ArchiveIcon size={16} />
      </button>
      <span
        className="inline-flex h-5 w-5 flex-none items-center justify-center"
        title={AGENT_LABELS[thread.agent]}
      >
        <ProviderLogos kind={thread.agent} size={16} />
      </span>
    </div>
  );
}, sameRow);

/** Chats and tasks moved out of the task list. Restoring one puts it back where it was. */
function ArchivedView({
  archivedThreads,
  onSelectThread,
  onRestoreThread,
  onDeleteArchived,
}: SidebarProps) {
  const picked = useSelectedRows();
  const setPicked = setSelectedRows;
  const [menu, setMenu] = useState<{ x: number; y: number; thread: Thread } | null>(null);
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
  useCommand("sidebar.selectAllArchived", () =>
    setPicked(archivedThreads.map((t) => targetOf(t.id))),
  );
  return (
    <div
      tabIndex={-1}
      {...shortcutContext("archivedListFocus")}
      onClick={clearSelectionOnBlank}
      className="flex min-h-0 flex-1 flex-col gap-px overflow-y-scroll overflow-x-hidden p-2 *:shrink-0 focus:outline-none"
    >
      <SectionLabel>Archived</SectionLabel>
      {archivedThreads.length === 0 && (
        <div className="px-3 py-2 text-[length:var(--fs-sm)] text-fg-muted">Nothing archived</div>
      )}
      {archivedThreads.map((t) => (
        <div
          key={t.id}
          className={`group mx-1.5 mb-1.5 flex cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] px-2.5 py-1.5 text-fg ${isPicked(t.id) ? "bg-selected" : "bg-[color-mix(in_srgb,var(--bg-hover)_40%,transparent)] hover:bg-hover"}`}
          onClick={(e) => pick(e, t.id)}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, thread: t });
          }}
        >
          <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap">
            {displayTitle(t.title)}
          </span>
          <button
            className="btn-icon hidden h-5 w-5 flex-none group-hover:inline-flex hover:bg-border-strong"
            title="Restore"
            aria-label="Restore"
            onClick={(e) => {
              e.stopPropagation();
              onRestoreThread(t.id);
            }}
          >
            <ArrowCounterClockwiseIcon size={16} />
          </button>
        </div>
      ))}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={[
            {
              label: "Restore",
              Icon: ArrowCounterClockwiseIcon,
              onSelect: () => onRestoreThread(menu.thread.id),
            },
            {
              label: "Delete permanently",
              Icon: TrashIcon,
              danger: true,
              onSelect: () => onDeleteArchived(menu.thread.id),
            },
          ]}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}

/** A small text button for acting on several changed files at once. */
function BulkButton({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      className="cursor-pointer rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-muted hover:bg-hover hover:text-fg"
      onClick={onClick}
    >
      {children}
    </button>
  );
}

/** One changed file: opens its diff. */
function ChangeRow({ change, onOpen }: { change: FileChange; onOpen: () => void }) {
  const name = baseName(change.path);
  return (
    <div className="flex h-[30px] items-center rounded-[var(--radius-sm)] py-1 pr-2 pl-1.5 hover:bg-hover">
      <button
        className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 border-none bg-transparent p-0 text-fg text-left"
        title={change.path}
        onClick={onOpen}
      >
        <span className="min-w-0 flex-1 truncate">{name}</span>
        {/* The icon column has a fixed width, so the icons line up across rows. */}
        <span className="flex flex-none items-center gap-2 text-[length:var(--fs-xs)] tabular-nums">
          <span className="flex items-center gap-1">
            <span className="text-success">+{change.additions}</span>
            <span className="text-danger">−{change.deletions}</span>
          </span>
          <span className="flex w-3 justify-center">
            <GitDiffIcon size={14} className="text-fg-muted" aria-label="Changed" />
          </span>
        </span>
      </button>
    </div>
  );
}

/** Changed files. Those from the reply the view was opened from are pinned on top; the rest are grouped by folder. */
function ChangesView({
  project,
  threads,
  selectedId,
  changes,
  changeFocus,
  onClearChangeFocus,
  onRefreshChanges,
  onOpenChange,
}: SidebarProps) {
  const [error, setError] = useState<string | null>(null);
  // With full access nothing waits for review, so the bulk actions are left out.
  const fullAccess = threads.find((t) => t.id === selectedId)?.accessMode === "full-access";
  // The reply's files, listed whether or not they still have uncommitted changes.
  const replyFiles = changeFocus ?? [];
  const pinned = changes.filter((c) => replyFiles.includes(c.path));
  const folders = new Map<string, FileChange[]>();
  for (const c of changes) {
    if (pinned.includes(c)) continue;
    const dir = dirName(c.path);
    folders.set(dir, [...(folders.get(dir) ?? []), c]);
  }

  async function act(action: () => Promise<void>) {
    setError(null);
    try {
      await action();
      onRefreshChanges();
    } catch (e) {
      setError(errorText(e));
    }
  }

  /** Stages every file in `files`, one after another. */
  function applyAll(files: FileChange[]) {
    if (!project) return;
    void act(async () => {
      for (const f of files) await api.acceptFile(project.id, f.path);
    });
  }

  /** Reverts every file in `files` after one confirmation. */
  async function rejectAll(files: FileChange[]) {
    if (!project || files.length === 0) return;
    const count = files.length === 1 ? "1 file" : `${files.length} files`;
    if (!(await confirm(`Discard changes to ${count}?`, { title: "Reject", kind: "warning" }))) {
      return;
    }
    void act(async () => {
      for (const f of files) await api.revertFile(project.id, f.path);
    });
  }

  const row = (c: FileChange) => (
    <ChangeRow key={c.path} change={c} onOpen={() => onOpenChange(c.path)} />
  );

  return (
    <>
      <div className="flex h-8 flex-none items-center justify-between pr-2 pl-3 text-[length:var(--fs-sm)] font-semibold">
        <span>Changes</span>
        <button
          className="btn-icon"
          title="Refresh"
          aria-label="Refresh"
          onClick={onRefreshChanges}
        >
          <ArrowsClockwiseIcon size={18} />
        </button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-scroll p-2">
        {changes.length > 0 && !fullAccess && (
          <div className="flex justify-end gap-1 px-1">
            <BulkButton onClick={() => applyAll(changes)}>Stage all</BulkButton>
            <BulkButton onClick={() => void rejectAll(changes)}>Reject all</BulkButton>
          </div>
        )}
        {changes.length === 0 && (
          <div className="px-3 py-2 text-[length:var(--fs-sm)] text-fg-muted">No changes</div>
        )}
        {replyFiles.length > 0 && (
          <div className="flex flex-col gap-px">
            <div className="flex items-center gap-1.5 px-1.5 pt-1 text-[length:var(--fs-xs)] text-fg-subtle">
              <ChatCircleIcon size={14} />
              <span className="min-w-0 flex-1 truncate">From this reply</span>
              {pinned.length > 0 && !fullAccess && (
                <span className="flex gap-1">
                  <BulkButton onClick={() => applyAll(pinned)}>Stage</BulkButton>
                  <BulkButton onClick={() => void rejectAll(pinned)}>Reject</BulkButton>
                </span>
              )}
              <button
                className="ml-auto flex flex-none cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 py-0.5 text-[length:var(--fs-xs)] text-fg-subtle hover:bg-hover hover:text-fg"
                onClick={onClearChangeFocus}
              >
                <ArrowLeftIcon size={14} />
                Project
              </button>
            </div>
            {replyFiles.map((path) => {
              const change = changes.find((c) => c.path === path);
              return change ? (
                row(change)
              ) : (
                <button
                  key={path}
                  className="flex min-h-[26px] w-full cursor-pointer items-center gap-2 rounded-[var(--radius-sm)] border-none bg-transparent py-0.5 pr-1 pl-1.5 text-fg text-left hover:bg-hover"
                  title={path}
                  onClick={() => onOpenChange(path)}
                >
                  <span className="min-w-0 flex-1 truncate">{baseName(path)}</span>
                  <PencilSimpleIcon size={14} className="flex-none text-fg-muted" />
                </button>
              );
            })}
          </div>
        )}
        {[...folders]
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([dir, files]) => (
            <div key={dir} className="flex flex-col gap-px">
              <div className="flex items-center gap-1.5 px-1.5 pt-1 text-[length:var(--fs-xs)] text-fg-subtle">
                <FolderIcon size={14} />
                <span className="min-w-0 truncate">{dir || "Project root"}</span>
              </div>
              {files.map(row)}
            </div>
          ))}
        {error && <div className="px-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}
      </div>
    </>
  );
}
