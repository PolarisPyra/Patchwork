import { invoke } from "~/api/invoke";
import type { Attachment, Message, RuleRef } from "~/api/types";

export const turnsApi = {
  /** Returns the stored user message; assistant output streams via onAgentEvent. */
  sendMessage: (threadId: string, text: string, attachments: Attachment[], queue: boolean) =>
    invoke<Message>("send_message", { threadId, text, attachments, queue }),
  cancelTurn: (threadId: string) => invoke<void>("cancel_turn", { threadId }),
  /** Stops one running subagent (Claude only). */
  stopSubagent: (threadId: string, toolId: string) =>
    invoke<void>("stop_subagent", { threadId, toolId }),
  /** Takes back a message queued behind the running turn, so it can be edited. */
  removeQueued: (threadId: string, messageId: string) =>
    invoke<void>("remove_queued", { threadId, messageId }),
  editQueued: (threadId: string, messageId: string, text: string, attachments: Attachment[]) =>
    invoke<void>("edit_queued", { threadId, messageId, text, attachments }),
  resumeQueued: (threadId: string) => invoke<void>("resume_queued", { threadId }),
  steerQueued: (threadId: string, messageId: string) =>
    invoke<void>("steer_queued", { threadId, messageId }),
  /** Clears the goal, in the agent's session too. Refused while a turn runs. */
  stopGoal: (threadId: string) => invoke<void>("stop_goal", { threadId }),
  /** Runs the project's check command again for a reply; the result arrives as a `checksRecorded` event. */
  runChecks: (threadId: string, messageId: string) =>
    invoke<void>("run_checks", { threadId, messageId }),
  respondApproval: (
    threadId: string,
    approvalId: string,
    allow: boolean,
    answers?: Record<string, string[]>,
    rule?: RuleRef,
  ) => invoke<void>("respond_approval", { threadId, approvalId, allow, answers, rule }),
};
