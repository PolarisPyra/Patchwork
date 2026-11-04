import { memo, useEffect, useMemo, useRef, useState, type MutableRefObject } from "react";
import { SquareIcon } from "@phosphor-icons/react";
import { EditorContent } from "@tiptap/react";
import { NodeSelection } from "@tiptap/pm/state";
import type { AccessMode, AgentKind, Attachment, ProviderSlashCommand } from "~/api";
import AccessPicker from "~/components/AccessPicker";
import CompactPicker from "~/components/CompactPicker";
import EffortPicker, { modelEfforts } from "~/components/EffortPicker";
import { useDismiss } from "~/hooks/useDismiss";
import AttachmentChip from "~/features/chat/AttachmentChip";
import { rematchChips } from "~/features/chat/lib/composerChips";
import { continueList } from "~/features/chat/lib/composerExtensions";
import AgentPicker from "~/features/chat/composer/components/AgentPicker";
import AttachButtons from "~/features/chat/composer/components/AttachButtons";
import FormatBar from "~/features/chat/composer/components/FormatBar";
import MentionMenu from "~/features/chat/composer/components/MentionMenu";
import SendButton from "~/features/chat/composer/components/SendButton";
import SlashMenu from "~/features/chat/composer/components/SlashMenu";
import { useComposerEditor } from "~/features/chat/composer/hooks/useComposerEditor";
import { useComposerFocus } from "~/features/chat/composer/hooks/useComposerFocus";
import { useDraftInserts } from "~/features/chat/composer/hooks/useDraftInserts";
import { useHistoryRecall } from "~/features/chat/composer/hooks/useHistoryRecall";
import { useMentionMenu } from "~/features/chat/composer/hooks/useMentionMenu";
import { useModels } from "~/features/chat/composer/hooks/useModels";
import { useSlashMenu } from "~/features/chat/composer/hooks/useSlashMenu";
import ModelPicker from "~/features/chat/composer/modelPicker/ModelPicker";
import type { DropMentions, HistoryEntry } from "~/features/chat/composer/types";

interface ComposerProps {
  /** The draft as Markdown. */
  value: string;
  onChange: (text: string) => void;
  /** Sends the draft; `queue` holds it for after the running turn instead of steering the turn. */
  onSubmit: (queue: boolean) => void;
  /** Promotes the first queued message while preserving the current draft. */
  onSteerQueued: () => void;
  onEditLatestQueued: () => void;
  canSteerQueued: boolean;
  editingQueued: boolean;
  canQueueFollowUp: boolean;
  onStop: () => void;
  working: boolean;
  /** The agent left commands running after its reply, which Stop ends with it. */
  backgroundRunning: boolean;
  error: string | null;
  agent: AgentKind;
  model: string | null;
  /** Agents whose models are offered (only the thread's own once it has started). */
  agents: AgentKind[];
  /** Picking another agent's model switches the agent too. */
  onPickModel: (agent: AgentKind, model: string | null) => void;
  modelPickerRequest: number;
  providerSlashCommands: ProviderSlashCommand[];
  onSelectSlashCommand: (name: string) => boolean;
  accessMode: AccessMode;
  onPickAccess: (mode: AccessMode) => void;
  effort: string | null;
  onPickEffort: (effort: string | null) => void;
  /** The chat's own context size to compact at, or null to follow Settings. */
  compactWindow: number | null;
  /** The size Settings gives the agent. */
  defaultCompactWindow: number;
  /** The chat's context now, or null before a reply has reported it. */
  context: number | null;
  onPickCompactWindow: (window: number | null) => void;
  /** Compacts the chat now; null when it cannot be. */
  onCompactNow: (() => void) | null;
  attachments: Attachment[];
  /** Adds the files or folders at these paths to the draft's attachments. */
  onAttachPaths: (paths: string[]) => void;
  onRemoveAttachment: (index: number) => void;
  /** A drag is hovering over the chat; the composer shows where it will land. */
  dropping: boolean;
  /** Set by the composer, so the chat can put dropped files, folders and images into the draft. */
  dropMentions: MutableRefObject<DropMentions | null>;
  /** The thread's goal is active: its border turns instead of holding still. */
  goalActive: boolean;
  /** The chat's earlier messages, oldest first, recalled with the arrow keys. */
  history: HistoryEntry[];
  /** Project files an @ mention can refer to, relative to the project root. */
  files: string[];
  /** Opens a file mentioned in the draft beside the chat; null without a project. */
  onOpenFile: ((path: string) => void) | null;
  /** Set by the composer, so the chat can add text quoted from a reply to the end of the draft. */
  quoteInto: MutableRefObject<((lines: string[]) => void) | null>;
  /** A choice panel is open: the editor is read-only until the user picks writing their own answer. */
  deferFocus: boolean;
  /** Nothing can be written or sent, such as while the project's folder is missing. */
  disabled: boolean;
  /** The draft is on its way to the agent: it stays, read-only, until the agent has taken it. */
  sending: boolean;
  /** Enter steers a running reply; otherwise it queues the message for after it. */
  steerWhileWorking: boolean;
  /** The chat is compacting its context: a message can be written, but not sent until it ends. */
  compacting: boolean;
  /** The project, whose own custom agents can be summoned too. */
  projectId: string | null;
  /** The custom agent the chat is in the role of. */
  role: string | null;
  onPickRole: (role: string | null) => void;
}

