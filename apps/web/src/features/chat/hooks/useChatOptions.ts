import { useState } from "react";
import { api, type AccessMode, type AgentKind, type Thread, type TokenSettings } from "~/api";
import { lastAccessMode } from "~/components/AccessPicker";
import { lastCompactWindow } from "~/components/CompactPicker";
import { effortFor, lastEffort } from "~/components/EffortPicker";
import type { NewThreadOptions } from "~/hooks/useThreads";
import { errorText } from "~/lib/util";

/**
 * The agent, model, access, effort, compact window, plan mode and role a chat runs with. An open
 * thread keeps its own, and a pick changes it on the server; the new-task screen keeps its picks
 * here until a send starts the thread with them.
 */
export function useChatOptions({
  thread,
  defaultAgent,
  tokens,
  setError,
  onSetModel,
  onSetAccessMode,
  onSetPlanMode,
  onSetEffort,
  onSetCompactWindow,
}: {
  thread: Thread | null;
  defaultAgent: AgentKind;
  tokens: TokenSettings;
  setError: (text: string | null) => void;
  onSetModel: (threadId: string, agent: AgentKind, model: string | null) => Promise<void>;
  onSetAccessMode: (threadId: string, mode: AccessMode) => Promise<void>;
  onSetPlanMode: (threadId: string, on: boolean) => Promise<void>;
  onSetEffort: (threadId: string, effort: string | null) => Promise<void>;
  onSetCompactWindow: (threadId: string, window: number | null) => Promise<void>;
}) {
  // Agent/model/access chosen for the next thread; null means the defaults.
  const [pick, setPick] = useState<NewThreadOptions | null>(null);
  const newOpts: NewThreadOptions = pick ?? {
    agent: defaultAgent,
    model: tokens.defaultModels[defaultAgent] ?? null,
    accessMode: lastAccessMode(),
    effort: lastEffort(defaultAgent),
    compactWindow: lastCompactWindow(defaultAgent),
  };
  const agent = thread?.agent ?? newOpts.agent;

  // Sets an option on the open thread, or picks it for the next one.
  function choose(next: Partial<NewThreadOptions>, onThread: (threadId: string) => Promise<void>) {
    if (!thread) {
      setPick({ ...newOpts, ...next });
      return;
    }
    setError(null);
    onThread(thread.id).catch((e: unknown) => setError(errorText(e)));
  }

  function pickEffort(next: string | null) {
    choose({ effort: next }, (id) => onSetEffort(id, next));
  }

  function pickModel(nextAgent: AgentKind, nextModel: string | null) {
    const same = nextAgent === agent;
    choose(
      {
        agent: nextAgent,
        model: nextModel,
        effort: same ? newOpts.effort : lastEffort(nextAgent),
        compactWindow: same ? newOpts.compactWindow : lastCompactWindow(nextAgent),
      },
      (id) => onSetModel(id, nextAgent, nextModel),
    );
    // A level the new agent does not accept would make its CLI refuse the turn.
    if (thread && thread.effort !== effortFor(nextAgent, thread.effort)) pickEffort(null);
  }

  return {
    newOpts,
    agent,
    model: thread ? thread.model : newOpts.model,
    accessMode: thread?.accessMode ?? newOpts.accessMode,
    effort: thread ? thread.effort : newOpts.effort,
    compactWindow: thread ? thread.compactWindow : newOpts.compactWindow,
    role: thread ? thread.role : (newOpts.role ?? null),
    planSupported: agent !== "antigravity",
    pickModel,
    pickEffort,
    pickCompactWindow: (next: number | null) =>
      choose({ compactWindow: next }, (id) => onSetCompactWindow(id, next)),
    pickAccess: (mode: AccessMode) =>
      choose({ accessMode: mode }, (id) => onSetAccessMode(id, mode)),
    setPlanMode: (enabled: boolean) =>
      choose({ planMode: enabled }, (id) => onSetPlanMode(id, enabled)),
    pickRole: (next: string | null) => choose({ role: next }, (id) => api.setThreadRole(id, next)),
    // The role went into the chat that was started; the next new chat starts with none.
    dropPickedRole: () => setPick((picked) => (picked ? { ...picked, role: null } : picked)),
  };
}
