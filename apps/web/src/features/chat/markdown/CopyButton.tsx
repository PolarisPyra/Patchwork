import { useState } from "react";
import { CheckIcon, CopyIcon, XIcon } from "@phosphor-icons/react";
import { copyText } from "~/lib/clipboard";

export const ACTION_CLASS =
  "inline-flex h-6 cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-none bg-transparent px-1.5 text-[length:var(--fs-xs)] text-fg-muted enabled:hover:bg-hover enabled:hover:text-fg disabled:cursor-default disabled:opacity-50";

export function CopyButton({
  text,
  className = ACTION_CLASS,
}: {
  text: string;
  className?: string;
}) {
  const [result, setResult] = useState<"copied" | "failed" | null>(null);
  const copy = () => {
    copyText(text).then(
      () => setResult("copied"),
      () => setResult("failed"),
    );
    setTimeout(() => setResult(null), 1500);
  };
  return (
    <button className={className} onClick={copy} title="Copy">
      {result === "copied" ? (
        <CheckIcon size={13} />
      ) : result === "failed" ? (
        <XIcon size={13} className="text-danger" />
      ) : (
        <CopyIcon size={13} />
      )}
      <span aria-live="polite">
        {result === "copied" ? "Copied" : result === "failed" ? "Couldn't copy" : "Copy"}
      </span>
    </button>
  );
}
