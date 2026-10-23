import { useCallback, useEffect, useRef, useState } from "react";
import {
  AGENT_LABELS,
  api,
  onAgentEvent,
  type AccessMode,
  type AgentEvent,
  type AgentEventEnvelope,
  type AgentKind,
  type Attachment,
  type Folder,
  type Message,
  type Part,
  type StepStatus,
  type Thread,
} from "~/api";
import { profileMark } from "~/lib/profile";
import { AGENT_FOLDER_NAMES, AGENT_KINDS } from "~/lib/util";

export interface NewThreadOptions {
  agent: AgentKind;
  model: string | null;
  accessMode: AccessMode;
  effort: string | null;
  /** Start the thread in plan mode. */
  planMode?: boolean;
}

/** True while a turn is running, that is a reply is streaming. A goal is pursued inside turns,
 * so an active goal never outlasts the reply it runs in. */
export function isWorking(thread: Thread | null): boolean {
  if (!thread) return false;
  return thread.messages.some((m) => m.status === "streaming");
}

/** How often streamed events are applied to the threads, in milliseconds. */
const FLUSH_MS = 100;

/** Same rule as the backend: first line without a leading `/goal`, at most 60 characters. */
function titleFrom(text: string): string {
  const line = (text.split("\n")[0] ?? "").trim();
  const objective = /^\/goal(\s|$)/.test(line) ? line.slice("/goal".length).trim() : line;
  return Array.from(objective || line).slice(0, 60).join("");
}

/** Adds or replaces a message, keeping creation order. */
function upsertMessage(list: Message[], msg: Message): Message[] {
  const rest = list.filter((m) => m.id !== msg.id);
  const at = rest.findIndex((m) => m.createdAt >= msg.createdAt);
  return at === -1 ? [...rest, msg] : [...rest.slice(0, at), msg, ...rest.slice(at)];
}

/** Ends the reasoning block still streaming, if any, as the next step starts. */
function endThinking(parts: Part[]): Part[] {
  const last = parts.at(-1);
  if (last?.type !== "thinking" || last.endedAt !== null) return parts;
  return [...parts.slice(0, -1), { ...last, endedAt: Date.now() }];
}

function updateMessage(thread: Thread, messageId: string, fn: (m: Message) => Message): Thread {
  return { ...thread, messages: thread.messages.map((m) => (m.id === messageId ? fn(m) : m)) };
}

