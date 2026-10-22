import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { XIcon } from "@phosphor-icons/react";
import { confirm } from "@tauri-apps/plugin-dialog";
import {
  api,
  defaultAgentOf,
  type FileChange,
  type Message,
  type Settings,
  type Thread,
} from "~/api";
import type { UpdateSettings } from "~/app/App";
import type { CanvasTab } from "~/components/canvas/Canvas";
import { lastAccessMode } from "~/components/chat/AccessPicker";
import { lastEffort } from "~/components/chat/EffortPicker";
import Chat from "~/components/chat/Chat";
import CommandPalette, { type PaletteItem } from "~/components/workspace/CommandPalette";
import SettingsDialog from "~/components/settings/SettingsDialog";
import { openFolderAsProject, syncProjects } from "~/lib/projects";
import Sidebar, { ToolStripe, type SidebarView, type Target } from "~/components/sidebar/Sidebar";
import { getPicked, setPickedRows } from "~/components/sidebar/pickedRows";
import StatusBar from "~/components/workspace/StatusBar";
import Toolbar from "~/components/workspace/Toolbar";
import { isWorking, useThreads } from "~/hooks/useThreads";
import { keyLabel, useCommand, useContextKey, useKeybindings } from "~/lib/shortcuts";
import { AGENT_KINDS, displayTitle, errorText } from "~/lib/util";

export interface WorkspaceProps {
  settings: Settings;
  updateSettings: UpdateSettings;
  toggleTheme: () => void;
}

interface CanvasState {
  /** Bumped on every open so the canvas re-initialises its tab. */
  seq: number;
  projectId: string;
  path: string | null;
  message: Message | null;
  tab: CanvasTab;
}

interface GitState {
  projectId: string;
  branch: string | null;
  changes: FileChange[];
}

const CANVAS_MIN_PX = 340;
const CANVAS_MAX_FRACTION = 0.7;
// How long an archived row takes to collapse out of the list. Matches the sidebar's Collapse.
const ARCHIVE_COLLAPSE_MS = 120;

// One shared empty list, so the chat's memoized rows see the same `changes` on every render.
const NO_CHANGES: FileChange[] = [];

function isTypingTarget(el: Element | null): boolean {
  return (
    el instanceof HTMLElement &&
    (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName))
  );
}

// The canvas (with its syntax highlighter) is only shown on demand, so it loads as a separate
// chunk instead of making the first screen wait for it. Once loaded, its module is kept and
// rendered directly: a lazy component suspends on every first render even when loaded, and
// React holds a suspended reveal for most of a second.
let canvasModule: typeof import("~/components/canvas/Canvas") | null = null;
const loadCanvas = () => import("~/components/canvas/Canvas").then((m) => (canvasModule = m));
const LazyCanvas = lazy(loadCanvas);
/** How long after the workspace shows the canvas chunk starts loading, so it is ready before
 * it is first opened. */
const PRELOAD_AFTER_MS = 1500;

