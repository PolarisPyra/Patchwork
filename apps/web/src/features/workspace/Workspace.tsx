import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  defaultAgentOf,
  type Message,
  type PermissionScope,
  type PermissionSettings,
  type Settings,
} from "~/api";
import type { UpdateSettings } from "~/api";
import type { CanvasTab } from "~/features/canvas/Canvas";
import { lastAccessMode } from "~/components/AccessPicker";
import { lastCompactWindow } from "~/components/CompactPicker";
import { lastEffort } from "~/components/EffortPicker";
import Chat from "~/features/chat";
import CommandPalette from "~/features/workspace/components/CommandPalette";
import SettingsDialog from "~/features/settings/SettingsDialog";
import { addFolders, openFolderAsProject, relocateProject, syncProjects } from "~/lib/projects";
import Sidebar, {
  ActivityBar,
  getSelectedRows,
  setSelectedRows,
  sidebarSignature,
  type SidebarView,
  type Target,
} from "~/features/sidebar";
import { getCollapsed, setCollapsed } from "~/lib/collapsedState";
import CanvasArea, {
  loadCanvas,
  type CanvasState,
} from "~/features/workspace/components/CanvasArea";
import ErrorToast from "~/features/workspace/components/ErrorToast";
import PanelArea, { loadPanel } from "~/features/workspace/components/PanelArea";
import StatusBar from "~/features/workspace/components/StatusBar";
import Toolbar from "~/features/workspace/components/Toolbar";
import { paletteItems } from "~/features/workspace/paletteItems";
import { useThreadRemoval } from "~/features/workspace/useThreadRemoval";
import { useProjectGit } from "~/features/workspace/useProjectGit";
import { useStableHandlers } from "~/hooks/useStableHandlers";
import { setRendererLogLevel } from "~/lib/rendererLog";
import { useThreads } from "~/hooks/useThreads";
import { isBusy } from "~/lib/threadEvents";
import { useCommand, useContextKey, useKeybindings } from "~/lib/shortcuts";
import { AGENT_KINDS, displayTitle, errorText } from "~/lib/util";

export interface WorkspaceProps {
  settings: Settings;
  updateSettings: UpdateSettings;
  toggleTheme: () => void;
}

function isTypingTarget(el: Element | null): boolean {
  return (
    el instanceof HTMLElement &&
    (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName))
  );
}

/** How long after the workspace shows the canvas chunk starts loading, so it is ready before
 * it is first opened. */
const PRELOAD_AFTER_MS = 1500;

/**
 * The window: toolbar, activity bar and sidebar, the chat with the file panel beside it and the
 * terminal panel under it, the status bar, and the dialogs over them. It owns which thread and
 * project are open and what the sidebar shows; the parts below each own their own state.
 */