/** Applies one streamed agent event. Pure: returns a new thread object. */
export function applyAgentEvent(thread: Thread, event: AgentEvent): Thread {
  switch (event.type) {
    case "goalUpdated":
      return { ...thread, goal: event.goal };
    case "slashCommandsUpdated":
      return { ...thread, slashCommands: event.commands };
    case "usage": {
      const { messageId, usage } = event;
      return updateMessage(thread, messageId, (m) => ({ ...m, usage }));
    }
    case "messageStarted":
      return { ...thread, messages: upsertMessage(thread.messages, event.message) };
    // Parts follow the server's (orchestration/stream.rs `apply`).
    case "textDelta": {
      const { messageId, text } = event;
      return updateMessage(thread, messageId, (m) => {
        const parts = endThinking(m.parts);
        const last = parts.at(-1);
        return {
          ...m,
          text: m.text + text,
          parts:
            last?.type === "text"
              ? [...parts.slice(0, -1), { type: "text", text: last.text + text }]
              : [...parts, { type: "text", text }],
        };
      });
    }
    case "thinking": {
      const { messageId, text, startedAt, endedAt } = event;
      return updateMessage(thread, messageId, (m) => {
        const last = m.parts.at(-1);
        return {
          ...m,
          parts:
            last?.type === "thinking" && last.endedAt === null
              ? [...m.parts.slice(0, -1), { ...last, text: last.text + text }]
              : [
                  ...m.parts,
                  {
                    type: "thinking",
                    text,
                    startedAt: startedAt ?? Date.now(),
                    endedAt: endedAt ?? null,
                  },
                ],
        };
      });
    }
    case "toolStarted": {
      const { messageId, tool } = event;
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        tools: [...m.tools, tool],
        parts: [...endThinking(m.parts), { type: "tool", id: tool.id }],
      }));
    }
    case "toolFinished": {
      const { messageId, toolId, isError, output } = event;
      const status: StepStatus = isError ? "error" : "done";
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        tools: m.tools.map((t) =>
          t.id === toolId ? { ...t, status, output, endedAt: Date.now() } : t,
        ),
      }));
    }
    case "planUpdated": {
      const { messageId, steps } = event;
      return updateMessage(thread, messageId, (m) => ({ ...m, plan: steps }));
    }
    case "fileTouched": {
      const { messageId, path } = event;
      return updateMessage(thread, messageId, (m) =>
        m.files.includes(path) ? m : { ...m, files: [...m.files, path] },
      );
    }
    case "approvalRequested": {
      const { messageId, approval } = event;
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        approvals: [...m.approvals, approval],
      }));
    }
    case "approvalResolved": {
      const { messageId, approvalId, allowed, answers } = event;
      const status = allowed ? "allowed" : "denied";
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        approvals: m.approvals.map((a) => (a.id === approvalId ? { ...a, status, answers } : a)),
      }));
    }
    case "threadCreated":
      return thread;
    case "planProposed":
      return updateMessage(thread, event.messageId, (m) => ({
        ...m,
        parts: [...endThinking(m.parts), { type: "plan", text: event.text }],
      }));
    case "messageCutOff":
      return updateMessage(thread, event.messageId, (m) => ({ ...m, cutOff: true }));
    case "refused":
      return updateMessage(thread, event.messageId, (m) => ({ ...m, refusal: event.refusal }));
    case "messageQueued": {
      const { messageId, queued } = event;
      return updateMessage(thread, messageId, (m) => ({ ...m, queued }));
    }
    case "messageSteered":
      return updateMessage(thread, event.messageId, (m) => ({ ...m, queued: false, steer: true }));
    case "messageRemoved":
      return { ...thread, messages: thread.messages.filter((m) => m.id !== event.messageId) };
    case "messageEdited":
      return updateMessage(thread, event.message.id, () => event.message);
    case "backgroundTaskStarted": {
      const { messageId, task } = event;
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        background: [...m.background, task],
      }));
    }
    case "backgroundTaskEnded": {
      const { messageId, toolId } = event;
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        background: m.background.filter((t) => t.toolId !== toolId),
      }));
    }
    case "turnCompleted": {
      const { messageId, status, error } = event;
      // Nothing can still be running once the turn has ended (mirrors orchestration/stream.rs).
      const toolEnd = status === "complete" ? "done" : "error";
      const endedAt = Date.now();
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        status,
        error,
        finishedAt: endedAt,
        parts: endThinking(m.parts),
        tools: m.tools.map((t) =>
          t.status === "running" ? { ...t, status: toolEnd, endedAt } : t,
        ),
        plan: m.plan.map((s) => (s.status === "running" ? { ...s, status: "pending" } : s)),
        approvals: m.approvals.map((a) =>
          a.status === "pending" ? { ...a, status: "denied" } : a,
        ),
      }));
    }
  }
}

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
 * `onTurnCompleted` fires after any turn ends so the caller can refresh git state.
 */
export function useThreads(initialProjectId: string | null, onTurnCompleted: () => void) {
  const [threads, setThreads] = useState<Thread[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  /** Per project, its tasks and the folder each sat in, in order, before its last sort by agent. */
  const [sortUndo, setSortUndo] = useState<
    Record<string, { id: string; folderId: string | null }[]>
  >({});
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const turnDone = useRef(onTurnCompleted);
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

  useEffect(() => {
    turnDone.current = onTurnCompleted;
  });

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
    const flush = () => {
      const batch = queue;
      queue = [];
      timer = 0;
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
      if (batch.some(({ event }) => event.type === "turnCompleted")) turnDone.current();
    };
    void onAgentEvent((envelope) => {
      queue.push(envelope);
      timer ||= window.setTimeout(flush, FLUSH_MS);
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

  const selected = threads.find((t) => t.id === selectedId) ?? null;

  /** Creates a thread in `projectId` (null: a chat without a project) and selects it. */
  async function createThread(projectId: string | null, opts: NewThreadOptions): Promise<Thread> {
    let thread = await api.createThread(
      projectId,
      opts.agent,
      opts.model,
      opts.accessMode,
      opts.effort,
    );
    if (opts.planMode) {
      await api.setThreadPlanMode(thread.id, true);
      thread = { ...thread, planMode: true };
    }
    setThreads((prev) => [thread, ...prev]);
    setSelectedId(thread.id);
    return thread;
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
              // Mirrors the backend: the first message names the thread.
              title: t.messages.length === 0 ? titleFrom(text) : t.title,
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
  };
}
