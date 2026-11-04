import { useCallback, useMemo, useRef, useState } from "react";
import {
  api,
  type Approval,
  type PermissionScope,
  type PermissionSettings,
  type Project,
  type Thread,
} from "~/api";
import {
  type PermissionsContextValue,
  suggestedRules,
  threadScope,
  withScopeRules,
} from "~/lib/permissions";
import { errorText } from "~/lib/util";

/** Answers given so far to the questions of one request, by its approval's id. */
interface Answering {
  id: string;
  answers: Record<string, string[]>;
  /** The user is writing the answer to the shown question in the composer. */
  inComposer: boolean;
}

/**
 * The requests a thread's agent is waiting on. Questions wait in a choice panel, one at a time;
 * permission requests wait in the banner. Answers given so far are kept with the request they
 * belong to, so a new request never starts with an earlier one's.
 */
export function useApprovals({
  thread,
  project,
  permissionSettings,
  projects,
  onUpdatePermissions,
  onOpenPermissions,
  setErrorFor,
}: {
  thread: Thread | null;
  project: Project | null;
  permissionSettings: PermissionSettings;
  projects: Project[];
  onUpdatePermissions: (change: (p: PermissionSettings) => PermissionSettings) => void;
  onOpenPermissions: (scope: PermissionScope, ruleId?: string) => void;
  setErrorFor: (chat: string, text: string | null) => void;
}) {
  const threadId = thread?.id;
  // Requests answered and not yet resolved, so a double click answers once. The server turns a
  // second answer down, which would show as an error.
  const answering = useRef(new Set<string>());
  const respond = useCallback(
    (approvalId: string, allow: boolean, answers?: Record<string, string[]>) => {
      if (!threadId || answering.current.has(approvalId)) return;
      answering.current.add(approvalId);
      setErrorFor(threadId, null);
      api.respondApproval(threadId, approvalId, allow, answers).catch((e: unknown) => {
        answering.current.delete(approvalId);
        setErrorFor(threadId, errorText(e));
      });
    },
    [threadId, setErrorFor],
  );

  const scopeProjectId = thread ? thread.projectId : (project?.id ?? null);
  const scope = useMemo(() => threadScope(scopeProjectId), [scopeProjectId]);
  const permissionsContext = useMemo<PermissionsContextValue>(
    () => ({
      permissions: permissionSettings,
      projects,
      scope,
      addRules: (to, rules) =>
        onUpdatePermissions((p) =>
          withScopeRules(p, to, (s) => ({
            ...s,
            // A rule with the same pattern is replaced, so saving one twice keeps one.
            rules: [
              ...s.rules.filter(
                (r) => !rules.some((n) => n.kind === r.kind && n.pattern === r.pattern),
              ),
              ...rules,
            ],
          })),
        ),
      removeRule: (from, ruleId) =>
        onUpdatePermissions((p) =>
          withScopeRules(p, from, (s) => ({ ...s, rules: s.rules.filter((r) => r.id !== ruleId) })),
        ),
      openRule: onOpenPermissions,
    }),
    [permissionSettings, projects, scope, onUpdatePermissions, onOpenPermissions],
  );
  /** Answers a permission request; `always` also saves its rules there, so the next ones like
   * it are answered without asking. The rules are saved after the answer, so the server answers
   * the other waiting requests they cover rather than this one. */
  const respondPermission = useCallback(
    (approval: Approval, allow: boolean, always?: PermissionScope) => {
      if (!threadId || answering.current.has(approval.id)) return;
      answering.current.add(approval.id);
      setErrorFor(threadId, null);
      const rules = always ? suggestedRules(approval) : [];
      const rule = always && rules[0] ? { scope: always, ruleId: rules[0].id } : undefined;
      api
        .respondApproval(threadId, approval.id, allow, undefined, rule)
        .then(() => {
          if (always && rules.length > 0) permissionsContext.addRules(always, rules);
        })
        .catch((e: unknown) => {
          answering.current.delete(approval.id);
          setErrorFor(threadId, errorText(e));
        });
    },
    [threadId, permissionsContext, setErrorFor],
  );

  // A secret is entered in its own card in the reply.
  const pending =
    thread?.messages
      .flatMap((m) => m.approvals)
      .filter((a) => a.status === "pending" && !a.secret) ?? [];
  const asking = pending.find((a) => a.questions?.length);
  const permissions = pending.filter((a) => !a.questions?.length);
  const [held, setHeld] = useState<Answering | null>(null);
  const current = asking && held?.id === asking.id ? held : null;
  const answered = current?.answers ?? {};
  const question = asking?.questions?.find((q) => !(q.id in answered));
  const inComposer = current?.inComposer ?? false;

  // Records the answer to the shown question, and sends them all once the last is answered.
  function answerQuestion(answer: string) {
    if (!asking?.questions || !question) return;
    const answers = { ...answered, [question.id]: [answer] };
    if (asking.questions.every((q) => q.id in answers)) {
      setHeld(null);
      respond(asking.id, true, answers);
    } else {
      setHeld({ id: asking.id, answers, inComposer: false });
    }
  }

  return {
    scope,
    permissionsContext,
    permissions,
    respondPermission,
    /** The request whose questions the agent is waiting on. */
    asking,
    /** Its first question not answered yet. */
    question,
    /** The answer to `question` is being written in the composer, in place of the panel. */
    answerInComposer: inComposer,
    answerQuestion,
    writeOwnAnswer: () => asking && setHeld({ id: asking.id, answers: answered, inComposer: true }),
    dismissQuestion: () => asking && respond(asking.id, false),
  };
}
