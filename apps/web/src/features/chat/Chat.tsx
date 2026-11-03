import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { ListChecksIcon } from "@phosphor-icons/react";
import {
  api,
  type AccessMode,
  type AgentKind,
  type Attachment,
  type BackgroundTask,
  type CheckSettings,
  type FileChange,
  type Message,
  type PermissionScope,
  type PermissionSettings,
  type Project,
  type ProviderSlashCommand,
  type Thread,
  type TokenSettings,
} from "~/api";
import type { CanvasTab } from "~/features/canvas/Canvas";
import { COMPACT_SETTINGS } from "~/components/CompactPicker";
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
import EmptyChat from "~/features/chat/EmptyChat";
import QueuedMessages from "~/features/chat/composer/components/QueuedMessages";
import LineageCard from "~/features/chat/lineage/LineageCard";
import Composer from "~/features/chat/composer/Composer";
import type { HistoryEntry } from "~/features/chat/composer/types";
import Transcript from "~/features/chat/transcript/Transcript";
import { UsageLine } from "~/features/chat/transcript/components/UsageLine";
import {
  ChecksContext,
  type ChecksContextValue,
} from "~/features/chat/transcript/components/ChecksCard";
import PendingApprovalBanner from "~/features/chat/prompts/PendingApprovalBanner";
import { useApprovals } from "~/features/chat/prompts/useApprovals";
import { PermissionsContext, scopeName } from "~/lib/permissions";
import PlanPanel from "~/features/chat/plan/PlanPanel";
import { planTitle } from "~/features/chat/plan/ProposedPlanCard";
import { usePlan } from "~/features/chat/plan/usePlan";
import MissingFolderPrompt from "~/features/chat/prompts/MissingFolderPrompt";
import ResumePrompt from "~/features/chat/prompts/ResumePrompt";
import { useChatOptions } from "~/features/chat/hooks/useChatOptions";
import { useCheckpoints } from "~/features/chat/hooks/useCheckpoints";
import { useChoices } from "~/features/chat/hooks/useChoices";
import { useComposerDraft } from "~/features/chat/composer/hooks/useComposerDraft";
import { useGoalClear } from "~/features/chat/hooks/useGoalClear";
import { useProjectFolder } from "~/features/chat/hooks/useProjectFolder";
import { useQueue } from "~/features/chat/hooks/useQueue";
import { useSends } from "~/features/chat/hooks/useSends";
import { copySelection } from "~/features/chat/lib/textSelection";
import { useStableHandlers } from "~/hooks/useStableHandlers";
import type { NewThreadOptions } from "~/hooks/useThreads";
import { isLiveTask, isWorking } from "~/lib/threadEvents";
import { coldLargeSession, threadUsage } from "~/lib/tokens";
import { displayTitle, errorText } from "~/lib/util";

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
    // Called with the thread the message goes to, once a send from the new-task screen has made it.
    onThread: (threadId: string) => void,
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
  /** Enter steers a running reply rather than queueing the message. */
  steerWhileWorking: boolean;
  projects: Project[];
  onUpdatePermissions: (change: (p: PermissionSettings) => PermissionSettings) => void;
  /** Opens Settings → Permissions on a scope, with one of its rules picked out. */
  onOpenPermissions: (scope: PermissionScope, ruleId?: string) => void;
  /** Settings → Projects, for the switch on a reply's Checks card. */
  checkSettings: CheckSettings;
  onSetProjectChecks: (projectId: string, on: boolean) => void;
}

// A fallback that keeps one identity, so the memoized composer sees no change.
const NO_COMMANDS: ProviderSlashCommand[] = [];

/** Longest gap between the two Escape presses that stop a running turn. */
const STOP_SEQUENCE_MS = 500;

/** A large, cold session asking before a message is sent to it. It asks about the thread as it
 * was then: a turn that starts while it waits, such as a queued message going out, has resumed
 * the session already. */
