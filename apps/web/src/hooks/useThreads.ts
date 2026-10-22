import { useCallback, useEffect, useRef, useState } from "react";
import {
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
    // Parts follow the server's (runner.rs `apply`).
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
      // Nothing can still be running once the turn has ended (mirrors runner.rs).
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

/**
 * Every thread (project tasks and project-less chats) plus the selection.
 * `select(null)` means no thread is selected (the composer then creates one on
 * send). `initialProjectId` picks the default selection after loading.
 * `onTurnCompleted` fires after any turn ends so the caller can refresh git state.
 */
export function useThreads(initialProjectId: string | null, onTurnCompleted: () => void) {
  const [threads, setThreads] = useState<Thread[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const turnDone = useRef(onTurnCompleted);
  const initialProject = useRef(initialProjectId);

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
      ([list, folderList, last]) => {
        if (cancelled) return;
        setThreads(list);
        setFolders(folderList);
        const live = list.filter((t) => t.archivedAt === null);
        const fallback = live.find((t) => t.projectId === initialProject.current)?.id ?? null;
        setSelectedId(live.find((t) => t.id === last)?.id ?? fallback);
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
        setThreads(list);
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
    await api.renameFolder(id, name);
    setFolders((prev) => prev.map((f) => (f.id === id ? { ...f, name } : f)));
  }

  async function removeFolder(id: string): Promise<void> {
    await api.removeFolder(id);
    setFolders((prev) => prev.filter((f) => f.id !== id));
    setThreads(await api.listThreads());
  }

  /** Puts threads in a folder (null: the top level) in `ids` order, then re-reads the order. */
  async function reorderThreads(ids: string[], folderId: string | null): Promise<void> {
    await api.reorderThreads(ids, folderId);
    setThreads(await api.listThreads());
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
    select: setSelectedId,
    createThread,
    deleteThread,
    setThreadArchived,
    renameThread,
    clearThread,
    createFolder,
    renameFolder,
    removeFolder,
    reorderThreads,
    setModel,
    setAccessMode,
    setPlanMode,
    setEffort,
    send,
    startThread,
  };
}
