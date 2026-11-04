import { useState } from "react";
import { CheckCircleIcon, KeyIcon, XCircleIcon } from "@phosphor-icons/react";
import { api, type Approval, type SecretStatus } from "~/api";
import { errorText } from "~/lib/util";

const OUTCOME: Record<SecretStatus, string> = {
  pending: "Waiting",
  saved: "Saved",
  declined: "Declined",
  cancelled: "Cancelled",
  timedOut: "Timed out",
};

/**
 * The agent's request for a secret. While it waits, a private field takes the value, which goes
 * straight to the backend and is never shown, kept in the chat, or seen by the agent; once
 * answered, a line with its outcome.
 */
export function SecretCard({ threadId, approval }: { threadId: string; approval: Approval }) {
  const { title, detail, secret } = approval;
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const status = secret?.status ?? "cancelled";

  if (status !== "pending") {
    return (
      <div className="flex min-w-0 items-center gap-1.5 px-1 text-[length:var(--fs-sm)] text-fg-muted">
        {status === "saved" ? (
          <CheckCircleIcon size={15} className="flex-none text-success" />
        ) : (
          <XCircleIcon size={15} className="flex-none text-danger" />
        )}
        <span className="min-w-0 truncate text-fg">{title}</span>
        <span className="flex-none">· {OUTCOME[status]}</span>
      </div>
    );
  }

  function answer(secret: string | null) {
    setBusy(true);
    setError(null);
    api.answerSecret(threadId, approval.id, secret).then(
      () => setValue(""),
      (e: unknown) => {
        setBusy(false);
        setError(errorText(e));
      },
    );
  }

  return (
    <form
      aria-label={title}
      className="flex flex-col gap-2 rounded-[var(--radius)] border border-warning bg-panel p-2.5 text-[length:var(--fs-sm)]"
      onSubmit={(e) => {
        e.preventDefault();
        if (value) answer(value);
      }}
    >
      <div className="flex items-center gap-1.5 text-warning">
        <KeyIcon size={15} className="flex-none" />
        <span className="min-w-0 truncate font-semibold text-fg">{title}</span>
      </div>
      {detail && <p className="m-0 text-fg-muted selectable">{detail}</p>}
      {/* A text field drawn as dots rather than a password field, so the browser and password
          managers neither offer nor save it. */}
      <input
        className="input min-w-0 font-[family-name:var(--font-mono)] [-webkit-text-security:disc]"
        type="text"
        aria-label={title}
        placeholder={secret?.placeholder ?? undefined}
        value={value}
        disabled={busy}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        data-1p-ignore
        data-lpignore="true"
        data-bwignore
        onChange={(e) => setValue(e.target.value)}
      />
      {error && (
        <p role="alert" className="m-0 text-danger">
          {error}
        </p>
      )}
      <div className="flex justify-end gap-1.5">
        <button type="button" className="btn" disabled={busy} onClick={() => answer(null)}>
          Decline
        </button>
        <button type="submit" className="btn btn-primary" disabled={busy || !value}>
          Save securely
        </button>
      </div>
    </form>
  );
}