/** Most characters a prompt can hold; a longer draft cannot be sent. */
const PROMPT_LIMIT = 120_000;

function Composer({
  value,
  onChange,
  onSubmit,
  onSteerQueued,
  onEditLatestQueued,
  canSteerQueued,
  editingQueued,
  canQueueFollowUp,
  onStop,
  working,
  backgroundRunning,
  error,
  agent,
  model,
  agents,
  onPickModel,
  modelPickerRequest,
  providerSlashCommands,
  onSelectSlashCommand,
  accessMode,
  onPickAccess,
  effort,
  onPickEffort,
  compactWindow,
  defaultCompactWindow,
  context,
  onPickCompactWindow,
  onCompactNow,
  attachments,
  onAttachPaths,
  onRemoveAttachment,
  dropping,
  dropMentions,
  goalActive,
  history,
  files,
  onOpenFile,
  quoteInto,
  deferFocus,
  disabled,
  sending,
  steerWhileWorking,
  compacting,
  projectId,
  role,
  onPickRole,
}: ComposerProps) {
  const models = useModels(agents);
  // The model's own effort levels, or null when its CLI does not report them.
  const levels = useMemo(() => modelEfforts(agent, model, models), [agent, model, models]);
  // How much the model holds, so a chat that compacts when full still shows how full it is.
  const fullWindow = models[agent]?.find((option) => option.id === model)?.contextWindow ?? null;
  // A level this model does not take would make its CLI refuse the turn, so it falls back to the
  // CLI's own default instead of being sent.
  useEffect(() => {
    if (levels && effort && !levels.includes(effort)) onPickEffort(null);
  }, [levels, effort, onPickEffort]);

  const slash = useSlashMenu({
    agent,
    projectId,
    providerSlashCommands,
    onChange,
    onSelectSlashCommand,
  });
  const mention = useMentionMenu(files);
  const overBy = value.length - PROMPT_LIMIT;

  // Holds the next message for after the running turn. Each send puts it back to the default:
  // queue, unless the setting makes Enter steer a running reply.
  // While an autonomous goal is running, new follow-ups wait by default. Sending remains
  // explicit: Enter or the send button enqueues the current draft, and the user can switch to
  // Steer before sending when they want to interrupt the current step.
  const queueByDefault = goalActive || !steerWhileWorking;
  const [queueNext, setQueueNext] = useState(queueByDefault);
  const [dispatchMenuOpen, setDispatchMenuOpen] = useState(false);
  const dispatchRef = useRef<HTMLDivElement>(null);
  useDismiss(dispatchRef, dispatchMenuOpen, () => setDispatchMenuOpen(false));
  useEffect(() => {
    setQueueNext(queueByDefault);
    setDispatchMenuOpen(false);
  }, [working, goalActive, queueByDefault]);

  const editor = useComposerEditor({
    value,
    mentionSuggestion: mention.suggestion,
    isSkill: (name) => slash.skillsRef.current.some((skill) => skill.name === name),
    isAgent: (name) => slash.customAgentsRef.current.some((custom) => custom.name === name),
    isCommand: (name) => slash.agentCommandNames.current.has(name),
    onKeyDown: handleKey,
    onOpenFile,
    onUpdate: (editor) => {
      slash.draftChanged(editor);
      onChange(editor.getMarkdown());
    },
    onSelectionUpdate: slash.selectionChanged,
  });

  const surfaceRef = useRef<HTMLDivElement>(null);
  useDraftInserts(editor, surfaceRef, dropMentions, quoteInto);

  // A draft changed from outside (history, a completed command, sending) loads into the editor.
  // Edits the user typed already match, so typing never reloads the document.
  useEffect(() => {
    if (!editor || editor.getMarkdown() === value) return;
    editor.commands.setContent(value, { contentType: "markdown", emitUpdate: false });
    rematch();
    if (editor.isFocused) editor.commands.focus("end");
  }, [editor, value]);

  // Commands, skills, agents and files written as text become chips once their names are known:
  // a draft loaded from history or kept from before a reload, or one typed before the lists came.
  function rematch() {
    if (!editor) return;
    rematchChips(editor, {
      isCommand: (name) => slash.agentCommandNames.current.has(name),
      isSkill: (name) => slash.skillsRef.current.some((skill) => skill.name === name),
      isAgent: (name) => slash.customAgentsRef.current.some((custom) => custom.name === name),
      isFile: mention.hasFile,
    });
  }
  const commandNames = [...slash.agentCommandNames.current].join(" ");
  useEffect(rematch, [editor, slash.skills, slash.customAgents, files, commandNames]);

  useComposerFocus(editor, { deferFocus, disabled, sending, working });

  const stepHistory = useHistoryRecall({
    history,
    value,
    hasAttachments: attachments.length > 0,
    onChange,
  });

  function handleKey(event: KeyboardEvent): boolean {
    if (!editor || event.isComposing) return false;
    // Enter with Alt or AltGr, with Ctrl and Cmd together, or with Shift and Ctrl or Cmd is left to
    // the editor, so a chord meant for something else never sends or steers.
    if (
      event.key === "Enter" &&
      (event.altKey ||
        event.getModifierState("AltGraph") ||
        (event.ctrlKey && event.metaKey) ||
        (event.shiftKey && (event.ctrlKey || event.metaKey)))
    ) {
      return false;
    }
    // Same as clicking the queued message's Steer button. The draft in the composer is not sent.
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey) && canSteerQueued) {
      onSteerQueued();
      return true;
    }
    if (mention.ownsKey(event)) return false;
    if (slash.menuOpen ? slash.handleKey(editor, event) : stepHistory(event, editor)) return true;
    if (event.key === "ArrowUp" && event.altKey && !value.trim()) {
      onEditLatestQueued();
      return true;
    }
    // Escape leaves the composer, so the page's keys work again. While a turn runs, it stays, so
    // a second Escape still reaches the chat and stops the turn.
    if (event.key === "Escape" && !working) {
      editor.commands.blur();
      return true;
    }
    // Enter on a file chip picked out with Backspace or Delete opens the file, as a click does.
    const { selection } = editor.state;
    const picked = selection instanceof NodeSelection ? selection.node : null;
    if (event.key === "Enter" && picked?.type.name === "mention" && onOpenFile) {
      const path = String(picked.attrs.id);
      if (!path.endsWith("/")) {
        onOpenFile(path);
        return true;
      }
    }
    // Enter sends, unless the draft is over the limit; Shift+Enter continues a list, or starts a
    // new line (the editor's default) elsewhere.
    if (event.key === "Enter" && !event.shiftKey) {
      if (overBy <= 0) sendDraft(working && (event.metaKey || event.ctrlKey));
      return true;
    }
    if (event.key === "Enter" && continueList(editor)) return true;
    return false;
  }

  function sendDraft(alternate = false) {
    const queue = working && (alternate ? !queueNext : queueNext);
    if (sending || (compacting && !editingQueued)) return;
    if (queue && !canQueueFollowUp && !editingQueued) return;
    onSubmit(queue);
    setQueueNext(queueByDefault);
    setDispatchMenuOpen(false);
  }

  // Picks how the next send is dispatched. Enter and the send button use it; nothing is sent here.
  function chooseMode(queue: boolean) {
    setQueueNext(queue);
    setDispatchMenuOpen(false);
  }

  const empty = !value.trim() && attachments.length === 0;
  const surface = dropping
    ? "border border-dashed border-accent bg-editor"
    : goalActive
      ? "goal-border"
      : disabled
        ? "border border-border-strong bg-editor"
        : "border border-border-strong bg-editor focus-within:border-accent";
  return (
    <div
      ref={surfaceRef}
      className={`relative flex flex-col gap-2 rounded-[var(--radius-lg)] pt-3.5 pr-3 pb-2.5 pl-3 ${surface}`}
      // The frame's padding and gaps belong to the text field: a click there puts the caret back
      // where it was in the draft, so Ctrl+A still selects the draft.
      onMouseDown={(e) => {
        if (e.target !== e.currentTarget || !editor) return;
        e.preventDefault();
        if (!editor.isFocused) editor.commands.focus();
      }}
    >
      {slash.menuOpen && (
        <SlashMenu
          kind={slash.kind}
          matches={slash.matches}
          active={slash.activeCommand?.name}
          activeRef={slash.optionRef}
          sigil={slash.token?.raw[0] ?? null}
          onSelect={(name) => slash.select(editor, name)}
        />
      )}
      {mention.menu && <MentionMenu menu={mention.menu} onPick={mention.pick} />}
      {attachments.length > 0 && (
        <div className="flex flex-wrap gap-1.5 px-1.5">
          {attachments.map((a, i) => (
            <AttachmentChip key={a.path} attachment={a} onRemove={() => onRemoveAttachment(i)} />
          ))}
        </div>
      )}
      <EditorContent editor={editor} className={`composer-editor min-w-0 `} />
      <div
        role="toolbar"
        aria-label="Composer controls"
        className="@container flex flex-wrap items-center gap-1.5"
      >
        <ModelPicker
          agent={agent}
          model={model}
          agents={agents}
          models={models}
          onPick={onPickModel}
          openRequest={modelPickerRequest}
        />
        <EffortPicker agent={agent} effort={effort} levels={levels} onPick={onPickEffort} />
        <CompactPicker
          agent={agent}
          window={compactWindow}
          fallback={defaultCompactWindow}
          full={fullWindow}
          context={context}
          onPick={onPickCompactWindow}
          onCompactNow={onCompactNow}
        />
        <AccessPicker mode={accessMode} onPick={onPickAccess} />
        <AgentPicker projectId={projectId} role={role} onPick={onPickRole} />
        <span className="flex-1" />
        {/* Hidden when the composer is too narrow for the pickers and the formatting buttons together. */}
        <div className={role ? "hidden @[780px]:block" : "hidden @[640px]:block"}>
          <FormatBar editor={editor} />
        </div>
        <AttachButtons disabled={disabled || sending} onAttachPaths={onAttachPaths} />
        {/* Commands the agent left running keep its process up after the reply, so Stop stays
            until they end. */}
        {(working || backgroundRunning) && (
          <button
            className="grid h-8 w-8 flex-none cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-selected-muted text-fg enabled:hover:bg-accent-hover"
            onClick={onStop}
            title={working ? "Stop (Esc Esc)" : "Stop background commands"}
            aria-label={working ? "Stop" : "Stop background commands"}
          >
            <SquareIcon size={12} weight="fill" />
          </button>
        )}
        {(!working || !empty) && (
          <SendButton
            working={working}
            editingQueued={editingQueued}
            sending={sending}
            compacting={compacting}
            disabled={disabled}
            empty={empty}
            overLimit={overBy > 0}
            canQueueFollowUp={canQueueFollowUp}
            queueNext={queueNext}
            menuOpen={dispatchMenuOpen}
            menuRef={dispatchRef}
            onMenuOpen={setDispatchMenuOpen}
            onChooseMode={chooseMode}
            onSend={() => sendDraft()}
          />
        )}
      </div>
      {error && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {error}
        </div>
      )}
      {overBy > 0 && (
        <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
          {`Prompt is ${overBy.toLocaleString("en-US")} ${overBy === 1 ? "character" : "characters"} over the ${PROMPT_LIMIT.toLocaleString("en-US")}-character limit. Shorten or split it before sending.`}
        </div>
      )}
    </div>
  );
}

// The chat re-renders as its reply streams; the editor only needs to when its own props change.
export default memo(Composer);