export default function Workspace({ settings, updateSettings, toggleTheme }: WorkspaceProps) {
  const Canvas = canvasModule?.default ?? LazyCanvas;
  const {
    threads,
    reload,
    selected,
    select,
    createThread,
    deleteThread,
    setThreadArchived,
    renameThread,
    clearThread,
    folders,
    createFolder,
    renameFolder,
    removeFolder,
    reorderThreads,
    sortByAgent,
    undoSortByAgent,
    setModel,
    setAccessMode,
    setPlanMode,
    setEffort,
    send,
    startThread,
  } = useThreads(settings.activeProjectId, () => void refreshGit());
  // The selected thread decides the project (null: a chat without a project);
  // with nothing selected, the last active project does.
  const project =
    settings.projects.find(
      (p) => p.id === (selected ? selected.projectId : settings.activeProjectId),
    ) ?? null;
  const projectId = project?.id ?? null;
  // Archived chats and tasks leave the task list and the palette; the Archived view lists them.
  // Subagents' threads are opened from their cards, not listed.
  const liveThreads = threads.filter((t) => t.archivedAt === null && !t.parent);
  const archivedThreads = threads.filter((t) => t.archivedAt !== null && !t.parent);
  const defaultAgent = defaultAgentOf(settings);
  // Display order stays fixed; `settings.agents` order only marks the default.
  const connectedAgents = AGENT_KINDS.filter((k) => settings.agents.includes(k));
  // The sidebar shows where threads sit and their status, never message text. Its props keep
  // their identity until something it shows changes, so streamed text does not re-render it.
  const sidebarKey = threads.map(sidebarSignature).join("\n");
  const selectedId = selected?.id ?? null;
  const sidebarThreads = useMemo(
    () => ({
      live: liveThreads,
      archived: archivedThreads,
    }),
    [sidebarKey],
  );
  const agentsKey = connectedAgents.join();
  const sidebarAgents = useMemo(
    () => (connectedAgents.length > 0 ? connectedAgents : [defaultAgent]),
    [agentsKey, defaultAgent],
  );

  const [view, setView] = useState<SidebarView | null>("tasks");
  // The side nav Space reopens: the last view that was open.
  const lastView = useRef<SidebarView>("tasks");
  useEffect(() => {
    if (view) lastView.current = view;
  }, [view]);
  useEffect(() => {
    const timer = setTimeout(() => void loadCanvas(), PRELOAD_AFTER_MS);
    return () => clearTimeout(timer);
  }, []);
  // Rows picked for removing together, in the view they were picked in; Ctrl+W and Delete act on them.
  // They live in a store the sidebar reads, so picking a row does not re-render the open chat.
  useEffect(() => {
    setPickedRows([]);
  }, [view]);
  // Chats and tasks being archived: their rows stay mounted, collapsing, until the archive lands.
  const [leavingIds, setLeavingIds] = useState<ReadonlySet<string>>(new Set());
  // The sidebar's tree rows in on-screen order, written by the sidebar after each render.
  const treeRowsRef = useRef<Target[]>([]);
  // Forgets a row once its chat is no longer live, so restoring it later opens at full height.
  useEffect(() => {
    setLeavingIds((prev) => {
      const next = new Set(
        [...prev].filter((id) => threads.some((t) => t.id === id && t.archivedAt === null)),
      );
      return next.size === prev.size ? prev : next;
    });
  }, [threads]);
  // Files of the reply whose "Show changes" was clicked; pinned at the top of the Changes view.
  const [changeFocus, setChangeFocus] = useState<string[] | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [canvas, setCanvas] = useState<CanvasState | null>(null);
  const [canvasWidth, setCanvasWidth] = useState<number | null>(null);
  const [git, setGit] = useState<GitState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const bodyRef = useRef<HTMLDivElement>(null);

  // A canvas belongs to the project it was opened in; it is hidden after a project switch.
  const openCanvas = canvas && canvas.projectId === projectId ? canvas : null;
  const changes = git && git.projectId === projectId ? git.changes : NO_CHANGES;
  const branch = git && git.projectId === projectId ? git.branch : null;

  const refreshGit = useCallback(() => {
    if (!projectId) return Promise.resolve();
    return Promise.all([
      api.gitBranch(projectId).catch(() => null),
      api.gitChanges(projectId).catch(() => []),
    ]).then(([nextBranch, nextChanges]) =>
      setGit({ projectId, branch: nextBranch, changes: nextChanges }),
    );
  }, [projectId]);

  useEffect(() => {
    void refreshGit();
  }, [refreshGit]);

  function attempt(fn: () => Promise<unknown>) {
    setError(null);
    fn().catch((e: unknown) => setError(errorText(e)));
  }

  const openFolder = () => attempt(() => openFolderAsProject(updateSettings));
  const renameProjectTo = (id: string, name: string) =>
    attempt(async () => {
      await api.renameProject(id, name);
      await syncProjects(updateSettings);
    });
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
    const name = settings.projects.find((p) => p.id === target.id)?.name ?? "this project";
    return confirm(`Remove ${name} and its tasks?`, { title: "Remove project", kind: "warning" });
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
    setPickedRows([]);
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

  /** Asks, then deletes an archived chat or task for good. */
  const deleteArchived = async (id: string) => {
    const title = threads.find((t) => t.id === id)?.title ?? "";
    if (
      !(await confirm(
        `Delete ${displayTitle(title)} permanently? Its messages cannot be recovered.`,
        {
          title: "Delete",
          kind: "warning",
        },
      ))
    )
      return;
    attempt(() => deleteThread(id));
  };

  /** Asks once, then deletes the picked archived chats or tasks for good. */
  const deleteArchivedMany = async (ids: string[]) => {
    if (ids.length === 1) return deleteArchived(ids[0]);
    if (
      !(await confirm(
        `Delete ${ids.length} items permanently? Their messages cannot be recovered.`,
        {
          title: "Delete",
          kind: "warning",
        },
      ))
    )
      return;
    setPickedRows([]);
    attempt(async () => {
      for (const id of ids) await deleteThread(id);
    });
  };

  /** Ctrl+W and Delete act on picked rows: in the archive they delete them, elsewhere they remove them. */
  const removePicked = (targets: Target[]) =>
    view === "archived" ? deleteArchivedMany(targets.map((t) => t.id)) : removeTargets(targets);

  // Project order is saved with the settings; the backend keeps the list's order.
  const reorderProjects = (ids: string[]) =>
    attempt(() =>
      updateSettings((s) => ({
        ...s,
        projects: ids.flatMap((id) => s.projects.find((p) => p.id === id) ?? []),
      })),
    );
  const reorderThreadsTo = (ids: string[], folderId: string | null) =>
    attempt(() => reorderThreads(ids, folderId));
  const newFolder = (projectId: string) =>
    createFolder(projectId).catch((e: unknown) => {
      setError(errorText(e));
      return null;
    });

  /** Remembers the current project so the next launch reopens it. */
  const rememberProject = async (id: string | null) => {
    if (settings.activeProjectId !== id)
      await updateSettings((s) => ({ ...s, activeProjectId: id }));
  };

  const selectThread = (id: string) => {
    // Opening a chat makes it the whole selection, even when it was already the open chat.
    setPickedRows([{ kind: "thread", id }]);
    select(id);
    const thread = threads.find((t) => t.id === id);
    if (thread) attempt(() => rememberProject(thread.projectId));
  };

  const selectProject = (id: string) => {
    select(threads.find((t) => t.projectId === id)?.id ?? null);
    attempt(() => rememberProject(id));
  };

  /** New thread in a project, or a chat without a project when `id` is null. */
  const newThreadIn = (id: string | null) =>
    attempt(async () => {
      await createThread(id, {
        agent: defaultAgent,
        model: settings.tokens.defaultModels[defaultAgent] ?? null,
        accessMode: lastAccessMode(),
        effort: lastEffort(defaultAgent),
      });
      await rememberProject(id);
    });
  const newChat = () => newThreadIn(null);

  /** Creates a task at the project's top level, then moves it to the top of `folderId`. */
  const newThreadInFolder = (projectId: string, folderId: string, threadIds: string[]) =>
    attempt(async () => {
      const thread = await createThread(projectId, {
        agent: defaultAgent,
        model: settings.tokens.defaultModels[defaultAgent] ?? null,
        accessMode: lastAccessMode(),
        effort: lastEffort(defaultAgent),
      });
      await reorderThreads([thread.id, ...threadIds], folderId);
      await rememberProject(projectId);
    });

  /** Ctrl+N: a task in the selected task's folder or project; a chat when a chat or nothing is selected. */
  const newThreadHere = () => {
    if (!selected?.projectId) return newChat();
    if (selected.folderId === null) return newThreadIn(selected.projectId);
    const folderTaskIds = liveThreads
      .filter((t) => t.folderId === selected.folderId)
      .map((t) => t.id);
    return newThreadInFolder(selected.projectId, selected.folderId, folderTaskIds);
  };

  const openCanvasAt = (path: string, message: Message | null, tab: CanvasTab) => {
    if (!projectId) return;
    setCanvas((prev) => ({ seq: (prev?.seq ?? 0) + 1, projectId, path, message, tab }));
  };

  function startResize(e: ReactMouseEvent) {
    const body = bodyRef.current;
    if (!body) return;
    e.preventDefault();
    const right = body.getBoundingClientRect().right;
    const max = body.clientWidth * CANVAS_MAX_FRACTION;
    const onMove = (ev: MouseEvent) =>
      setCanvasWidth(Math.min(max, Math.max(CANVAS_MIN_PX, right - ev.clientX)));
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }

  // What each shortcut does; which key runs it is set in lib/shortcutDefaults.ts and the Shortcuts
  // settings. The handlers read current state, so they need no re-registering.
  useCommand("palette.toggle", () => setPaletteOpen((o) => !o));
  useCommand("chat.new", () => newThreadIn(projectId));
  useCommand("chat.newHere", newThreadHere);
  useCommand("settings.open", () => setSettingsOpen(true));
  useCommand("composer.focus", () =>
    document.querySelector<HTMLElement>(".composer-editor .ProseMirror")?.focus(),
  );
  // Closes the open side panel, or reopens the last one, wherever focus sits after a click.
  useCommand("sidePanel.toggle", () => setView((current) => (current ? null : lastView.current)));
  // Acts on the picked rows and, while the compose box is not active, on the open chat in the task
  // list even when it is not picked.
  useCommand("workspace.closeChat", () => {
    const picked = getPicked();
    const openChat =
      view === "tasks" &&
      selected &&
      !isTypingTarget(document.activeElement) &&
      !picked.some((p) => p.kind === "thread" && p.id === selected.id);
    const targets = openChat ? [...picked, { kind: "thread" as const, id: selected.id }] : picked;
    if (targets.length > 0) void removePicked(targets);
  });
  // Acts on the picked rows; with none picked, it archives the open chat in the task list.
  useCommand("workspace.deleteChat", () => {
    const picked = getPicked();
    if (picked.length > 0) void removePicked(picked);
    else if (view === "tasks" && selected)
      void removeTargets([{ kind: "thread", id: selected.id }]);
  });
  useCommand("palette.close", () => setPaletteOpen(false));
  useCommand("settings.close", () => setSettingsOpen(false));
  useCommand("canvas.close", () => setCanvas(null));
  useCommand("sidebar.clearPicked", () => setPickedRows([]));
  useContextKey("paletteOpen", () => paletteOpen);
  useContextKey("settingsOpen", () => settingsOpen);
  useContextKey("canvasOpen", () => openCanvas !== null);
  useContextKey("hasPickedRows", () => getPicked().length > 0);

  const { rules } = useKeybindings();
  const paletteItems: PaletteItem[] = [
    {
      id: "new-chat",
      group: "Actions",
      label: "New chat",
      hint: project ? undefined : keyLabel(rules, "chat.new"),
      run: newChat,
    },
    ...(project
      ? [
          {
            id: "new-task",
            group: "Actions",
            label: `New task in ${project.name}`,
            hint: keyLabel(rules, "chat.new"),
            run: () => newThreadIn(project.id),
          },
        ]
      : []),
    { id: "toggle-theme", group: "Actions", label: "Toggle theme", run: toggleTheme },
    {
      id: "settings",
      group: "Actions",
      label: "Open settings",
      hint: keyLabel(rules, "settings.open"),
      run: () => setSettingsOpen(true),
    },
    { id: "open-folder", group: "Actions", label: "Open folder…", run: openFolder },
    // Development only: a fresh load also rebuilds the composer editor.
    ...(import.meta.env.DEV
      ? [
          {
            id: "reload-webview",
            group: "Actions",
            label: "Reload webview",
            hint: "Ctrl R",
            run: () => window.location.reload(),
          },
        ]
      : []),
    ...liveThreads.map((t) => ({
      id: `task-${t.id}`,
      group: settings.projects.find((p) => p.id === t.projectId)?.name ?? "Chats",
      label: displayTitle(t.title),
      run: () => {
        setView("tasks");
        selectThread(t.id);
      },
    })),
    ...settings.projects.map((p) => ({
      id: `project-${p.id}`,
      group: "Projects",
      label: p.name,
      hint: p.path,
      run: () => selectProject(p.id),
    })),
  ];

  const sidebarHandlers = useStableHandlers({
    onRestoreThread: (id: string) => attempt(() => setThreadArchived(id, false)),
    onDeleteArchived: deleteArchived,
    onClearChangeFocus: () => setChangeFocus(null),
    onNewTask: newThreadIn,
    onNewChat: newChat,
    onSelectThread: selectThread,
    onRenameThread: (id: string, title: string) => attempt(() => renameThread(id, title)),
    onRenameProject: renameProjectTo,
    onRemoveTargets: removeTargets,
    onReorderProjects: reorderProjects,
    onReorderThreads: reorderThreadsTo,
    onNewFolder: newFolder,
    onSortByAgent: (projectId: string) => attempt(() => sortByAgent(projectId)),
    onUndoSortByAgent: (projectId: string) => attempt(() => undoSortByAgent(projectId)),
    onRenameFolder: (id: string, name: string) => attempt(() => renameFolder(id, name)),
    onNewTaskInFolder: newThreadInFolder,
    onOpenFolder: openFolder,
    onRefreshChanges: () => attempt(refreshGit),
    onOpenChange: (path: string) => openCanvasAt(path, null, "diff"),
    onOpenPalette: () => setPaletteOpen(true),
  });

  return (
    <div className="grid h-full grid-rows-[var(--toolbar-h)_minmax(0,1fr)_var(--statusbar-h)] bg-app text-fg">
      <Toolbar
        project={project}
        projects={settings.projects}
        onSelectProject={selectProject}
        onOpenFolder={openFolder}
      />

      <div className="flex min-h-0 min-w-0" ref={bodyRef}>
        <ToolStripe
          view={view}
          onSelect={(v) => setView(view === v ? null : v)}
          onOpenSettings={() => setSettingsOpen(true)}
        />
        {view && (
          <Sidebar
            view={view}
            agents={sidebarAgents}
            project={project}
            projects={settings.projects}
            threads={sidebarThreads.live}
            leavingIds={leavingIds}
            treeRowsRef={treeRowsRef}
            archivedThreads={sidebarThreads.archived}
            selectedId={selectedId}
            changes={changes}
            changeFocus={changeFocus}
            folders={folders}
            {...sidebarHandlers}
          />
        )}

        <div className="flex min-h-0 min-w-0 flex-1">
          <main className="flex min-w-0 flex-1 bg-editor">
            <Chat
              project={project}
              thread={selected}
              parentThread={
                (selected?.parent && threads.find((t) => t.id === selected.parent?.threadId)) ||
                null
              }
              onOpenThread={select}
              changes={changes}
              defaultAgent={defaultAgent}
              agents={connectedAgents}
              tokens={settings.tokens}
              onUpdateTokens={(patch) =>
                void updateSettings((s) => ({ ...s, tokens: { ...s.tokens, ...patch } }))
              }
              onOpenFolder={openFolder}
              onOpenCanvas={openCanvasAt}
              onShowChanges={(paths) => {
                setChangeFocus(paths);
                setView("changes");
              }}
              onSend={(text, attachments, draft, queue) =>
                send(text, attachments, draft, projectId, queue)
              }
              onNewChat={() => newThreadIn(projectId)}
              onClearThread={(id) => clearThread(id).catch((e: unknown) => setError(errorText(e)))}
              onCancel={(id) => api.cancelTurn(id)}
              onSetModel={setModel}
              onSetAccessMode={setAccessMode}
              onSetPlanMode={setPlanMode}
              onStartThread={(opts, title, text) =>
                startThread(projectId, opts, title, text).catch((e: unknown) =>
                  setError(errorText(e)),
                )
              }
              onSetEffort={setEffort}
            />
          </main>

          {openCanvas && (
            <>
              <div
                className="relative w-1 flex-none cursor-col-resize after:absolute after:top-0 after:bottom-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-border after:content-['']"
                onMouseDown={startResize}
              />
              <div
                className="flex w-[max(420px,46%)] min-h-0 min-w-0 flex-none [&>*]:min-w-0 [&>*]:flex-1"
                style={canvasWidth !== null ? { width: canvasWidth } : undefined}
              >
                <Suspense fallback={null}>
                  <Canvas
                    key={openCanvas.seq}
                    projectId={openCanvas.projectId}
                    path={openCanvas.path}
                    message={openCanvas.message}
                    initialTab={openCanvas.tab}
                    onClose={() => setCanvas(null)}
                    onChangesUpdated={() => void refreshGit()}
                  />
                </Suspense>
              </div>
            </>
          )}
        </div>
      </div>

      <StatusBar
        project={project}
        branch={branch}
        threadTitle={selected ? displayTitle(selected.title) : null}
        changeCount={changes.length}
        agent={selected?.agent ?? defaultAgent}
        working={isWorking(selected)}
        planMode={selected?.planMode ?? false}
        onOpenChanges={() => {
          setChangeFocus(null);
          setView("changes");
        }}
      />

      {error && (
        <div
          className="fixed right-4 bottom-[calc(var(--statusbar-h)_+_12px)] z-[95] flex max-w-[420px] items-center gap-2 rounded-[var(--radius)] border border-danger bg-elevated py-2 pr-2 pl-3 text-[length:var(--fs-sm)] shadow-[var(--shadow-popup)]"
          role="alert"
        >
          <span>{error}</span>
          <button className="btn-icon" aria-label="Dismiss" onClick={() => setError(null)}>
            <XIcon size={14} />
          </button>
        </div>
      )}
      {paletteOpen && <CommandPalette items={paletteItems} onClose={() => setPaletteOpen(false)} />}
      {settingsOpen && (
        <SettingsDialog
          settings={settings}
          updateSettings={updateSettings}
          onImported={() => void reload()}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}

/** Signatures by thread object: a change makes a new object, so each is computed once, not on
 * every streamed update for every thread. */
const signatures = new WeakMap<Thread, string>();

/** What the sidebar shows of a thread; it re-renders only when one of these changes. */
function sidebarSignature(t: Thread): string {
  let signature = signatures.get(t);
  if (signature === undefined) {
    signature = computeSidebarSignature(t);
    signatures.set(t, signature);
  }
  return signature;
}

function computeSidebarSignature(t: Thread): string {
  const last = t.messages[t.messages.length - 1];
  const needsApproval = last?.approvals.some((a) => a.status === "pending") ?? false;
  return [
    t.id,
    t.title,
    t.projectId,
    t.folderId,
    t.archivedAt,
    t.agent,
    t.accessMode,
    t.goal?.status,
    last?.createdAt,
    needsApproval,
    isWorking(t),
  ].join("|");
}

/**
 * Stand-ins for `handlers` that keep their identity and call the latest render's versions,
 * so a memoized child can skip renders without holding stale closures. Keys must not change.
 */
function useStableHandlers<T extends Record<string, (...args: never[]) => unknown>>(
  handlers: T,
): T {
  const latest = useRef(handlers);
  useLayoutEffect(() => {
    latest.current = handlers;
  });
  const [stable] = useState(
    () =>
      Object.fromEntries(
        Object.keys(handlers).map((key) => [
          key,
          (...args: never[]) => latest.current[key](...args),
        ]),
      ) as T,
  );
  return stable;
}
