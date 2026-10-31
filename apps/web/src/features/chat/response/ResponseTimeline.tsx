import { useEffect, useMemo, useState, type ReactNode } from "react";
import { type AgentKind, type Message } from "~/api";
import { isGoalClearedReply } from "~/features/chat/lib/goalPresentation";
import CompactionLine from "~/features/chat/response/components/CompactionLine";
import FoldHead from "~/features/chat/response/components/FoldHead";
import GoalClearedNotice from "~/features/chat/response/components/GoalClearedNotice";
import TurnStatus from "~/features/chat/response/components/TurnStatus";
import Subagents from "~/features/chat/response/components/Subagents";
import StepGroup from "~/features/chat/response/components/StepGroup";
import {
  type Step,
  foldHeadOf,
  foldLayoutOf,
  phaseOf,
  shownSteps,
  stepsOf,
  workKey,
} from "~/features/chat/response/lib/model";
import ProposedPlanCard from "~/features/chat/components/ProposedPlanCard";
// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.
import StreamedMarkdown from "~/features/chat/components/StreamedMarkdown";

/** Replies seen streaming in this window. The list remounts a turn scrolled away and back, so
 * this outlives the component. */
const watched = new Set<string>();

/** Replies drawn in this window. One that came in while its chat was open is revealed from its
 * first word at its first draw only, so scrolling it away and back does not replay it. */
const drawn = new Set<string>();

/**
 * A reply as it happened: text, reasoning and calls in order, under one line that says what the
 * turn is doing.
 *
 * While the reply streams, that line reports the current phase ("Working for 1m 14s", a retry, a
 * goal check) and stays at the top of the turn. Once it settles, the same line reports what it took
 * and opens the steps it folded away. The shape of the turn — its steps, its phase, its fold — is
 * worked out in `model.ts`; this component only decides what to draw and where.
 */
export default function ResponseTimeline({
  message,
  agent,
  clearsGoal = false,
  compacts = false,
  arrived = false,
  onOpenThread,
  onOpenFile,
  renderApprovals,
}: {
  message: Message;
  agent: AgentKind;
  /** The reply came in while its chat was open, so its text is revealed rather than shown whole. */
  arrived?: boolean;
  /** The reply answers a cleared goal, so it shows no working line even before its text arrives. */
  clearsGoal?: boolean;
  /** The reply answers a /compact, whose divider shows its progress instead of a working line. */
  compacts?: boolean;
  /** Opens a subagent's thread. */
  onOpenThread: (id: string) => void;
  /** Opens a file; edit rows link to their file when given. */
  onOpenFile?: (path: string) => void;
  /** The approvals and questions with these ids, shown where they were asked. */
  renderApprovals?: (ids: string[]) => ReactNode;
}) {
  const streaming = message.status === "streaming";
  if (streaming) watched.add(message.id);
  // Its first batch can already hold text, a call, even the end of the turn.
  const [live] = useState(() => arrived && !drawn.has(message.id));
  useEffect(() => void drawn.add(message.id), [message.id]);
  // A reply seen running stays open as it settles, still able to fold: folding it then took its
  // steps out from under the chat, kept at its end, and dropped every message above.
  const [open, setOpen] = useState(() => live || watched.has(message.id));
  // A streaming reply re-renders on every delta; its steps only change when its parts do.
  const steps = useMemo(() => stepsOf(message), [message.parts, message.tools, message.text]);
  // The texts already there when the turn was drawn. One that arrives later is revealed from its
  // first word even when the call after it, or the end of the turn, landed with it.
  const [drawnTexts] = useState(
    () => new Set(live ? [] : steps.flatMap((step) => (step.kind === "text" ? [step.index] : []))),
  );
  // A reply that only reports a cleared goal did no work, so it reads as a status, not a turn.
  const goalCleared =
    clearsGoal ||
    (steps.length > 0 && steps.every((s) => s.kind === "text" && isGoalClearedReply(s.text)));
  const phase = phaseOf(message, streaming);
  const didWork = steps.some((step) => step.kind === "work" || step.kind === "agents");
  const fold = foldLayoutOf(message, steps, streaming);
  const shown = shownSteps(message, steps, { ...fold, open });
  const head = foldHeadOf(message, fold.actions, open);
  const last = steps.at(-1);
  // The block still arriving settles into place; the ones before it are already read, so a chat
  // list that remounts them while scrolling does not replay their entrance.
  const arriving = streaming ? shown.at(-1) : undefined;
  const stepClass = (step: Step) => `px-1 py-0.5 pb-2${step === arriving ? " timeline-enter" : ""}`;
  return (
    <div className="flex min-w-0 flex-col">
      {/* The head of a turn, in the one place its status ever goes. */}
      {!goalCleared && !compacts && phase !== null && (
        <TurnStatus phase={phase} since={message.createdAt} />
      )}
      {/* A turn stopped before it did anything keeps the line too, saying so, so stopping does not
          take a line out of the chat and drop everything above it. */}
      {!goalCleared && !streaming && (didWork || message.status === "cancelled") && (
        <FoldHead summary={head} open={open} onToggle={() => setOpen((o) => !o)} />
      )}
      {/* Holds the head's place until the agent's reply fills it, so the chat does not grow as the
          goal banner above the composer closes. */}
      {streaming && clearsGoal && steps.length === 0 && (
        <div className="px-1 py-0.5 pb-2">
          <GoalClearedNotice text="Clearing goal" live pending />
        </div>
      )}
      {shown.map((step) => {
        switch (step.kind) {
          case "text":
            return (
              <div key={`t${step.index}`} className={stepClass(step)}>
                {goalCleared ? (
                  <GoalClearedNotice text={step.text} live={streaming && !clearsGoal} />
                ) : (
                  <StreamedMarkdown
                    text={step.text}
                    streaming={streaming && step === last && !goalCleared}
                    fresh={!drawnTexts.has(step.index)}
                  />
                )}
              </div>
            );
          case "plan":
            return (
              <div key={`p${step.index}`} className={stepClass(step)}>
                <ProposedPlanCard markdown={step.text} />
              </div>
            );
          case "approvals":
            return (
              <div key={`q${step.ids[0]}`} className="flex flex-col gap-2.5 pb-2">
                {renderApprovals?.(step.ids)}
              </div>
            );
          case "agents":
            return (
              <Subagents
                key={`a${step.tools[0]?.id}`}
                subagents={step.tools}
                calls={message.tools}
                agent={agent}
                onOpenThread={onOpenThread}
              />
            );
          // Marked once it is done; while it runs the status line says so. One the reply ended on
          // never finished. A /compact's reply already has the transcript's divider above it.
          case "compaction":
            return compacts || (streaming && message.compacting && step === last) ? null : (
              <CompactionLine
                key={`c${step.index}`}
                label={
                  step === last && message.status === "error"
                    ? "Compaction failed"
                    : step === last && message.status === "cancelled"
                      ? "Compaction stopped"
                      : "Context compacted"
                }
                summary={step.summary}
                entering={step === arriving}
              />
            );
          case "work":
            return (
              <StepGroup
                key={`w${step.entries[0] ? workKey(step.entries[0]) : step.entries.length}`}
                entries={step.entries}
                live={streaming}
                onOpenFile={onOpenFile}
              />
            );
        }
      })}
    </div>
  );
}
