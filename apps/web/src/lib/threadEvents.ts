import { type AgentEvent, type Message, type Part, type StepStatus, type Thread } from "~/api";

/**
 * How a thread changes as its agent streams: one event in, a new thread out. Pure and React-free,
 * so `useThreads` only batches the events and the rules read on their own.
 */

/** Adds or replaces a message, keeping creation order. */
export function upsertMessage(list: Message[], msg: Message): Message[] {
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

/** True while a turn is running, that is a reply is streaming. A goal is pursued inside turns,
 * so an active goal never outlasts the reply it runs in. */
export function isWorking(thread: Thread | null): boolean {
  if (!thread) return false;
  return thread.messages.some((m) => m.status === "streaming");
}

function updateMessage(thread: Thread, messageId: string, fn: (m: Message) => Message): Thread {
  return { ...thread, messages: thread.messages.map((m) => (m.id === messageId ? fn(m) : m)) };
}

/** Applies one streamed agent event. Pure: returns a new thread object. */
export function applyAgentEvent(thread: Thread, event: AgentEvent): Thread {
  switch (event.type) {
    case "goalUpdated":
      // An unmet check sends the agent back to work, which ends the check.
      return {
        ...thread,
        goal: event.goal,
        messages: thread.messages.map((m) => (m.checkingGoal ? { ...m, checkingGoal: false } : m)),
      };
    case "goalChecking":
      return updateMessage(thread, event.messageId, (m) => ({ ...m, checkingGoal: true }));
    case "compacting":
      return updateMessage(thread, event.messageId, (m) => ({ ...m, compacting: true }));
    case "retrying": {
      const { messageId, reason, attempt, maxAttempts } = event;
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        retry: { reason, attempt, maxAttempts },
      }));
    }
    case "slashCommandsUpdated":
      return { ...thread, slashCommands: event.commands };
    case "roleChanged":
      return { ...thread, role: event.role };
    case "modelChanged":
      return { ...thread, model: event.model };
    case "titleUpdated":
      return { ...thread, title: event.title };
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
          checkingGoal: false,
          compacting: false,
          retry: null,
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
          checkingGoal: false,
          compacting: false,
          retry: null,
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
        checkingGoal: false,
        compacting: false,
        retry: null,
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
        parts: [...endThinking(m.parts), { type: "approval", id: approval.id }],
        approvals: [...m.approvals, approval],
      }));
    }
    case "approvalResolved": {
      const { messageId, approvalId, allowed, answers, rule, auto } = event;
      const status = allowed ? "allowed" : "denied";
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        approvals: m.approvals.map((a) =>
          a.id === approvalId ? { ...a, status, answers, rule, auto } : a,
        ),
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
    case "backgroundTaskStarted":
    case "backgroundTaskUpdated":
    case "backgroundTaskEnded": {
      // A task is listed once, and each change replaces its record, which keeps its final status.
      const { messageId, task } = event;
      return updateMessage(thread, messageId, (m) => ({
        ...m,
        background: m.background.some((t) => t.toolId === task.toolId)
          ? m.background.map((t) => (t.toolId === task.toolId ? task : t))
          : [...m.background, task],
      }));
    }
    case "turnRecorded": {
      const { messageId, turn } = event;
      return updateMessage(thread, messageId, (m) => ({ ...m, turn }));
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