export default function Workspace({ settings, updateSettings, toggleTheme }: WorkspaceProps) {
  const {
    threads,
    reload,
    selected,
    select,
    opening,
    createThread,
    deleteThread,
    setThreadArchived,
    renameThread,
    clearThread,
    restoreCheckpoint,
    forkThread,
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
    setCompactWindow,
    send,
    startThread,
  } = useThreads(settings.activeProjectId);
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
  const connectedAgents = useMemo(
    () => AGENT_KINDS.filter((k) => settings.agents.includes(k)),
    [settings.agents],
  );
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
  const [view, setView] = useState<SidebarView | null>("tasks");
  // The side nav Space reopens: the last view that was open.
  const lastView = useRef<SidebarView>("tasks");
  useEffect(() => {
    if (view) lastView.current = view;
  }, [view]);
  useEffect(() => {
    const timer = setTimeout(() => {
      void loadCanvas();
      void loadPanel();
    }, PRELOAD_AFTER_MS);
    return () => clearTimeout(timer);
  }, []);
  // Rows picked for removing together, in the view they were picked in; Ctrl+W and Delete act on them.
  // They live in a store the sidebar reads, so picking a row does not re-render the open chat.
  useEffect(() => {
    setSelectedRows([]);
  }, [view]);
  // The sidebar's tree rows in on-screen order, written by the sidebar after each render.
  const treeRowsRef = useRef<Target[]>([]);
  // The reply whose "Show changes" was clicked; its files are pinned at the top of the Changes
  // view and follow the reply as it keeps editing.
  const [changeFocus, setChangeFocus] = useState<{ threadId: string; messageId: string } | null>(
    null,
  );
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // The permission rule Settings opens on, from an approval in the transcript.
  const [settingsFocus, setSettingsFocus] = useState<{
    scope: PermissionScope;
    ruleId?: string;
  } | null>(null);
  const openPermissions = useCallback((scope: PermissionScope, ruleId?: string) => {
    setSettingsFocus({ scope, ruleId });
    setSettingsOpen(true);
  }, []);
  const closeSettings = useCallback(() => {
    setSettingsOpen(false);
    setSettingsFocus(null);
  }, []);
  const reloadImported = useCallback(() => void reload(), [reload]);
  const updatePermissions = useCallback(
    (change: (p: PermissionSettings) => PermissionSettings) =>
      void updateSettings((s) => ({ ...s, permissions: change(s.permissions) })),
    [updateSettings],
  );
  const setProjectChecks = useCallback(
    (id: string, on: boolean) =>
      void updateSettings((s) => ({
        ...s,
        checks: { ...s.checks, projects: { ...s.checks.projects, [id]: on } },
      })),
    [updateSettings],
  );
  const [canvas, setCanvas] = useState<CanvasState | null>(null);
  const columnRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  // A project whose folder was added again; the sidebar flashes its row for as long as the CSS
  // animation runs.
  const [flashId, setFlashId] = useState<string | null>(null);
  useEffect(() => {
    if (!flashId) return;
    const timer = setTimeout(() => setFlashId(null), 900);
    return () => clearTimeout(timer);
  }, [flashId]);
  const [error, setError] = useState<string | null>(null);
  const { changes, branch, refreshGit } = useProjectGit(projectId, project?.path ?? null, setError);

  // The reply a canvas was opened from, as it is now, since it may still be streaming.
  const canvasMessageId = canvas?.message?.id;
  const liveMessage =
    canvasMessageId === undefined
      ? undefined
      : threads
          .find((t) => t.messages.some((m) => m.id === canvasMessageId))
          ?.messages.find((m) => m.id === canvasMessageId);
  // A canvas belongs to the project it was opened in; it is hidden after a project switch.
  const openCanvas = useMemo(
    () =>
      canvas && canvas.projectId === projectId
        ? { ...canvas, message: liveMessage ?? canvas.message }
        : null,
    [canvas, projectId, liveMessage],
  );
  const focusFiles = changeFocus
    ? (threads
        .find((t) => t.id === changeFocus.threadId)
        ?.messages.find((m) => m.id === changeFocus.messageId)?.files ?? null)
    : null;

  function attempt(fn: () => Promise<unknown>) {
    setError(null);
    fn().catch((e: unknown) => setError(errorText(e)));
  }

  // The reply the checks are for: the last one the open task got.
  const checkable =
    project && selected
      ? (selected.messages
          .filter((m) => m.role === "assistant" && m.status !== "streaming")
          .at(-1) ?? null)
      : null;
  const runChecks =
    checkable && selected ? () => attempt(() => api.runChecks(selected.id, checkable.id)) : null;

  /** A folder that is already a project flashes that project in the sidebar instead. */
  const addingFolders = (add: () => Promise<string[]>) =>
    attempt(async () => {
      const existing = await add();
      if (existing.length > 0) setFlashId(existing[existing.length - 1]);
    });
  const openFolder = () =>
    addingFolders(() => openFolderAsProject(settings.projects, updateSettings));
  const dropFolders = (paths: string[]) =>
    addingFolders(() => addFolders(paths, settings.projects, updateSettings));
  const renameProjectTo = (id: string, name: string) =>
    attempt(async () => {
      await api.renameProject(id, name);
      await syncProjects(updateSettings);
    });

  const { leavingIds, removeTargets, deleteArchivedMany } = useThreadRemoval({
    threads,
    folders,
    projects: settings.projects,
    selected,
    treeRowsRef,
    updateSettings,
    reload,
    removeFolder,
    setThreadArchived,
    deleteThread,
    selectThread: (id) => selectThread(id),
    attempt,
  });

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
    setSelectedRows([{ kind: "thread", id }]);
    select(id);
    const thread = threads.find((t) => t.id === id);
    if (thread) attempt(() => rememberProject(thread.projectId));
  };

  /** Opens the project and folder a thread sits in, so the sidebar shows it, then selects it. */
  const revealThread = (id: string) => {
    const thread = threads.find((t) => t.id === id);
    if (thread?.projectId) {
      const open = new Set(getCollapsed());
      open.delete(thread.projectId);
      if (thread.folderId) open.delete(`folder:${thread.folderId}`);
      setCollapsed(open);
    }
    selectThread(id);
    // The folder opens in the same commit; scroll once the row is in the tree.
    requestAnimationFrame(() =>
      document
        .querySelector(`[data-thread-id="${CSS.escape(id)}"]`)
        ?.scrollIntoView({ block: "nearest" }),
    );
  };

  const selectProject = (id: string) => {
    select(liveThreads.find((t) => t.projectId === id)?.id ?? null);
    attempt(() => rememberProject(id));
  };

  /** A new thread's agent, model, access, and effort: the ones last picked. */
  const newThreadOptions = () => ({
    agent: defaultAgent,
    model: settings.tokens.defaultModels[defaultAgent] ?? null,
    accessMode: lastAccessMode(),
    effort: lastEffort(defaultAgent),
    compactWindow: lastCompactWindow(defaultAgent),
  });

  /** New thread in a project, or a chat without a project when `id` is null. */
  const newThreadIn = (id: string | null) =>
    attempt(async () => {
      await createThread(id, newThreadOptions());
      await rememberProject(id);
    });
  const newChat = () => newThreadIn(null);

  /** Creates a task at the project's top level, then moves it to the top of `folderId`. */
  const newThreadInFolder = (projectId: string, folderId: string, threadIds: string[]) =>
    attempt(async () => {
      const thread = await createThread(projectId, newThreadOptions());
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

  useEffect(() => setRendererLogLevel(settings.logging.window), [settings.logging.window]);

  // What each shortcut does; which key runs it is set in lib/shortcutDefaults.ts and the Shortcuts
  // settings. The handlers read current state, so they need no re-registering. The panel's own
  // shortcuts are in PanelArea.
  useCommand("palette.toggle", () => setPaletteOpen((o) => !o));
  useCommand("chat.new", () => newThreadIn(projectId));
  useCommand("chat.newHere", newThreadHere);
  useCommand("settings.open", () => setSettingsOpen(true));
  useCommand("composer.focus", () =>
    document.querySelector<HTMLElement>(".composer-editor .ProseMirror")?.focus(),
  );
  // Closes the open sidebar, or reopens the last one, wherever focus sits after a click.
  useCommand("sidebar.toggle", () => setView((current) => (current ? null : lastView.current)));
  useCommand("sidebar.showTasks", () => setView("tasks"));
  useCommand("sidebar.showChanges", () => setView("changes"));
  useCommand("sidebar.showArchived", () => setView("archived"));
  // Acts on the picked rows and, while the compose box is not active, on the open chat in the task
  // list even when it is not picked.
  // Both wait while the next chat is still loading to open: the one on screen is the chat they just
  // archived, so a held key archived it again instead of moving on, and the run stopped.
  useCommand("workspace.closeChat", () => {
    if (opening()) return;
    const picked = getSelectedRows();
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
    if (opening()) return;
    const picked = getSelectedRows();
    if (picked.length > 0) void removePicked(picked);
    else if (view === "tasks" && selected)
      void removeTargets([{ kind: "thread", id: selected.id }]);
  });
  useCommand("palette.close", () => setPaletteOpen(false));
  useCommand("settings.close", closeSettings);
  useCommand("canvas.close", () => setCanvas(null));
  useCommand("sidebar.clearSelection", () => setSelectedRows([]));
  useContextKey("paletteOpen", () => paletteOpen);
  useContextKey("settingsOpen", () => settingsOpen);
  useContextKey("canvasOpen", () => openCanvas !== null);
  useContextKey("hasSelection", () => getSelectedRows().length > 0);

  const { rules } = useKeybindings();

  const sidebarHandlers = useStableHandlers({
    onRestoreThread: (id: string) => attempt(() => setThreadArchived(id, false)),
    onDeleteArchived: deleteArchivedMany,
    onClearChangeFocus: () => setChangeFocus(null),
    onNewTask: newThreadIn,
    onNewChat: newChat,
    onSelectThread: selectThread,
    onRenameThread: (id: string, title: string) => attempt(() => renameThread(id, title)),
    onRenameProject: renameProjectTo,
    onRelocateProject: (id: string) => attempt(() => relocateProject(id, updateSettings)),
    onRemoveTargets: removeTargets,
    onReorderProjects: reorderProjects,
    onReorderThreads: reorderThreadsTo,
    onNewFolder: newFolder,
    onSortByAgent: (projectId: string) => attempt(() => sortByAgent(projectId)),
    onUndoSortByAgent: (projectId: string) => attempt(() => undoSortByAgent(projectId)),
    onRenameFolder: (id: string, name: string) => attempt(() => renameFolder(id, name)),
    onNewTaskInFolder: newThreadInFolder,
    onOpenFolder: openFolder,
    onDropFolders: dropFolders,
    onRefreshChanges: () => attempt(refreshGit),
    onOpenChange: (path: string) =>
      openCanvasAt(path, null, changes.some((c) => c.path === path) ? "diff" : "code"),
    onOpenPalette: () => setPaletteOpen(true),
  });

  // The bars around the chat skip the renders streamed text causes.
  const barHandlers = useStableHandlers({
    selectProject,
    openFolder,
    selectView: (v: SidebarView) => setView(view === v ? null : v),
    openSettings: () => setSettingsOpen(true),
    openChanges: () => {
      setChangeFocus(null);
      setView("changes");
    },
  });

  return (
    <div className="grid h-full grid-rows-[var(--toolbar-h)_minmax(0,1fr)_var(--statusbar-h)] bg-app text-fg">
      <Toolbar
        project={project}
        projects={settings.projects}
        onSelectProject={barHandlers.selectProject}
        onOpenFolder={barHandlers.openFolder}
      />

      <div className="flex min-h-0 min-w-0" ref={bodyRef}>
        <ActivityBar
          view={view}
          onSelect={barHandlers.selectView}
          onOpenSettings={barHandlers.openSettings}
        />
        {view && (
          <Sidebar
            view={view}
            connectedAgents={connectedAgents}
            project={project}
            projects={settings.projects}
            threads={sidebarThreads.live}
            leavingIds={leavingIds}
            treeRowsRef={treeRowsRef}
            archivedThreads={sidebarThreads.archived}
            selectedId={selectedId}
            changes={changes}
            changeFocus={focusFiles}
            folders={folders}
            flashId={flashId}
            threadLogsMenu={settings.logging.threadMenu}
            {...sidebarHandlers}
          />
        )}

        <div className="flex min-h-0 min-w-0 flex-1 flex-col" ref={columnRef}>
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
                onShowChanges={(messageId) => {
                  if (selected) setChangeFocus({ threadId: selected.id, messageId });
                  setView("changes");
                }}
                onSend={(text, attachments, draft, queue, onThread) =>
                  send(text, attachments, draft, projectId, queue, onThread)
                }
                onNewChat={() => newThreadIn(projectId)}
                onChangeFolder={() => {
                  if (projectId) attempt(() => relocateProject(projectId, updateSettings));
                }}
                onClearThread={(id) =>
                  clearThread(id).catch((e: unknown) => setError(errorText(e)))
                }
                onRestoreCheckpoint={restoreCheckpoint}
                onForkThread={forkThread}
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
                onSetCompactWindow={setCompactWindow}
                permissionSettings={settings.permissions}
                confirmAnswers={settings.confirmAnswers}
                steerWhileWorking={settings.steerWhileWorking}
                projects={settings.projects}
                onUpdatePermissions={updatePermissions}
                onOpenPermissions={openPermissions}
                checkSettings={settings.checks}
                onSetProjectChecks={setProjectChecks}
              />
            </main>

            <CanvasArea
              canvas={openCanvas}
              bodyRef={bodyRef}
              changes={changes}
              onClose={() => setCanvas(null)}
              onChangesUpdated={() => void refreshGit()}
            />
          </div>

          <PanelArea
            columnRef={columnRef}
            settings={settings}
            updateSettings={updateSettings}
            cwd={project?.path ?? null}
          />
        </div>
      </div>

      <StatusBar
        project={project}
        branch={branch}
        threadTitle={selected ? displayTitle(selected.title) : null}
        changeCount={changes.length}
        agent={selected?.agent ?? defaultAgent}
        working={isBusy(selected)}
        done={
          selected?.messages.at(-1)?.status === "complete" &&
          selected.messages.at(-1)?.role === "assistant"
        }
        planMode={selected?.planMode ?? false}
        onOpenChanges={barHandlers.openChanges}
      />

      <ErrorToast error={error} onDismiss={() => setError(null)} />
      {paletteOpen && (
        <CommandPalette
          items={paletteItems({
            project,
            projects: settings.projects,
            threads: liveThreads,
            rules,
            newChat,
            newTask: newThreadIn,
            toggleTheme,
            openSettings: () => setSettingsOpen(true),
            openFolder,
            openThread: (id) => {
              setView("tasks");
              revealThread(id);
            },
            openProject: selectProject,
            runChecks,
          })}
          onClose={() => setPaletteOpen(false)}
        />
      )}
      {settingsOpen && (
        <SettingsDialog
          settings={settings}
          updateSettings={updateSettings}
          focus={settingsFocus}
          onImported={reloadImported}
          onClose={closeSettings}
        />
      )}
    </div>
  );
}
