import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FolderPlusIcon, ListChecksIcon } from "@phosphor-icons/react";
import {
  api,
  type AccessMode,
  type AgentKind,
  type Attachment,
  type BackgroundTask,
  type FileChange,
  type Approval,
  type Message,
  type PermissionScope,
  type PermissionSettings,
  type Project,
  type ProviderSlashCommand,
  type Thread,
  type TokenSettings,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import type { CanvasTab } from "~/features/canvas/Canvas";
import { lastAccessMode } from "~/components/AccessPicker";
import { COMPACT_SETTINGS, lastCompactWindow } from "~/components/CompactPicker";
import { effortFor, lastEffort } from "~/components/EffortPicker";
import { confirm } from "@tauri-apps/plugin-dialog";
import ChoicePanel from "~/features/chat/prompts/ChoicePanel";
import GoalBanner from "~/features/chat/prompts/GoalBanner";
import {
  isCompactCommand,
  isGoalClearCommand,
  localCommand,
  type LocalCommand,
} from "~/lib/composerActions";
import { ComposerStripPresence } from "~/features/chat/ComposerStrip";
import BackgroundTasks from "~/features/chat/BackgroundTasks";
import QueuedMessages from "~/features/chat/composer/QueuedMessages";
import LineageCard from "~/features/chat/lineage/LineageCard";
import Composer from "~/features/chat/composer/Composer";
import type { HistoryEntry } from "~/features/chat/composer/types";
import Transcript from "~/features/chat/transcript/Transcript";
import { UsageLine } from "~/features/chat/transcript/components/UsageLine";
import PendingApprovalBanner from "~/features/chat/prompts/PendingApprovalBanner";
import {
  PermissionsContext,
  type PermissionsContextValue,
  scopeName,
  suggestedRules,
  threadScope,
  withScopeRules,
} from "~/lib/permissions";
import PlanPanel from "~/features/chat/plan/PlanPanel";
import { planTitle } from "~/features/chat/plan/ProposedPlanCard";
import MissingFolderPrompt from "~/features/chat/prompts/MissingFolderPrompt";
import ResumePrompt from "~/features/chat/prompts/ResumePrompt";
import { useCheckpoints } from "~/features/chat/hooks/useCheckpoints";
import { useChoices } from "~/features/chat/hooks/useChoices";
import { useComposerDraft } from "~/features/chat/composer/useComposerDraft";
import { useGoalClear } from "~/features/chat/hooks/useGoalClear";
import { useStableHandlers } from "~/hooks/useStableHandlers";
import type { NewThreadOptions } from "~/hooks/useThreads";
import { isWorking } from "~/lib/threadEvents";
import { coldLargeSession, threadUsage } from "~/lib/tokens";
import { displayTitle, errorText } from "~/lib/util";

const SUGGESTIONS = [
  { label: "Explain this codebase", prefill: "Explain this codebase" },
  { label: "Find and fix a bug", prefill: "Find and fix a bug" },
  { label: "Write tests for…", prefill: "Write tests for " },
];

const CHAT_SUGGESTIONS = [
  { label: "Explain a concept", prefill: "Explain " },
  { label: "Draft a script", prefill: "Write a script that " },
  { label: "Review a snippet", prefill: "Review this code:\n" },
];

interface ChatProps {
  project: Project | null;
  thread: Thread | null;
  /** The thread that started this one, when it is a subagent's. */
  parentThread: Thread | null;
  /** Opens another thread, such as a subagent's. */
  onOpenThread: (id: string) => void;
  changes: FileChange[];
  defaultAgent: AgentKind;
  /** Connected agents offered in the composer's agent picker. */
  agents: AgentKind[];
  tokens: TokenSettings;
  onUpdateTokens: (patch: Partial<TokenSettings>) => void;
  onOpenFolder: () => void;
  onOpenCanvas: (path: string, message: Message | null, tab: CanvasTab) => void;
  onShowChanges: (messageId: string) => void;
  onSend: (
    text: string,
    attachments: Attachment[],
    draft: NewThreadOptions,
    queue: boolean,
  ) => Promise<void>;
  onCancel: (threadId: string) => Promise<void>;
  onSetModel: (threadId: string, agent: AgentKind, model: string | null) => Promise<void>;
  onSetAccessMode: (threadId: string, mode: AccessMode) => Promise<void>;
  onSetPlanMode: (threadId: string, on: boolean) => Promise<void>;
  /** Starts a thread in this project with `text` as its first message, named `title`. */
  onStartThread: (opts: NewThreadOptions, title: string, text: string) => Promise<void>;
  onSetEffort: (threadId: string, effort: string | null) => Promise<void>;
  onSetCompactWindow: (threadId: string, window: number | null) => Promise<void>;
  /** Starts a fresh chat in this project; the current chat stays in the sidebar. */
  onNewChat: () => void;
  /** Picks another folder for the project, after it was moved or deleted. */
  onChangeFolder: () => void;
  /** Deletes the chat's messages and agent session; the chat stays. */
  onClearThread: (threadId: string) => Promise<void>;
  /** Undoes the turn of a user message and everything after it, the project's files included. */
  onRestoreCheckpoint: (threadId: string, messageId: string) => Promise<void>;
  /** Starts a new thread with the conversation up to and including a reply. */
  onForkThread: (threadId: string, messageId: string) => Promise<void>;
  /** Settings → Permissions, for the approval banner and the transcript. */
  permissionSettings: PermissionSettings;
  /** Answers to the agent's questions take a second click. */
  confirmAnswers: boolean;
  projects: Project[];
  onUpdatePermissions: (change: (p: PermissionSettings) => PermissionSettings) => void;
  /** Opens Settings → Permissions on a scope, with one of its rules picked out. */
  onOpenPermissions: (scope: PermissionScope, ruleId?: string) => void;
}

// A fallback that keeps one identity, so the memoized composer sees no change.
const NO_COMMANDS: ProviderSlashCommand[] = [];

export default function Chat({
  project,
  onChangeFolder,
  thread,
  parentThread,
  onOpenThread,
  changes,
  defaultAgent,
  agents,
  tokens,
  onUpdateTokens,
  onOpenFolder,
  onOpenCanvas,
  onShowChanges,
  onSend,
  onNewChat,
  onClearThread,
  onRestoreCheckpoint,
  onForkThread,
  onCancel,
  onSetModel,
  onSetAccessMode,
  onSetPlanMode,
  onStartThread,
  onSetEffort,
  onSetCompactWindow,
  permissionSettings,
  confirmAnswers,
  projects,
  onUpdatePermissions,
  onOpenPermissions,
}: ChatProps) {
  // Errors by the chat they happened in ("new" for the new-task screen), so one never shows in
  // another chat.
  const [errors, setErrors] = useState<Record<string, string | null>>({});
  const setErrorFor = useCallback(
    (chat: string, text: string | null) => setErrors((prev) => ({ ...prev, [chat]: text })),
    [],
  );

  // The composer's text and files for this chat, and the files dropped or pasted into them.
  const {
    draft,
    setDraft,
    attachments,
    setAttachments,
    queuedEdit,
    setQueuedEdit,
    composerDraft,
    composerAttachments,
    setComposerDraft,
    setComposerAttachments,
    sentWith,
    draftWithout,
    restoreDraft,
    attach,
    dropping,
    dropMentions,
    rootRef,
    keyRef,
  } = useComposerDraft({
    threadKey: thread?.id ?? "new",
    projectPath: project?.path ?? null,
    onError: setErrorFor,
  });

  // Files the @ mention menu offers; they follow the project.
  const [files, setFiles] = useState<string[]>([]);
  const projectId = project?.id ?? null;
  useEffect(() => {
    if (!projectId) {
      setFiles([]);
      return;
    }
    let cancelled = false;
    api.listFiles(projectId).then(
      (list) => {
        if (!cancelled) setFiles(list);
      },
      () => {
        if (!cancelled) setFiles([]);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [projectId]);

  // A project whose folder was moved or deleted cannot run its agent until it is pointed
  // elsewhere.
  const projectPath = project?.path ?? null;
  const [folderMissing, setFolderMissing] = useState(false);
  useEffect(() => {
    setFolderMissing(false);
    if (!projectPath) return;
    let cancelled = false;
    api.isFolder(projectPath).then(
      (found) => {
        if (!cancelled) setFolderMissing(!found);
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  }, [projectPath]);

  // Agent/model/access chosen for the next thread; null means the defaults.
  const [pick, setPick] = useState<NewThreadOptions | null>(null);
  const [modelPickerRequest, setModelPickerRequest] = useState(0);
  const [sending, setSending] = useState(false);
  // The thread whose large, cold session is asking before a message is sent to it.
  const [resumeAsk, setResumeAsk] = useState<string | null>(null);
  // Kept here so Ctrl+Enter in the composer can tell whether the queued strip is expanded.
  const [queueOpen, setQueueOpen] = useState(true);
  // Ctrl+Enter shows the Steer button as pressed for a moment, as a mouse click would.
  const [steerPressed, setSteerPressed] = useState(false);
  const steerPressedTimer = useRef(0);

  const key = thread?.id ?? "new";
  const error = errors[key] ?? null;
  const setError = (text: string | null) => setErrorFor(key, text);

  const newOpts: NewThreadOptions = pick ?? {
    agent: defaultAgent,
    model: tokens.defaultModels[defaultAgent] ?? null,
    accessMode: lastAccessMode(),
    effort: lastEffort(defaultAgent),
    compactWindow: lastCompactWindow(defaultAgent),
  };
  const empty = !thread || thread.messages.length === 0;
  const queued = useMemo(() => thread?.messages.filter((m) => m.queued) ?? [], [thread?.messages]);
  // A queued message can be sent while it is being edited: the agent took it, or it was steered.
  // What was typed stays in the composer as an ordinary draft, to send or queue again.
  useEffect(() => {
    if (!queuedEdit || queuedEdit.chat !== key || queued.some((m) => m.id === queuedEdit.id)) {
      return;
    }
    setDraft(queuedEdit.text);
    setAttachments(queuedEdit.attachments);
    setQueuedEdit(null);
  }, [queued, queuedEdit, key]);
  const background = useMemo(
    () => thread?.messages.flatMap((m) => m.background) ?? [],
    [thread?.messages],
  );
  const agent = thread?.agent ?? newOpts.agent;
  const model = thread ? thread.model : newOpts.model;
  const accessMode = thread?.accessMode ?? newOpts.accessMode;
  const effort = thread ? thread.effort : newOpts.effort;
  const compactWindow = thread ? thread.compactWindow : newOpts.compactWindow;
  const working = sending || isWorking(thread);
  // The reply under way is compacting, or a /compact was sent and its reply has not started.
  const compacting = useMemo(() => {
    if (!working) return false;
    const last = thread?.messages.filter((m) => !m.queued).at(-1);
    return (
      last !== undefined &&
      (last.compacting === true || (last.role === "user" && isCompactCommand(last.text)))
    );
  }, [working, thread?.messages]);
  const usage = thread && threadUsage(thread.messages);
  const { clearOfGoal, clearGoal } = useGoalClear(thread, working);
  const checkpoints = useCheckpoints(thread?.id, projectId, working);
  // Before the first message any connected agent's models are offered.
  const offered = agents.includes(agent) ? agents : [agent, ...agents];
  /** Every agent is switched off, so a new chat has nothing to run it. */
  const noAgents = empty && agents.length === 0;
  // What the user sent in this chat, for Up/Down in the composer; repeats count once. It keeps
  // its identity while a reply streams, so the composer skips those renders.
  const historyRef = useRef<HistoryEntry[]>([]);
  const history = useMemo(() => {
    const entries: HistoryEntry[] = [];
    for (const m of thread?.messages ?? []) {
      const text = m.text.trim();
      if (m.role !== "user" || !text) continue;
      if (entries.at(-1)?.text === text) entries.pop();
      entries.push({ id: m.id, text });
    }
    const prev = historyRef.current;
    const same =
      prev.length === entries.length &&
      prev.every((h, i) => h.id === entries[i].id && h.text === entries[i].text);
    if (!same) historyRef.current = entries;
    return historyRef.current;
  }, [thread?.messages]);
  const composerAgents = empty || thread?.contextPending ? offered : [agent];
  const composerAgentsKey = composerAgents.join();
  const stableComposerAgents = useMemo(() => composerAgents, [composerAgentsKey]);

  async function dispatch(
    text: string,
    files: Attachment[],
    restoreOnError: boolean,
    queue = false,
  ) {
    const sentKey = key;
    setError(null);
    setSending(true);
    try {
      await onSend(text, files, newOpts, queue);
      // The role went into the chat that was started; the next new chat starts with none.
      setPick((picked) => (picked ? { ...picked, role: null } : picked));
    } catch (e) {
      // A send from the new-task screen has opened its thread by now, if one was made. The error
      // and the text go back to that thread, where the composer now is.
      const errorKey = sentKey === "new" ? keyRef.current : sentKey;
      setErrorFor(errorKey, errorText(e));
      if (restoreOnError) restoreDraft(errorKey, text, draftWithout(text, files));
    } finally {
      setSending(false);
    }
  }

  // /clear asks first: the messages are deleted and cannot be recovered.
  async function clearChat() {
    if (!thread) {
      setDraft("");
      return;
    }
    const ok = await confirm(
      `Clear ${displayTitle(thread.title)}? Its messages are deleted and the agent forgets this conversation.`,
      { title: "Clear chat", kind: "warning" },
    );
    if (!ok) return;
    setDraft("");
    setAttachments([]);
    await onClearThread(thread.id);
  }

  // Restoring rewrites the project's files, so it asks first. The undone message goes back into
  // the composer, to edit or send again.
  async function restoreCheckpoint(message: Message) {
    if (!thread || working) return;
    const ok = await confirm(
      "Put the project's files back as they were before this message? Changes made since, by the agent or by you, are discarded, and this message and the ones after it are removed.",
      { title: "Restore checkpoint", kind: "warning" },
    );
    if (!ok) return;
    setError(null);
    try {
      await onRestoreCheckpoint(thread.id, message.id);
      // What is written in the composer now stays; the restored message only fills an empty one.
      if (!draft.trim() && attachments.length === 0) {
        setDraft(message.text);
        setAttachments(draftWithout(message.text, message.attachments));
      }
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function forkThread(message: Message) {
    if (!thread) return;
    setError(null);
    try {
      await onForkThread(thread.id, message.id);
    } catch (e) {
      setError(errorText(e));
    }
  }

  // The typed /compact: the draft held only the command.
  function compactChat() {
    setDraft("");
    compactNow();
  }

  // /compact is passed to the agent, which compacts its own context. The compact picker calls it
  // too, leaving the draft as it is.
  function compactNow() {
    if (!thread) return;
    void dispatch("/compact", [], true);
  }

  function submit(queue: boolean) {
    const text = composerDraft.trim();
    if (folderMissing) return;
    if (queuedEdit) {
      if (!thread) return;
      api
        .editQueued(thread.id, queuedEdit.id, text, sentWith(text, composerAttachments))
        .then(() => setQueuedEdit(null))
        .catch((e: unknown) => setError(errorText(e)));
      return;
    }
    // With a plan ready, sending nothing implements it; anything written refines it.
    if (readyPlan !== null && !text && attachments.length === 0) {
      void implementPlan(false);
      return;
    }
    // An answer written in the composer goes to the agent's question, not into the chat.
    if (asking && ownAnswer === asking.id) {
      if (!text) return;
      setDraft("");
      answerQuestion(text);
      return;
    }
    const command = attachments.length === 0 ? localCommand(text) : null;
    if (command) {
      runLocalCommand(command);
      return;
    }
    // Mid-turn, /goal clear is the Stop button: the agent's session drops the goal once it exits.
    if (working && thread?.goal && isGoalClearCommand(text) && attachments.length === 0) {
      setDraft("");
      stop();
      return;
    }
    // While a turn runs, a message steers it (the server decides whether the agent can).
    if (!text && attachments.length === 0) return;
    // Resuming a large session whose cache has expired re-reads all of it at full price.
    if (
      thread &&
      !working &&
      tokens.askBeforeResume &&
      coldLargeSession(thread, Date.now(), tokens.warnAbove)
    ) {
      setResumeAsk(thread.id);
      return;
    }
    setDraft("");
    setAttachments([]);
    void dispatch(text, sentWith(text, attachments), true, queue);
  }

  // Takes a queued message back from the agent's queue. Editing puts its text and attachments in
  // the composer; it only does so once the server has taken the message back.
  function takeBack(message: Message, edit: boolean) {
    if (!thread) return;
    if (edit) {
      setQueuedEdit({
        chat: thread.id,
        id: message.id,
        text: message.text,
        attachments: draftWithout(message.text, message.attachments),
      });
      return;
    }
    api.removeQueued(thread.id, message.id).catch((e: unknown) => setError(errorText(e)));
  }

  function editLatestQueued() {
    if (queuedEdit || composerDraft.trim() || composerAttachments.length) return;
    const latestQueued = queued.at(-1);
    if (thread && latestQueued)
      setQueuedEdit({
        chat: thread.id,
        id: latestQueued.id,
        text: latestQueued.text,
        attachments: draftWithout(latestQueued.text, latestQueued.attachments),
      });
  }

  function resumeQueued() {
    if (!thread || working) return;
    api.resumeQueued(thread.id).catch((e: unknown) => setError(errorText(e)));
  }

  function steerQueued(message: Message) {
    if (!thread || !working) return;
    api.steerQueued(thread.id, message.id).catch((e: unknown) => setError(errorText(e)));
  }

  function stopBackground(task: BackgroundTask) {
    if (!thread) return;
    api.stopSubagent(thread.id, task.toolId).catch((e: unknown) => setError(errorText(e)));
  }

  // Answers the resume prompt. Compacting leaves the draft in the composer, so the message
  // is sent once the compaction is done rather than steering it.
  function answerResume(compact: boolean, dontAsk: boolean) {
    setResumeAsk(null);
    if (dontAsk) onUpdateTokens({ askBeforeResume: false });
    if (compact) {
      void dispatch("/compact", [], false);
      return;
    }
    const text = draft.trim();
    setDraft("");
    setAttachments([]);
    void dispatch(text, sentWith(text, attachments), true);
  }

  // Handed to a memoized Transcript, whose rows then skip the renders a stream causes.
  const transcriptHandlers = useStableHandlers({
    onRetry: (message: Message) => {
      if (!working) void dispatch(message.text, message.attachments, false);
    },
    onOpenCanvas: (path: string, message: Message | null, tab: CanvasTab) =>
      onOpenCanvas(path, message, tab),
    onShowChanges: (messageId: string) => onShowChanges(messageId),
    onRestore: (message: Message) => void restoreCheckpoint(message),
    onFork: (message: Message) => void forkThread(message),
  });

  // Stops the running turn, goal or not. A goal it was pursuing is cleared once the agent has
  // exited, in the agent's session too.
  function stop() {
    if (!thread) return;
    setError(null);
    onCancel(thread.id).catch((e: unknown) => setError(errorText(e)));
  }

  // Codex takes /goal resume; Claude and Antigravity have no such command, so a message carries
  // the goal on.
  function resumeGoal() {
    if (!thread) return;
    void dispatch(thread.agent === "codex" ? "/goal resume" : "Continue the goal.", [], false);
  }

  function pickModel(nextAgent: AgentKind, nextModel: string | null) {
    if (!thread) {
      const same = nextAgent === agent;
      setPick({
        ...newOpts,
        agent: nextAgent,
        model: nextModel,
        effort: same ? newOpts.effort : lastEffort(nextAgent),
        compactWindow: same ? newOpts.compactWindow : lastCompactWindow(nextAgent),
      });
      return;
    }
    setError(null);
    onSetModel(thread.id, nextAgent, nextModel).catch((e: unknown) => setError(errorText(e)));
    // A level the new agent does not accept would make its CLI refuse the turn.
    if (thread.effort !== effortFor(nextAgent, thread.effort)) pickEffort(null);
  }

  function pickEffort(next: string | null) {
    if (!thread) {
      setPick({ ...newOpts, effort: next });
      return;
    }
    setError(null);
    onSetEffort(thread.id, next).catch((e: unknown) => setError(errorText(e)));
  }

  function pickCompactWindow(next: number | null) {
    if (!thread) {
      setPick({ ...newOpts, compactWindow: next });
      return;
    }
    setError(null);
    onSetCompactWindow(thread.id, next).catch((e: unknown) => setError(errorText(e)));
  }

  const planSupported = agent !== "antigravity";
  function setPlanMode(enabled: boolean) {
    if (!thread) {
      setPick({ ...newOpts, planMode: enabled });
      return;
    }
    setError(null);
    onSetPlanMode(thread.id, enabled).catch((e: unknown) => setError(errorText(e)));
  }
  function runLocalCommand(command: LocalCommand) {
    switch (command.name) {
      case "/clear":
        void clearChat();
        return;
      case "/compact":
        compactChat();
        return;
      case "/model":
        setDraft("");
        setModelPickerRequest((request) => request + 1);
        return;
      case "/plan":
      case "/default":
        if (!planSupported) setError("Plan mode is not supported by Antigravity.");
        else setPlanMode(command.name === "/plan");
        setDraft("");
        return;
      // /new starts a fresh chat; the current one is kept in the sidebar.
      case "/new":
        setDraft("");
        onNewChat();
        return;
    }
  }
  function selectSlashCommand(name: string): boolean {
    const command = localCommand(name);
    if (command?.runs !== "pick") return false;
    runLocalCommand(command);
    return true;
  }
  function pickAccess(mode: AccessMode) {
    if (!thread) {
      setPick({ ...newOpts, accessMode: mode });
      return;
    }
    setError(null);
    onSetAccessMode(thread.id, mode).catch((e: unknown) => setError(errorText(e)));
  }

  const threadId = thread?.id;
  const respond = useCallback(
    (approvalId: string, allow: boolean, answers?: Record<string, string[]>) => {
      if (!threadId) return;
      setError(null);
      api
        .respondApproval(threadId, approvalId, allow, answers)
        .catch((e: unknown) => setError(errorText(e)));
    },
    [threadId],
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
      if (!threadId) return;
      setError(null);
      const rules = always ? suggestedRules(approval) : [];
      const rule = always && rules[0] ? { scope: always, ruleId: rules[0].id } : undefined;
      api
        .respondApproval(threadId, approval.id, allow, undefined, rule)
        .then(() => {
          if (always && rules.length > 0) permissionsContext.addRules(always, rules);
        })
        .catch((e: unknown) => setError(errorText(e)));
    },
    [threadId, permissionsContext],
  );
  const pendingApprovals =
    thread?.messages.flatMap((m) => m.approvals).filter((a) => a.status === "pending") ?? [];
  // Questions the agent asks wait in a choice panel, one at a time; permission requests wait in
  // the banner.
  const asking = pendingApprovals.find((a) => a.questions?.length);
  const permissions = pendingApprovals.filter((a) => !a.questions?.length);
  const [picks, setPicks] = useState<{ id: string; answers: Record<string, string[]> } | null>(
    null,
  );
  // The question the user is answering in the composer, by its approval's id.
  const [ownAnswer, setOwnAnswer] = useState<string | null>(null);
  const answered = asking && picks?.id === asking.id ? picks.answers : {};
  const question = asking?.questions?.find((q) => !(q.id in answered));
  const showQuestion = question !== undefined && ownAnswer !== asking?.id;
  // Records the answer to the shown question, and sends them all once the last is answered.
  function answerQuestion(answer: string) {
    if (!asking?.questions || !question) return;
    const answers = { ...answered, [question.id]: [answer] };
    setOwnAnswer(null);
    if (asking.questions.every((q) => q.id in answers)) {
      setPicks(null);
      respond(asking.id, true, answers);
    } else {
      setPicks({ id: asking.id, answers });
    }
  }

  // The plan of the reply running now. Once that reply ends its plan stays, until the next message
  // is sent: the panel closing as the turn stopped took its height back from the chat, which moved
  // every message down. Plans of replies that ended before the chat was opened are not shown.
  const lastMessage = thread?.messages.at(-1);
  const livePlanId =
    working &&
    lastMessage?.role === "assistant" &&
    lastMessage.status === "streaming" &&
    lastMessage.plan.length > 0
      ? lastMessage.id
      : null;
  const [keptPlanId, setKeptPlanId] = useState<string | null>(null);
  useEffect(() => {
    if (livePlanId !== null) setKeptPlanId(livePlanId);
  }, [livePlanId]);
  const plan =
    lastMessage?.role === "assistant" &&
    (lastMessage.id === livePlanId || lastMessage.id === keptPlanId)
      ? lastMessage.plan
      : [];
  // A finished list stays a moment to show it complete, then folds away; a new list brings it back.
  const planDone = plan.length > 0 && plan.every((step) => step.status === "done");
  const [planCleared, setPlanCleared] = useState(false);
  useEffect(() => {
    setPlanCleared(false);
    if (!planDone) return;
    const timer = window.setTimeout(() => setPlanCleared(true), 1200);
    return () => window.clearTimeout(timer);
  }, [planDone]);
  // Choices the agent offered in its last finished reply, until the user answers or dismisses them.
  // A plan the last reply proposed in plan mode, ready to implement or refine.
  const proposed =
    thread?.planMode && !working && !asking && lastMessage?.role === "assistant"
      ? [...lastMessage.parts].reverse().find((p) => p.type === "plan")
      : undefined;
  const readyPlan = proposed?.type === "plan" ? proposed.text : null;
  const IMPLEMENT = "PLEASE IMPLEMENT THIS PLAN:\n";
  // Implementing leaves plan mode, in this thread or a new one named after the plan.
  async function implementPlan(newThread: boolean) {
    if (!thread || readyPlan === null) return;
    const text = IMPLEMENT + readyPlan.trim();
    setError(null);
    if (newThread) {
      const opts: NewThreadOptions = {
        agent: thread.agent,
        model: thread.model,
        accessMode: thread.accessMode,
        effort: thread.effort,
        compactWindow: thread.compactWindow,
      };
      await onStartThread(opts, `Implement ${planTitle(readyPlan)}`.slice(0, 60), text);
      return;
    }
    try {
      await onSetPlanMode(thread.id, false);
      await dispatch(text, [], false);
    } catch (e) {
      setError(errorText(e));
    }
  }
  const { choices, showChoices, answerChoice, dismissChoices } = useChoices(
    thread,
    working,
    (option) => void dispatch(option, [], true),
  );
  const isNew = !thread || empty;
  // The composer is memoized; these keep their identity across the renders a stream causes.
  const composerHandlers = useStableHandlers({
    onChange: setComposerDraft,
    onSubmit: submit,
    onEditLatestQueued: editLatestQueued,
    onSteerQueued: () => {
      if (!queued[0]) return;
      setSteerPressed(true);
      window.clearTimeout(steerPressedTimer.current);
      steerPressedTimer.current = window.setTimeout(() => setSteerPressed(false), 150);
      steerQueued(queued[0]);
    },
    onStop: stop,
    onPickModel: pickModel,
    onSelectSlashCommand: selectSlashCommand,
    onPickAccess: pickAccess,
    onPickEffort: pickEffort,
    onPickCompactWindow: pickCompactWindow,
    onCompactNow: compactNow,
    onAttachPaths: (paths: string[]) => void attach(paths),
    onRemoveAttachment: (i: number) =>
      setComposerAttachments(composerAttachments.filter((_, j) => j !== i)),
    onPickRole: (next: string | null) => {
      if (!thread) {
        setPick({ ...newOpts, role: next });
        return;
      }
      setError(null);
      api.setThreadRole(thread.id, next).catch((e: unknown) => setError(errorText(e)));
    },
  });

  return (
    <div
      className={`relative flex min-h-0 min-w-0 flex-1 flex-col${isNew ? " justify-center" : ""}`}
      ref={rootRef}
    >
      {thread && (
        <LineageCard thread={thread} working={working} onDisconnect={stop} onError={setError} />
      )}
      <div className={`flex min-h-0 flex-col ${isNew ? "flex-none overflow-y-auto" : "flex-1"}`}>
        {!isNew ? (
          <PermissionsContext.Provider value={permissionsContext}>
            <Transcript
              thread={thread}
              onOpenThread={onOpenThread}
              changes={changes}
              busy={working}
              onOpenCanvas={project ? transcriptHandlers.onOpenCanvas : undefined}
              onShowChanges={transcriptHandlers.onShowChanges}
              onRetry={transcriptHandlers.onRetry}
              checkpoints={checkpoints}
              onRestore={transcriptHandlers.onRestore}
              onFork={transcriptHandlers.onFork}
              tokens={tokens}
            />
          </PermissionsContext.Provider>
        ) : (
          <div className="flex h-auto flex-col items-center justify-center gap-4 p-6 text-center">
            <ProviderLogos kind={agent} size={40} />
            <h2 className="m-0 text-[length:var(--fs-xl)] font-semibold">
              {project ? `What should we build in ${project.name}?` : "What's on your mind?"}
            </h2>
            {!project && (
              <>
                <p className="-mt-1.5 mb-0 text-fg-muted">
                  This chat has no project; the agent works in a private scratch folder.
                </p>
                <button className="btn my-2 h-8 !px-4" onClick={onOpenFolder}>
                  <FolderPlusIcon size={14} />
                  Open a project
                </button>
              </>
            )}
            <div className="flex max-w-[560px] flex-wrap justify-center gap-2">
              {(project ? SUGGESTIONS : CHAT_SUGGESTIONS).map((s) => (
                <button
                  key={s.label}
                  className="h-7 cursor-pointer rounded-[14px] border border-border-strong bg-panel px-3 text-fg hover:bg-hover"
                  onClick={() => setDraft(s.prefill)}
                >
                  {s.label}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
      {/* A subagent's thread is its transcript; it takes no messages. */}
      {thread?.parent && (
        <div className="flex w-full flex-none box-border items-center gap-2 border-t border-border px-6 py-2.5 text-[length:var(--fs-sm)] text-fg-muted">
          <span>Subagent of</span>
          <button
            type="button"
            className="cursor-pointer truncate rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-fg hover:bg-hover"
            onClick={() => parentThread && onOpenThread(parentThread.id)}
            disabled={!parentThread}
          >
            {parentThread ? displayTitle(parentThread.title) : "a deleted thread"}
          </button>
          <span className="ml-auto text-fg-subtle">Read-only</span>
        </div>
      )}
      <div className={`w-full flex-none box-border px-6 pb-4 ${thread?.parent ? " hidden" : ""}`}>
        <BackgroundTasks tasks={background} onStop={stopBackground} />
        <ComposerStripPresence
          key={`${key}-plan`}
          present={plan.length > 0 && !(planDone && planCleared)}
        >
          {plan.length > 0 && <PlanPanel steps={plan} />}
        </ComposerStripPresence>
        {readyPlan !== null && (
          <div
            title="Write feedback to refine the plan, or send nothing to implement it"
            className="mb-1.5 flex items-center gap-2 rounded-[var(--radius)] border border-border bg-panel px-2.5 py-1.5 text-[length:var(--fs-sm)]"
          >
            <ListChecksIcon size={14} className="flex-none text-fg-muted" />
            <span className="flex-none font-medium text-fg-muted">Plan ready</span>
            <span className="min-w-0 flex-1 truncate text-fg">{planTitle(readyPlan)}</span>
            <button className="btn flex-none" onClick={() => void implementPlan(true)}>
              Implement in a new thread
            </button>
            <button className="btn btn-primary flex-none" onClick={() => void implementPlan(false)}>
              Implement
            </button>
          </div>
        )}
        {permissions.length > 0 && (
          <PendingApprovalBanner
            approvals={permissions}
            scope={scope}
            scopeName={scopeName(scope, projects)}
            onRespond={respondPermission}
          />
        )}
        {asking && question && showQuestion && (
          <ChoicePanel
            key={question.id}
            question={question.question}
            options={question.options.map((o) => o.label)}
            descriptions={question.options.map((o) => o.description)}
            onPick={answerQuestion}
            confirm={confirmAnswers}
            onOwn={() => setOwnAnswer(asking.id)}
            onDismiss={() => respond(asking.id, false)}
          />
        )}
        {folderMissing && projectPath && (
          <MissingFolderPrompt path={projectPath} onChangeFolder={onChangeFolder} />
        )}
        {thread && resumeAsk === thread.id && (
          <ResumePrompt
            idleMs={Date.now() - (thread.messages.at(-1)?.finishedAt ?? Date.now())}
            context={thread.messages.at(-1)?.usage?.context ?? 0}
            onAnswer={answerResume}
          />
        )}
        {showChoices && choices && (
          <ChoicePanel
            options={choices}
            onPick={answerChoice}
            onOwn={dismissChoices}
            onDismiss={dismissChoices}
          />
        )}
        <ComposerStripPresence key={`${key}-queue`} present={queued.length > 0}>
          <QueuedMessages
            messages={queued}
            canEdit={true}
            canSteer={working}
            steerPressed={steerPressed}
            open={queueOpen}
            onToggle={() => setQueueOpen((value) => !value)}
            onEdit={(message) => takeBack(message, true)}
            onRemove={(message) => takeBack(message, false)}
            onSteer={steerQueued}
            onResume={resumeQueued}
            editingId={queuedEdit?.id ?? null}
            onCancelEdit={() => setQueuedEdit(null)}
          />
        </ComposerStripPresence>
        <ComposerStripPresence key={`${key}-goal`} present={Boolean(thread?.goal)}>
          {thread?.goal && (
            <GoalBanner
              key={thread.goal.startedAt}
              goal={thread.goal}
              working={working}
              clearing={clearOfGoal?.error === null}
              clearError={clearOfGoal?.error ?? null}
              onStop={stop}
              onResume={resumeGoal}
              onClear={clearGoal}
            />
          )}
        </ComposerStripPresence>
        {thread && tokens.showUsage && (
          <UsageLine usage={usage || null} warnAbove={tokens.warnAbove} />
        )}
        {/* Loaded with the chat, not on its own: a placeholder of another height moved the
            messages once the editor arrived. */}
        <Composer
          key={key}
          value={composerDraft}
          onChange={composerHandlers.onChange}
          onSubmit={composerHandlers.onSubmit}
          onEditLatestQueued={composerHandlers.onEditLatestQueued}
          onSteerQueued={composerHandlers.onSteerQueued}
          canSteerQueued={working && queued.length > 0}
          editingQueued={queuedEdit !== null}
          canQueueFollowUp={queued.length === 0}
          onStop={composerHandlers.onStop}
          goalActive={thread?.goal?.status === "active"}
          working={working}
          backgroundRunning={background.some(
            (task) => task.status === "running" || task.status === "waiting",
          )}
          error={
            error ?? (noAgents ? "No agents connected. Turn one on in Settings → Providers." : null)
          }
          agent={agent}
          model={model}
          agents={stableComposerAgents}
          onPickModel={composerHandlers.onPickModel}
          modelPickerRequest={modelPickerRequest}
          providerSlashCommands={thread?.slashCommands ?? NO_COMMANDS}
          onSelectSlashCommand={composerHandlers.onSelectSlashCommand}
          accessMode={accessMode}
          onPickAccess={composerHandlers.onPickAccess}
          effort={effort}
          onPickEffort={composerHandlers.onPickEffort}
          compactWindow={compactWindow}
          defaultCompactWindow={tokens[COMPACT_SETTINGS[agent]]}
          context={usage?.context ?? null}
          onPickCompactWindow={composerHandlers.onPickCompactWindow}
          onCompactNow={thread && !empty && !working ? composerHandlers.onCompactNow : null}
          attachments={composerAttachments}
          onAttachPaths={composerHandlers.onAttachPaths}
          onRemoveAttachment={composerHandlers.onRemoveAttachment}
          dropping={dropping}
          dropMentions={dropMentions}
          history={history}
          files={files}
          deferFocus={showChoices || showQuestion}
          disabled={folderMissing || noAgents}
          compacting={compacting}
          projectId={projectId}
          role={thread ? thread.role : (newOpts.role ?? null)}
          onPickRole={composerHandlers.onPickRole}
        />
      </div>
    </div>
  );
}
