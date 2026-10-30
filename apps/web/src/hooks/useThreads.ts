import { useCallback, useEffect, useRef, useState } from "react";
import {
  AGENT_LABELS,
  api,
  onAgentEvent,
  type AccessMode,
  type AgentEventEnvelope,
  type AgentKind,
  type Attachment,
  type Folder,
  type Thread,
} from "~/api";
import { profileMark } from "~/lib/profile";
import { applyAgentEvent, upsertMessage } from "~/lib/threadEvents";
import { AGENT_FOLDER_NAMES, AGENT_KINDS } from "~/lib/util";

/** A fresh thread id. `randomUUID` needs a secure context, which the app's webview is; the
 * fallback keeps a plain-HTTP dev page working. */
function newId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === "x" ? r : (r & 0x3) | 0x8).toString(16);
  });
}

export interface NewThreadOptions {
  agent: AgentKind;
  model: string | null;
  accessMode: AccessMode;
  effort: string | null;
  /** The context size to compact at; null follows Settings → Tokens. */
  compactWindow: number | null;
  /** Start the thread in plan mode. */
  planMode?: boolean;
}

/** How often streamed events are applied to the threads, in milliseconds. */
const FLUSH_MS = 100;
/** How often streamed text for threads that are not open is applied. Nothing on screen shows it,
 * and each flush re-renders the workspace. */
const BACKGROUND_FLUSH_MS = 1000;

/** Takes a fresh `listThreads` result, keeping the messages of threads already loaded in full:
 * events have kept those current, and the list carries only a few of them. */
function mergeListed(prev: Thread[], list: Thread[]): Thread[] {
  const loaded = new Map(prev.filter((t) => !t.partial).map((t) => [t.id, t]));
  return list.map((t) => {
    const old = loaded.get(t.id);
    return old ? { ...t, messages: old.messages, partial: false } : { ...t, partial: true };
  });
}

/** Fills a listed thread with its full history. Messages it already has are newer than the
 * saved ones (streamed text is saved in batches), so they are kept. */
function mergeFull(current: Thread, full: Thread): Thread {
  const known = new Map(current.messages.map((m) => [m.id, m]));
  const saved = new Set(full.messages.map((m) => m.id));
  return {
    ...current,
    messages: [
      ...full.messages.map((m) => known.get(m.id) ?? m),
      ...current.messages.filter((m) => !saved.has(m.id)),
    ],
    partial: false,
  };
}

/**
 * Every thread (project tasks and project-less chats) plus the selection.
 * `select(null)` means no thread is selected (the composer then creates one on
 * send). `initialProjectId` picks the default selection after loading.
 */
