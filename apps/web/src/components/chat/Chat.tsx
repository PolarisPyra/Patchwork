import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { FolderPlusIcon } from "@phosphor-icons/react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  api,
  type AccessMode,
  type AgentKind,
  type Attachment,
  type BackgroundTask,
  type FileChange,
  type Message,
  type Project,
  type Thread,
  type TokenSettings,
} from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import type { CanvasTab } from "~/components/canvas/Canvas";
import { lastAccessMode } from "~/components/chat/AccessPicker";
import { effortFor, lastEffort } from "~/components/chat/EffortPicker";
import { confirm } from "@tauri-apps/plugin-dialog";
import ChoicePanel from "~/components/chat/ChoicePanel";
import {
  dismissedChoiceIds,
  parseChoices,
  rememberDismissedChoice,
} from "~/components/chat/choices";
import GoalBanner from "~/components/chat/GoalBanner";
import BackgroundTasks from "~/components/chat/BackgroundTasks";
import QueuedMessages from "~/components/chat/QueuedMessages";
import LineageCard from "~/components/chat/LineageCard";
import type { HistoryEntry } from "~/components/chat/Composer";
import Messages from "~/components/chat/Messages";
import PendingApprovalBanner from "~/components/chat/PendingApprovalBanner";
import TasksBadge from "~/components/chat/TasksBadge";
import ResumePrompt from "~/components/chat/ResumePrompt";
import { isWorking, type NewThreadOptions } from "~/hooks/useThreads";
import { coldLargeSession } from "~/lib/tokens";
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
  changes: FileChange[];
  defaultAgent: AgentKind;
  /** Connected agents offered in the composer's agent picker. */
  agents: AgentKind[];
  tokens: TokenSettings;
  onUpdateTokens: (patch: Partial<TokenSettings>) => void;
  onOpenFolder: () => void;
  onOpenCanvas: (path: string, message: Message | null, tab: CanvasTab) => void;
  onShowChanges: (paths: string[]) => void;
  onSend: (
    text: string,
    attachments: Attachment[],
    draft: NewThreadOptions,
    queue: boolean,
  ) => Promise<void>;
  onCancel: (threadId: string) => Promise<void>;
  onSetModel: (threadId: string, agent: AgentKind, model: string | null) => Promise<void>;
  onSetAccessMode: (threadId: string, mode: AccessMode) => Promise<void>;
  onSetEffort: (threadId: string, effort: string | null) => Promise<void>;
  /** Starts a fresh chat in this project; the current chat stays in the sidebar. */
  onNewChat: () => void;
  /** Deletes the chat's messages and agent session; the chat stays. */
  onClearThread: (threadId: string) => Promise<void>;
}

/** The file's bytes, base64-encoded, for sending to the backend. */
function base64Of(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",", 2)[1] ?? "");
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

// The editor (Tiptap) is large and the chat can show before it loads, so it loads on its own.
const Composer = lazy(() => import("~/components/chat/Composer"));