interface ResumeAsk {
  threadId: string;
  /** The thread's last message that was not queued, when it asked. */
  after: string | undefined;
}

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
  steerWhileWorking,
  projects,
  onUpdatePermissions,
  onOpenPermissions,
  checkSettings,
  onSetProjectChecks,
}: ChatProps) {
  // Errors by the chat they happened in ("new" for the new-task screen), so one never shows in
  // another chat.
  const [errors, setErrors] = useState<Record<string, string | null>>({});
  const setErrorFor = useCallback(
    (chat: string, text: string | null) => setErrors((prev) => ({ ...prev, [chat]: text })),
    [],
  );
  const key = thread?.id ?? "new";
  const error = errors[key] ?? null;
  const setError = (text: string | null) => setErrorFor(key, text);

  const projectId = project?.id ?? null;
  const projectPath = project?.path ?? null;
  // The composer's text and files for this chat, and the files dropped or pasted into them.
  const composer = useComposerDraft({ threadKey: key, projectPath, onError: setErrorFor });
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
    attach,
    dropping,
    dropMentions,
    rootRef,
  } = composer;
  const { files, folderMissing } = useProjectFolder(projectId, projectPath);
  const options = useChatOptions({
    thread,
    defaultAgent,
    tokens,
    setError,
    onSetModel,
    onSetAccessMode,
    onSetPlanMode,
    onSetEffort,
    onSetCompactWindow,
  });
  const { agent, newOpts } = options;
  const sends = useSends({
    chat: key,
    options: newOpts,
    draft: composer,
    onSend,
    onSent: options.dropPickedRole,
    setErrorFor,
  });
  const { dispatch } = sends;
  const working = sends.working || isWorking(thread);
  const queue = useQueue({ thread, working, draft: composer, setError });
  const { queued } = queue;
  const approvals = useApprovals({
    thread,
    project,
    permissionSettings,
    projects,
    onUpdatePermissions,
    onOpenPermissions,
    setErrorFor,
  });
  const { asking, question, scope } = approvals;
  const showQuestion = question !== undefined && !approvals.answerInComposer;
  const plan = usePlan({
    thread,
    working,
    asking: asking !== undefined,
    send: (text) => dispatch(text, [], false),
    onStartThread,
    onSetPlanMode,
    setError,
  });
  const { readyPlan, implementPlan } = plan;
  const { choices, showChoices, answerChoice, dismissChoices } = useChoices(
    thread,
    working,
    (option) => void dispatch(option, [], true),
  );

  const [modelPickerRequest, setModelPickerRequest] = useState(0);
  const [resumeAsk, setResumeAsk] = useState<ResumeAsk | null>(null);
  const settledId = thread?.messages.filter((m) => !m.queued).at(-1)?.id;
  const asksResume =
    thread !== null &&
    !working &&
    resumeAsk?.threadId === thread.id &&
    resumeAsk.after === settledId;
  // Set by the composer: adds text quoted from a reply to the draft.
  const quoteInto = useRef<((lines: string[]) => void) | null>(null);

  const empty = !thread || thread.messages.length === 0;
  const isNew = !thread || empty;
  const background = useMemo(
    () => thread?.messages.flatMap((m) => m.background) ?? [],
    [thread?.messages],
  );
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
  const checksContext = useMemo<ChecksContextValue>(
    () => ({ settings: checkSettings, setProject: onSetProjectChecks }),
    [checkSettings, onSetProjectChecks],
  );

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

  function submit(toQueue: boolean) {
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
    if (asking && approvals.answerInComposer) {
      if (!text) return;
      setDraft("");
      approvals.answerQuestion(text);
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
      setResumeAsk({ threadId: thread.id, after: settledId });
      return;
    }
    void sends.sendHeld(toQueue);
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
    // The draft may have been cleared while the prompt waited.
    if (!text && attachments.length === 0) return;
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
    onQuote: (lines: string[]) => quoteInto.current?.(lines),
  });

  // Stops the running turn, goal or not. A goal it was pursuing is cleared once the agent has
  // exited, in the agent's session too.
  function stop() {
    if (!thread) return;
    setError(null);
    onCancel(thread.id).catch((e: unknown) => setError(errorText(e)));
  }

  // Escape pressed twice in the chat or its composer stops the running turn, as Stop does; queued
  // messages stay. The presses are read before anything handles them, so an open menu, dialog or
  // approval takes them instead, and a held key, a composition or a new turn starts again.
  const lastEscape = useRef(0);
  useEffect(() => {
    lastEscape.current = 0;
  }, [working, key]);
  function stopOnEscapeTwice(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    const popup = document.querySelector(
      '[role="menu"], [role="listbox"], [role="dialog"], [aria-label="Approval request"]',
    );
    if (!working || e.repeat || e.nativeEvent.isComposing || popup) {
      lastEscape.current = 0;
      return;
    }
    if (e.timeStamp - lastEscape.current < STOP_SEQUENCE_MS) {
      lastEscape.current = 0;
      stop();
    } else {
      lastEscape.current = e.timeStamp;
    }
  }

  // Codex takes /goal resume; Claude and Antigravity have no such command, so a message carries
  // the goal on.
  function resumeGoal() {
    if (!thread) return;
    void dispatch(thread.agent === "codex" ? "/goal resume" : "Continue the goal.", [], false);
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
        if (!options.planSupported) setError("Plan mode is not supported by Antigravity.");
        else options.setPlanMode(command.name === "/plan");
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

  // The composer is memoized; these keep their identity across the renders a stream causes.
  const composerHandlers = useStableHandlers({
    onOpenFile: (path: string) => onOpenCanvas(path, null, "code"),
    onChange: setComposerDraft,
    onSubmit: submit,
    onEditLatestQueued: queue.editLatest,
    onSteerQueued: queue.steerFirst,
    onStop: stop,
    onPickModel: options.pickModel,
    onSelectSlashCommand: selectSlashCommand,
    onPickAccess: options.pickAccess,
    onPickEffort: options.pickEffort,
    onPickCompactWindow: options.pickCompactWindow,
    onCompactNow: compactNow,
    onAttachPaths: (paths: string[]) => void attach(paths),
    onRemoveAttachment: (i: number) =>
      setComposerAttachments(composerAttachments.filter((_, j) => j !== i)),
    onPickRole: options.pickRole,
  });

  // One copy handler for everything the chat renders: replies, messages, tool output, prompts.
  return (
    <div
      className={`relative flex min-h-0 min-w-0 flex-1 flex-col${isNew ? " justify-center" : ""}`}
      ref={rootRef}
      onCopy={copySelection}
      onKeyDownCapture={stopOnEscapeTwice}
    >
      {thread && (
        <LineageCard thread={thread} working={working} onDisconnect={stop} onError={setError} />
      )}
      <div className={`flex min-h-0 flex-col ${isNew ? "flex-none overflow-y-auto" : "flex-1"}`}>
        {!isNew ? (
          <PermissionsContext.Provider value={approvals.permissionsContext}>
            <ChecksContext.Provider value={checksContext}>
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
                onQuote={transcriptHandlers.onQuote}
                tokens={tokens}
              />
            </ChecksContext.Provider>
          </PermissionsContext.Provider>
        ) : (
          <EmptyChat
            agent={agent}
            project={project}
            onOpenFolder={onOpenFolder}
            onPrefill={setDraft}
          />
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
        <ComposerStripPresence key={`${key}-plan`} present={plan.stepsShown}>
          {plan.steps.length > 0 && <PlanPanel steps={plan.steps} />}
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
        {approvals.permissions.length > 0 && (
          <PendingApprovalBanner
            approvals={approvals.permissions}
            scope={scope}
            scopeName={scopeName(scope, projects)}
            onRespond={approvals.respondPermission}
          />
        )}
        {question && showQuestion && (
          <ChoicePanel
            key={question.id}
            question={question.question}
            options={question.options.map((o) => o.label)}
            descriptions={question.options.map((o) => o.description)}
            onPick={approvals.answerQuestion}
            confirm={confirmAnswers}
            onOwn={approvals.writeOwnAnswer}
            onDismiss={approvals.dismissQuestion}
          />
        )}
        {folderMissing && projectPath && (
          <MissingFolderPrompt path={projectPath} onChangeFolder={onChangeFolder} />
        )}
        {thread && asksResume && (
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
            steerPressed={queue.steerPressed}
            open={queue.open}
            onToggle={queue.toggle}
            onEdit={queue.edit}
            onRemove={queue.remove}
            onSteer={queue.steer}
            onResume={queue.resume}
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
          backgroundRunning={background.some(isLiveTask)}
          error={
            error ?? (noAgents ? "No agents connected. Turn one on in Settings → Providers." : null)
          }
          agent={agent}
          model={options.model}
          agents={stableComposerAgents}
          onPickModel={composerHandlers.onPickModel}
          modelPickerRequest={modelPickerRequest}
          providerSlashCommands={thread?.slashCommands ?? NO_COMMANDS}
          onSelectSlashCommand={composerHandlers.onSelectSlashCommand}
          accessMode={options.accessMode}
          onPickAccess={composerHandlers.onPickAccess}
          effort={options.effort}
          onPickEffort={composerHandlers.onPickEffort}
          compactWindow={options.compactWindow}
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
          onOpenFile={project ? composerHandlers.onOpenFile : null}
          quoteInto={quoteInto}
          deferFocus={showChoices || showQuestion}
          disabled={folderMissing || noAgents}
          sending={sends.holding}
          steerWhileWorking={steerWhileWorking}
          compacting={compacting}
          projectId={projectId}
          role={options.role}
          onPickRole={composerHandlers.onPickRole}
        />
      </div>
    </div>
  );
}