export function useThreads(initialProjectId: string | null) {
  const [threads, setThreads] = useState<Thread[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  /** Per project, its tasks and the folder each sat in, in order, before its last sort by agent. */
  const [sortUndo, setSortUndo] = useState<
    Record<string, { id: string; folderId: string | null }[]>
  >({});
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selectedRef = useRef(selectedId);
  selectedRef.current = selectedId;
  /** Applies the streamed events still waiting, e.g. a background thread's as it opens. */
  const flushRef = useRef(() => {});
  const initialProject = useRef(initialProjectId);
  const threadsRef = useRef(threads);
  threadsRef.current = threads;
  /** The thread last asked to open; a slower load for an earlier pick does not open it. */
  const wantedId = useRef<string | null>(null);

  /** Loads a listed thread's full history into the list. */
  const loadThread = useCallback(async (id: string) => {
    const full = await api.getThread(id);
    setThreads((prev) => prev.map((t) => (t.id === id && t.partial ? mergeFull(t, full) : t)));
  }, []);

  /** Opens a thread, once its full history is loaded, so it never shows with part of it. */
  const select = useCallback(
    (id: string | null) => {
      wantedId.current = id;
      if (!id || !threadsRef.current.find((t) => t.id === id)?.partial) {
        setSelectedId(id);
        return;
      }
      // If loading fails, the thread opens with the messages it has.
      void loadThread(id)
        .catch(() => undefined)
        .then(() => {
          if (wantedId.current === id) setSelectedId(id);
        });
    },
    [loadThread],
  );

  // Remember the open thread, so the next launch returns to it. A failed write only
  // means the next launch opens the newest task instead.
  useEffect(() => {
    if (selectedId) void api.setLastThread(selectedId).catch(() => undefined);
  }, [selectedId]);

  // Load once; reopen the thread last open if it still exists and is not archived, else the
  // newest thread of the initial project (or chat).
  useEffect(() => {
    let cancelled = false;
    Promise.all([api.listThreads(), api.listFolders(), api.getLastThread()]).then(
      async ([list, folderList, last]) => {
        // Follow-up queues are tied to the current app session. Discard any unsent messages
        // restored from storage so a refresh cannot leave an old test prompt above the composer.
        let restored = list;
        for (let attempt = 0; attempt < 2; attempt += 1) {
          const queued = restored.flatMap((thread) =>
            thread.messages
              .filter((message) => message.queued)
              .map((message) => ({ threadId: thread.id, messageId: message.id })),
          );
          if (queued.length === 0) break;
          await Promise.all(
            queued.map(({ threadId, messageId }) =>
              api.removeQueued(threadId, messageId).catch(() => undefined),
            ),
          );
          // Use a fresh snapshot: a turn can finish while cleanup is running and change
          // queued flags after the original list was read.
          try {
            restored = await api.listThreads();
          } catch {
            break;
          }
        }
        if (cancelled) return;
        setThreads((prev) => mergeListed(prev, restored));
        profileMark("boot:threads-set");
        setFolders(folderList);
        const live = restored.filter((t) => t.archivedAt === null);
        const fallback = live.find((t) => t.projectId === initialProject.current)?.id ?? null;
        const open = live.find((t) => t.id === last)?.id ?? fallback;
        if (open) await loadThread(open).catch(() => undefined);
        if (!cancelled && wantedId.current === null) setSelectedId(open);
      },
      () => {
        if (!cancelled) setThreads([]);
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  /** Re-reads every thread and folder from the backend, e.g. after importing. */
  const reload = useCallback(
    () =>
      Promise.all([api.listThreads(), api.listFolders()]).then(([list, folderList]) => {
        setThreads((prev) => mergeListed(prev, list));
        setFolders(folderList);
      }),
    [],
  );

  // Subscribe once for the app's lifetime; events for unknown threads are ignored. Text arrives
  // token by token, so events are applied in batches every FLUSH_MS: the webview paints in
  // software on some machines, and a re-render per frame kept the CPU busy for a whole reply.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let queue: AgentEventEnvelope[] = [];
    let timer = 0;
    let urgent = false;
    const flush = () => {
      window.clearTimeout(timer);
      timer = 0;
      urgent = false;
      if (queue.length === 0) return;
      const batch = queue;
      queue = [];
      setThreads((prev) =>
        batch.reduce((threads, { threadId, event }) => {
          // A thread the server made, such as a subagent's, joins the list.
          if (event.type === "threadCreated") {
            return threads.some((t) => t.id === threadId) ? threads : [...threads, event.thread];
          }
          return threads.some((t) => t.id === threadId)
            ? threads.map((t) => (t.id === threadId ? applyAgentEvent(t, event) : t))
            : threads;
        }, prev),
      );
    };
    flushRef.current = flush;
    void onAgentEvent((envelope) => {
      queue.push(envelope);
      // A title arrives once per thread, so it shows at once rather than with the next batch.
      if (envelope.event.type === "titleUpdated") {
        flush();
        return;
      }
      // Text and reasoning for a thread that is not open wait for the slower flush; anything
      // else, or anything for the open thread, brings the whole queue forward, in order.
      const { type } = envelope.event;
      const quiet =
        (type === "textDelta" || type === "thinking") && envelope.threadId !== selectedRef.current;
      if (!quiet && !urgent) {
        urgent = true;
        window.clearTimeout(timer);
        timer = window.setTimeout(flush, FLUSH_MS);
      }
      timer ||= window.setTimeout(flush, BACKGROUND_FLUSH_MS);
    }).then((u) => {
      if (disposed) u();
      else unlisten = u;
    });
    return () => {
      disposed = true;
      unlisten?.();
      window.clearTimeout(timer);
    };
  }, []);

  // A thread that opens shows the text it streamed in the background straight away.
  useEffect(() => flushRef.current(), [selectedId]);

  const selected = threads.find((t) => t.id === selectedId) ?? null;

  /** Creates a thread in `projectId` (null: a chat without a project) and selects it. */
  /**
   * Starts a thread. The row is drawn on the click, from a thread this builds itself, and the
   * backend is asked in the background; the row is replaced by the saved thread when it answers,
   * and taken away again if the backend refuses. Waiting for the round trip first left the
   * sidebar empty for as long as the call took, which is what the "+" button felt like.
   *
   * The id is ours for the same reason: the row needs one before the call returns, and the
   * backend uses the id it is given.
   */
  async function createThread(projectId: string | null, opts: NewThreadOptions): Promise<Thread> {
    const id = newId();
    const draft: Thread = {
      id,
      projectId,
      title: projectId === null ? "New chat" : "New task",
      agent: opts.agent,
      model: opts.model,
      accessMode: opts.accessMode,
      effort: opts.effort,
      compactWindow: opts.compactWindow,
      sessionId: null,
      createdAt: Date.now(),
      messages: [],
      goal: null,
      folderId: null,
      archivedAt: null,
      parent: null,
      planMode: opts.planMode ?? false,
      slashCommands: [],
      importedFrom: null,
      contextPending: false,
      role: null,
    };
    setThreads((prev) => [draft, ...prev]);
    setSelectedId(id);
    try {
      const thread = await api.createThread(
        id,
        projectId,
        opts.agent,
        opts.model,
        opts.accessMode,
        opts.effort,
        opts.compactWindow,
        draft.planMode,
      );
      setThreads((prev) =>
        prev.map((t) => (t.id === id ? { ...thread, messages: t.messages } : t)),
      );
      return thread;
    } catch (e) {
      // The row was never saved, so it goes, and the error reaches the caller.
      setThreads((prev) => prev.filter((t) => t.id !== id));
      setSelectedId((current) => (current === id ? null : current));
      throw e;
    }
  }

  async function deleteThread(id: string): Promise<void> {
    await api.deleteThread(id);
    setThreads((prev) => prev.filter((t) => t.id !== id));
    setSelectedId((cur) => (cur === id ? null : cur));
  }

  /** Moves a thread to the archive, or back to the task list. Archiving the selected thread clears the selection. */
  async function setThreadArchived(id: string, archived: boolean): Promise<void> {
    await api.setThreadArchived(id, archived);
    setThreads((prev) =>
      prev.map((t) => (t.id === id ? { ...t, archivedAt: archived ? Date.now() : null } : t)),
    );
    if (archived) setSelectedId((cur) => (cur === id ? null : cur));
  }

  /** Empties a chat: its messages and agent session are deleted, the chat itself stays. */
  async function clearThread(id: string): Promise<void> {
    await api.clearThread(id);
    setThreads((prev) =>
      prev.map((t) => (t.id === id ? { ...t, messages: [], sessionId: null, goal: null } : t)),
    );
  }

  /** Undoes a user message's turn and the ones after it; the thread comes back as it is now. */
  async function restoreCheckpoint(threadId: string, messageId: string): Promise<void> {
    const restored = await api.restoreCheckpoint(threadId, messageId);
    setThreads((prev) => prev.map((t) => (t.id === threadId ? restored : t)));
  }

  /** Starts a new thread with the conversation up to and including a message, and selects it. */
  async function forkThread(threadId: string, messageId: string): Promise<void> {
    const thread = await api.forkThread(threadId, messageId);
    setThreads((prev) => [thread, ...prev]);
    setSelectedId(thread.id);
  }

  async function renameThread(id: string, title: string): Promise<void> {
    await api.renameThread(id, title);
    setThreads((prev) => prev.map((t) => (t.id === id ? { ...t, title } : t)));
  }

  async function createFolder(projectId: string): Promise<Folder> {
    const folder = await api.createFolder(projectId, "New folder");
    setFolders((prev) => [...prev, folder]);
    return folder;
  }

  async function renameFolder(id: string, name: string): Promise<void> {
    // Sorting by provider and undoing it treat folders with these names as their own.
    const reserved = [...AGENT_FOLDER_NAMES].find((n) => n.toLowerCase() === name.toLowerCase());
    if (reserved) {
      throw new Error(
        `"${reserved}" is reserved for sorting tasks by provider; undoing that sort would empty and remove this folder. Pick another name.`,
      );
    }
    await api.renameFolder(id, name);
    setFolders((prev) => prev.map((f) => (f.id === id ? { ...f, name } : f)));
  }

  async function removeFolder(id: string): Promise<void> {
    await api.removeFolder(id);
    setFolders((prev) => prev.filter((f) => f.id !== id));
    const list = await api.listThreads();
    setThreads((prev) => mergeListed(prev, list));
  }

  /** Puts threads in a folder (null: the top level) in `ids` order, then re-reads the order. */
  async function reorderThreads(ids: string[], folderId: string | null): Promise<void> {
    await api.reorderThreads(ids, folderId);
    const list = await api.listThreads();
    setThreads((prev) => mergeListed(prev, list));
  }

  /** Moves a project's tasks into one folder per agent, named for it, creating those folders as needed. */
  async function sortByAgent(projectId: string): Promise<void> {
    const tasks = threads.filter(
      (t) => t.projectId === projectId && t.archivedAt === null && !t.parent,
    );
    try {
      for (const agent of AGENT_KINDS) {
        const ids = tasks.filter((t) => t.agent === agent).map((t) => t.id);
        if (ids.length === 0) continue;
        const name = AGENT_LABELS[agent];
        let folder = folders.find((f) => f.projectId === projectId && f.name === name);
        if (!folder) {
          const created = await api.createFolder(projectId, name);
          setFolders((prev) => [...prev, created]);
          folder = created;
        }
        await api.reorderThreads(ids, folder.id);
      }
    } finally {
      const before = tasks.map((t) => ({ id: t.id, folderId: t.folderId }));
      setSortUndo((prev) => ({ ...prev, [projectId]: before }));
      const list = await api.listThreads();
      setThreads((prev) => mergeListed(prev, list));
    }
  }

  /** Takes a project's tasks out of the folders named for agents, back to the folder each was in before
   * the last sort when that is known and still there, else to the top level; then removes those
   * folders once empty. */
  async function undoSortByAgent(projectId: string): Promise<void> {
    const agentFolderIds = new Set(
      folders
        .filter((f) => f.projectId === projectId && AGENT_FOLDER_NAMES.has(f.name))
        .map((f) => f.id),
    );
    const before = sortUndo[projectId] ?? [];
    const priorFolder = new Map(before.map((b) => [b.id, b.folderId]));
    const rank = new Map(before.map((b, i) => [b.id, i]));
    const destination = (t: Thread): string | null => {
      if (t.folderId === null || !agentFolderIds.has(t.folderId)) return t.folderId;
      const prior = priorFolder.get(t.id) ?? null;
      return prior !== null && !agentFolderIds.has(prior) && folders.some((f) => f.id === prior)
        ? prior
        : null;
    };
    const lists = new Map<string | null, Thread[]>();
    for (const t of threads) {
      if (t.projectId !== projectId || t.archivedAt !== null || t.parent) continue;
      const to = destination(t);
      lists.set(to, [...(lists.get(to) ?? []), t]);
    }
    setSortUndo(({ [projectId]: _, ...rest }) => rest);
    try {
      for (const [folderId, list] of lists) {
        if (list.every((t) => t.folderId === folderId)) continue;
        // Tasks the sort moved go back to their old order; others keep theirs after them.
        const order = (t: Thread) => rank.get(t.id) ?? before.length;
        const ids = [...list].sort((x, y) => order(x) - order(y)).map((t) => t.id);
        await api.reorderThreads(ids, folderId);
      }
      const all = await api.listThreads();
      for (const id of agentFolderIds) {
        if (!all.some((t) => t.folderId === id)) await api.removeFolder(id);
      }
    } finally {
      setFolders(await api.listFolders());
      const list = await api.listThreads();
      setThreads((prev) => mergeListed(prev, list));
    }
  }

  async function setModel(id: string, agent: AgentKind, model: string | null): Promise<void> {
    await api.setThreadModel(id, agent, model);
    setThreads((prev) => prev.map((t) => (t.id === id ? { ...t, agent, model } : t)));
  }

  async function setAccessMode(id: string, accessMode: AccessMode): Promise<void> {
    await api.setThreadAccessMode(id, accessMode);
    setThreads((prev) => prev.map((t) => (t.id === id ? { ...t, accessMode } : t)));
  }

  async function setPlanMode(id: string, planMode: boolean): Promise<void> {
    await api.setThreadPlanMode(id, planMode);
    setThreads((prev) => prev.map((t) => (t.id === id ? { ...t, planMode } : t)));
  }

  async function setEffort(id: string, effort: string | null): Promise<void> {
    await api.setThreadEffort(id, effort);
    setThreads((prev) => prev.map((t) => (t.id === id ? { ...t, effort } : t)));
  }

  async function setCompactWindow(id: string, compactWindow: number | null): Promise<void> {
    await api.setThreadCompactWindow(id, compactWindow);
    setThreads((prev) => prev.map((t) => (t.id === id ? { ...t, compactWindow } : t)));
  }

  /** Sends a message, creating a thread in `projectId` first when none is selected. */
  async function send(
    text: string,
    attachments: Attachment[],
    draft: NewThreadOptions,
    projectId: string | null,
    queue: boolean,
  ): Promise<void> {
    const thread = selected ?? (await createThread(projectId, draft));
    const message = await api.sendMessage(thread.id, text, attachments, queue);
    setThreads((prev) =>
      prev.map((t) =>
        t.id !== thread.id
          ? t
          : {
              ...t,
              messages: upsertMessage(t.messages, message),
            },
      ),
    );
  }

  /** Starts a thread in `projectId` with `text` as its first message, named `title`, and selects it. */
  async function startThread(
    projectId: string | null,
    opts: NewThreadOptions,
    title: string,
    text: string,
  ): Promise<void> {
    const thread = await createThread(projectId, opts);
    const message = await api.sendMessage(thread.id, text, [], false);
    await api.renameThread(thread.id, title);
    setThreads((prev) =>
      prev.map((t) =>
        t.id === thread.id ? { ...t, title, messages: upsertMessage(t.messages, message) } : t,
      ),
    );
  }

  return {
    threads,
    folders,
    reload,
    selected,
    select,
    createThread,
    deleteThread,
    setThreadArchived,
    renameThread,
    clearThread,
    restoreCheckpoint,
    forkThread,
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
  };
}