export default function Chat({
  project,
  thread,
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
  onCancel,
  onSetModel,
  onSetAccessMode,
  onSetEffort,
}: ChatProps) {
  // Drafts are kept per thread ("new" when nothing is selected).
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  // Files and images dropped into the composer, kept per thread like drafts.
  const [pending, setPending] = useState<Record<string, Attachment[]>>({});
  const [dropping, setDropping] = useState(false);
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
  // Agent/model/access chosen for the next thread; null means the defaults.
  const [pick, setPick] = useState<NewThreadOptions | null>(null);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The thread whose large, cold session is asking before a message is sent to it.
  const [resumeAsk, setResumeAsk] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  const key = thread?.id ?? "new";
  const keyRef = useRef(key);
  const draft = drafts[key] ?? "";
  const setDraft = (text: string) => setDrafts((d) => ({ ...d, [key]: text }));
  const attachments = pending[key] ?? [];
  const setAttachments = (next: Attachment[]) => setPending((p) => ({ ...p, [key]: next }));

  const newOpts: NewThreadOptions = pick ?? {
    agent: defaultAgent,
    model: tokens.defaultModels[defaultAgent] ?? null,
    accessMode: lastAccessMode(),
    effort: lastEffort(defaultAgent),
  };
  const empty = !thread || thread.messages.length === 0;
  const queued = useMemo(() => thread?.messages.filter((m) => m.queued) ?? [], [thread?.messages]);
  const background = useMemo(
    () => thread?.messages.flatMap((m) => m.background) ?? [],
    [thread?.messages],
  );
  const agent = thread?.agent ?? newOpts.agent;
  const model = thread ? thread.model : newOpts.model;
  const accessMode = thread?.accessMode ?? newOpts.accessMode;
  const effort = thread ? thread.effort : newOpts.effort;
  const working = sending || isWorking(thread);
  // Before the first message any connected agent's models are offered.
  const offered = agents.includes(agent) ? agents : [agent, ...agents];
  // What the user sent in this chat, for Up/Down in the composer; repeats count once.
  const history = useMemo(() => {
    const entries: HistoryEntry[] = [];
    for (const m of thread?.messages ?? []) {
      const text = m.text.trim();
      if (m.role !== "user" || !text) continue;
      if (entries.at(-1)?.text === text) entries.pop();
      entries.push({ id: m.id, text });
    }
    return entries;
  }, [thread?.messages]);

  useEffect(() => {
    keyRef.current = key;
  }, [key]);

  // Tauri hands dropped files over as paths (HTML drop events carry none), so drops are
  // listened for on the webview and kept only when they land on the chat.
  useEffect(() => {
    async function attach(paths: string[]) {
      const sentKey = keyRef.current;
      try {
        const found = await Promise.all(paths.map((path) => api.attachmentForPath(path)));
        setPending((prev) => {
          const current = prev[sentKey] ?? [];
          const fresh = found.filter((a) => !current.some((c) => c.path === a.path));
          return { ...prev, [sentKey]: [...current, ...fresh] };
        });
      } catch (e) {
        setError(errorText(e));
      }
    }
    function overChat(position: { x: number; y: number }) {
      const el = rootRef.current;
      if (!el) return false;
      // Positions are physical pixels; the chat's rect is in CSS pixels.
      const scale = window.devicePixelRatio;
      const r = el.getBoundingClientRect();
      const x = position.x / scale;
      const y = position.y / scale;
      return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "leave") {
          setDropping(false);
        } else if (payload.type === "drop") {
          setDropping(false);
          if (overChat(payload.position)) void attach(payload.paths);
        } else {
          setDropping(overChat(payload.position));
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Images pasted into the chat are saved to files and attached like dropped ones. The
  // event stops here, so the composer does not also try to paste them as text.
  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    async function attachPasted(images: File[]) {
      const sentKey = keyRef.current;
      try {
        const found = await Promise.all(
          images.map(async (image) => api.savePastedImage(image.type, await base64Of(image))),
        );
        setPending((prev) => ({ ...prev, [sentKey]: [...(prev[sentKey] ?? []), ...found] }));
      } catch (e) {
        setError(errorText(e));
      }
    }
    function onPaste(event: ClipboardEvent) {
      const images = Array.from(event.clipboardData?.files ?? []).filter((file) =>
        file.type.startsWith("image/"),
      );
      if (images.length === 0) return;
      event.preventDefault();
      event.stopPropagation();
      void attachPasted(images);
    }
    el.addEventListener("paste", onPaste, true);
    return () => el.removeEventListener("paste", onPaste, true);
  }, []);

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
    } catch (e) {
      setError(errorText(e));
      if (restoreOnError) {
        setDrafts((d) => (d[sentKey] ? d : { ...d, [sentKey]: text }));
        setPending((p) => (p[sentKey] ? p : { ...p, [sentKey]: files }));
      }
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

  // /compact is passed to Claude Code, which compacts its own context. Other agents have no
  // such command, so the request is refused here rather than sent as plain text.
  function compactChat() {
    setDraft("");
    if (!thread || thread.agent !== "claude") {
      setError("/compact works in Claude chats only.");
      return;
    }
    void dispatch("/compact", [], true);
  }

  function submit(queue: boolean) {
    const text = draft.trim();
    // An answer written in the composer goes to the agent's question, not into the chat.
    if (asking && ownAnswer === asking.id) {
      if (!text) return;
      setDraft("");
      answerQuestion(text);
      return;
    }
    if (text === "/clear" && attachments.length === 0) {
      void clearChat();
      return;
    }
    if (text === "/compact" && attachments.length === 0) {
      compactChat();
      return;
    }
    // /new starts a fresh chat; the current one is kept in the sidebar.
    if (text === "/new" && attachments.length === 0) {
      setDraft("");
      onNewChat();
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
    void dispatch(text, attachments, true, queue);
  }

  // Takes a queued message back from the agent's queue. Editing puts its text and attachments in
  // the composer; it only does so once the server has taken the message back.
  function takeBack(message: Message, edit: boolean) {
    if (!thread) return;
    api
      .removeQueued(thread.id, message.id)
      .then(() => {
        if (!edit) return;
        setDraft(message.text);
        setAttachments(message.attachments);
      })
      .catch((e: unknown) => setError(errorText(e)));
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
    setDraft("");
    setAttachments([]);
    void dispatch(draft.trim(), attachments, true);
  }

  // The latest render's values, so the callbacks handed to Messages keep one identity
  // and its memoized rows skip re-rendering.
  const latest = useRef({ dispatch, working, onOpenCanvas, onShowChanges });
  useLayoutEffect(() => {
    latest.current = { dispatch, working, onOpenCanvas, onShowChanges };
  });

  const retry = useCallback((message: Message) => {
    if (!latest.current.working)
      void latest.current.dispatch(message.text, message.attachments, false);
  }, []);
  const openCanvas = useCallback(
    (path: string, message: Message | null, tab: CanvasTab) =>
      latest.current.onOpenCanvas(path, message, tab),
    [],
  );
  const showChanges = useCallback((paths: string[]) => latest.current.onShowChanges(paths), []);

  // Stops the running turn and, with a goal active, the goal too: between turns there is no
  // turn to cancel, so the goal is what keeps the agent going.
  function stop() {
    if (!thread) return;
    setError(null);
    if (thread.goal?.status === "active") stopGoal();
    if (thread.messages.some((m) => m.status === "streaming")) {
      onCancel(thread.id).catch((e: unknown) => setError(errorText(e)));
    }
  }

  function stopGoal() {
    if (!thread) return;
    setError(null);
    api.stopGoal(thread.id).catch((e: unknown) => setError(errorText(e)));
  }

  // Codex keeps a cancelled goal paused in its own session, so cancelling the turn pauses it.
  function pauseGoal() {
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
      const nextEffort = nextAgent === agent ? newOpts.effort : lastEffort(nextAgent);
      setPick({ ...newOpts, agent: nextAgent, model: nextModel, effort: nextEffort });
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

  // The plan of the latest reply, while it has one.
  const plan =
    [...(thread?.messages ?? [])].reverse().find((m) => m.role === "assistant")?.plan ?? [];
  // Choices the agent offered in its last finished reply, until the user answers or dismisses them.
  const lastMessage = thread?.messages.at(-1);
  const choiceSource =
    !working && lastMessage?.role === "assistant" && lastMessage.status === "complete"
      ? lastMessage
      : null;
  const choices = choiceSource ? parseChoices(choiceSource.text) : null;
  // A reply's choices are shown once: answering, dismissing, or leaving the thread records the reply as
  // done, so reopening the thread or restarting the app does not bring the popup back.
  const [dismissedChoices, setDismissedChoices] = useState<string | null>(null);
  const showChoices =
    choices !== null &&
    choiceSource !== null &&
    dismissedChoices !== choiceSource.id &&
    !dismissedChoiceIds().has(choiceSource.id);
  const dismissChoice = (id: string) => {
    rememberDismissedChoice(id);
    setDismissedChoices(id);
  };
  const shownChoice = useRef<string | null>(null);
  useEffect(() => {
    shownChoice.current = showChoices && choiceSource ? choiceSource.id : null;
  });
  // Leaving the thread with its choices still open counts as dismissing them.
  useEffect(
    () => () => {
      if (shownChoice.current) rememberDismissedChoice(shownChoice.current);
    },
    [thread?.id],
  );
  const answerChoice = (option: string) => {
    if (choiceSource) dismissChoice(choiceSource.id);
    void dispatch(option, [], true);
  };
  const dismissChoices = () => {
    if (choiceSource) dismissChoice(choiceSource.id);
  };
  const isNew = !thread || empty;
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
          <Messages
            key={thread.id}
            thread={thread}
            changes={changes}
            busy={working}
            onOpenCanvas={project ? openCanvas : undefined}
            onShowChanges={showChanges}
            onRetry={retry}
            tokens={tokens}
          />
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
      <div className="w-full flex-none box-border px-6 pb-4">
        {plan.length > 0 && <TasksBadge steps={plan} />}
        {permissions.length > 0 && (
          <PendingApprovalBanner approvals={permissions} onRespond={respond} />
        )}
        {asking && question && showQuestion && (
          <ChoicePanel
            key={question.id}
            question={question.question}
            options={question.options.map((o) => o.label)}
            descriptions={question.options.map((o) => o.description)}
            onPick={answerQuestion}
            onOwn={() => setOwnAnswer(asking.id)}
            onDismiss={() => respond(asking.id, false)}
          />
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
        <BackgroundTasks tasks={background} onStop={stopBackground} />
        <QueuedMessages
          messages={queued}
          canEdit={!draft.trim() && attachments.length === 0}
          onEdit={(message) => takeBack(message, true)}
          onRemove={(message) => takeBack(message, false)}
        />
        {thread?.goal && (
          <GoalBanner
            goal={thread.goal}
            working={working}
            onPause={thread.agent === "codex" ? pauseGoal : undefined}
            onResume={resumeGoal}
            onClear={stopGoal}
          />
        )}
        <Suspense
          fallback={
            <div className="h-[106px] rounded-[8px] border border-border-strong bg-editor" />
          }
        >
          <Composer
            key={key}
            value={draft}
            onChange={setDraft}
            onSubmit={submit}
            onStop={stop}
            goalActive={thread?.goal?.status === "active"}
            working={working}
            error={error}
            agent={agent}
            model={model}
            agents={empty ? offered : [agent]}
            onPickModel={pickModel}
            accessMode={accessMode}
            onPickAccess={pickAccess}
            effort={effort}
            onPickEffort={pickEffort}
            attachments={attachments}
            onRemoveAttachment={(i) => setAttachments(attachments.filter((_, j) => j !== i))}
            dropping={dropping}
            history={history}
            files={files}
            deferFocus={showChoices || showQuestion}
          />
        </Suspense>
      </div>
    </div>
  );
}
