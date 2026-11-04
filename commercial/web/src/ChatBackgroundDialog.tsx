import { useEffect, useRef, useState } from "react";
import { XIcon } from "@phosphor-icons/react";
import type { ChatBackground, Thread } from "~/api";
import BackgroundPicker from "~/components/BackgroundPicker";
import { useDismiss } from "~/hooks/useDismiss";
import { useFeature } from "~/hooks/useLicense";
import { displayTitle, errorText } from "~/lib/util";

interface ChatBackgroundDialogProps {
  thread: Thread;
  /** The background every chat shows unless it has its own. */
  shared: ChatBackground;
  /** Saves the chat's own background; null goes back to the shared one. */
  onSave: (background: ChatBackground | null) => Promise<void>;
  /** Opens Settings → License, for a plan without per-chat backgrounds. */
  onOpenLicense: () => void;
  onClose: () => void;
}

/** Picks a background for one chat, which the Basic plan unlocks. */
export default function ChatBackgroundDialog({
  thread,
  shared,
  onSave,
  onOpenLicense,
  onClose,
}: ChatBackgroundDialogProps) {
  const unlocked = useFeature("chatBackgrounds");
  const [value, setValue] = useState(thread.background);
  const [error, setError] = useState<string | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  useDismiss(panel, true, onClose);
  useEffect(() => panel.current?.focus(), []);
  const pick = (background: ChatBackground | null) => {
    const before = value;
    setValue(background);
    setError(null);
    onSave(background).catch((e) => {
      setValue(before);
      setError(errorText(e));
    });
  };
  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-[rgba(0,0,0,0.4)]">
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label={`Background of ${displayTitle(thread.title)}`}
        tabIndex={-1}
        className="flex w-[480px] max-w-[calc(100vw_-_48px)] flex-col gap-3 rounded-[var(--radius-lg)] border border-border-strong bg-elevated p-4 shadow-[var(--shadow-popup)] outline-none"
      >
        <div className="flex items-center justify-between">
          <h2 className="m-0 truncate text-[length:var(--fs-lg)] font-semibold">
            {displayTitle(thread.title)}
          </h2>
          <button className="btn-icon" title="Close" aria-label="Close" onClick={onClose}>
            <XIcon size={16} />
          </button>
        </div>
        {!unlocked && (
          <div className="flex items-center justify-between gap-3 rounded-[var(--radius-sm)] bg-accent-soft px-3 py-2">
            <span>A background for each chat comes with the Basic plan.</span>
            <button type="button" className="btn btn-primary flex-none" onClick={onOpenLicense}>
              Enter license key
            </button>
          </div>
        )}
        <BackgroundPicker
          label="Background of this chat"
          value={unlocked ? value : null}
          shared={shared}
          disabled={!unlocked}
          onChange={pick}
        />
        {error && (
          <div role="alert" className="text-[length:var(--fs-sm)] text-danger">
            {error}
          </div>
        )}
      </div>
    </div>
  );
}
