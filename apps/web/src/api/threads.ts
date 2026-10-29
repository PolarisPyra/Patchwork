import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "~/api/invoke";
import type { AccessMode, AgentEventEnvelope, AgentKind, Thread } from "~/api/types";

export const threadsApi = {
  /** Every thread, each with only the messages the task list reads (see `Thread.partial`). */
  listThreads: () => invoke<Thread[]>("list_threads"),
  /** One thread with all its messages. */
  getThread: (id: string) => invoke<Thread>("get_thread", { id }),
  /** Puts threads in a folder (null: the top level) in the given order. */
  reorderThreads: (ids: string[], folderId: string | null) =>
    invoke<void>("reorder_threads", { ids, folderId }),
  /**
   * Creates a thread. `id` is chosen by the caller so the row can be drawn before this returns;
   * `planMode` is applied in the same call, so a new plan-mode thread is one round trip and not two.
   */
  createThread: (
    id: string,
    projectId: string | null,
    agent: AgentKind,
    model: string | null,
    accessMode: AccessMode,
    effort: string | null,
    planMode: boolean,
  ) =>
    invoke<Thread>("create_thread", {
      request: { id, projectId, agent, model, accessMode, effort, planMode },
    }),
  deleteThread: (id: string) => invoke<void>("delete_thread", { id }),
  /** Deletes a chat's messages and agent session; the chat stays, empty. */
  clearThread: (id: string) => invoke<void>("clear_thread", { id }),
  setThreadArchived: (id: string, archived: boolean) =>
    invoke<void>("set_thread_archived", { id, archived }),
  renameThread: (id: string, title: string) => invoke<void>("rename_thread", { id, title }),
  /** Sets agent + model; the agent can only change before the first message. */
  setThreadModel: (id: string, agent: AgentKind, model: string | null) =>
    invoke<void>("set_thread_model", { id, agent, model }),
  /** Takes effect from the thread's next turn. */
  setThreadAccessMode: (id: string, mode: AccessMode) =>
    invoke<void>("set_thread_access_mode", { id, mode }),
  setThreadPlanMode: (id: string, on: boolean) => invoke<void>("set_thread_plan_mode", { id, on }),
  setThreadEffort: (id: string, effort: string | null) =>
    invoke<void>("set_thread_effort", { id, effort }),
  setThreadRole: (id: string, role: string | null) => invoke<void>("set_thread_role", { id, role }),
  /** The user messages of a thread whose turn can be undone: the project was snapshotted before it. */
  checkpoints: (threadId: string) => invoke<string[]>("checkpoints", { threadId }),
  /** Puts the project's files back as they were before the message's turn, and removes that message
   * and the ones after it. Returns the thread as it is now. */
  restoreCheckpoint: (threadId: string, messageId: string) =>
    invoke<Thread>("restore_checkpoint", { threadId, messageId }),
  forkThread: (threadId: string, messageId: string) =>
    invoke<Thread>("fork_thread", { threadId, messageId }),
  /** The thread that was open when the app last closed. */
  getLastThread: () => invoke<string | null>("get_last_thread"),
  setLastThread: (id: string) => invoke<void>("set_last_thread", { id }),
};

export function onAgentEvent(cb: (e: AgentEventEnvelope) => void): Promise<UnlistenFn> {
  return listen<AgentEventEnvelope>("agent-event", (e) => cb(e.payload));
}
