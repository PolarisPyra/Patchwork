import { useState } from "react";
import { compactElapsed } from "~/lib/util";
import { formatTokens } from "~/lib/tokens";

interface ResumePromptProps {
  /** How long the thread has been idle, and how large its context is. */
  idleMs: number;
  context: number;
  /** Compact first (true) or continue (false); `dontAsk` is whether the box is ticked. */
  onAnswer: (compact: boolean, dontAsk: boolean) => void;
}

/** Above the composer, before a message is sent to a large session that has gone cold. */
export default function ResumePrompt({ idleMs, context, onAnswer }: ResumePromptProps) {
  const [dontAsk, setDontAsk] = useState(false);
  return (
    <div className="mb-1.5 flex flex-col gap-2 rounded-[var(--radius)] border border-border-strong bg-panel p-2.5 text-[length:var(--fs-sm)]">
      <p className="m-0">
        This session has been idle for {compactElapsed(idleMs)} and uses {formatTokens(context)}{" "}
        tokens. Compact before continuing?
      </p>
      <div className="flex items-center justify-between gap-1.5">
        <label className="flex cursor-pointer items-center gap-1.5 text-fg-muted">
          <input type="checkbox" checked={dontAsk} onChange={(e) => setDontAsk(e.target.checked)} />
          Don&apos;t ask again
        </label>
        <div className="flex gap-1.5">
          <button className="btn" onClick={() => onAnswer(false, dontAsk)}>
            Continue
          </button>
          <button className="btn btn-primary" onClick={() => onAnswer(true, dontAsk)}>
            Compact first
          </button>
        </div>
      </div>
    </div>
  );
}
