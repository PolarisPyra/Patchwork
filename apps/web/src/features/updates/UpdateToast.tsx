import { useEffect, useState } from "react";
import { ArrowCircleUpIcon, XIcon } from "@phosphor-icons/react";
import { api, onUpdateStatus, type UpdateStatus } from "~/api";
import { RELEASES_URL, restartToUpdate } from "~/lib/updates";
import { errorText } from "~/lib/util";

/** The version whose card the user closed, so each version shows once. */
const DISMISSED_KEY = "patchwork:update-dismissed";

/** A card in the top right once a new version is installed (restart to use it) or, for a copy
 * that cannot install updates itself, out (download it). */
export default function UpdateToast() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [dismissed, setDismissed] = useState(() => localStorage.getItem(DISMISSED_KEY));
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const unlisten = onUpdateStatus((next) => {
      if (live) setStatus(next);
    });
    // An event that arrived first is newer than this reply.
    api.updateStatus().then(
      (next) => {
        if (live) setStatus((current) => current ?? next);
      },
      () => {},
    );
    return () => {
      live = false;
      void unlisten.then((stop) => stop());
    };
  }, []);

  const state = status?.state;
  if (state?.status !== "ready" && state?.status !== "available") return null;
  if (dismissed === state.version) return null;
  const ready = state.status === "ready";

  const dismiss = () => {
    localStorage.setItem(DISMISSED_KEY, state.version);
    setDismissed(state.version);
  };
  const act = () => {
    setError(null);
    const action = ready ? restartToUpdate() : api.openUrl(RELEASES_URL);
    action.catch((e: unknown) => setError(errorText(e)));
  };

  return (
    <div
      role="status"
      className="update-card-in fixed top-12 right-3 z-50 flex max-w-[calc(100vw_-_24px)] flex-col gap-1.5 rounded-[8px] border border-border-strong bg-elevated p-2.5 text-[length:var(--fs-md)] text-fg shadow-[var(--shadow-popup)]"
    >
      <div className="flex items-center gap-2">
        <ArrowCircleUpIcon size={18} className="flex-none text-accent" />
        <span className="min-w-0 flex-1 truncate font-semibold">
          Patchwork {state.version} {ready ? "is ready" : "is out"}
        </span>
        <button className="btn btn-primary flex-none" onClick={act}>
          {ready ? "Restart" : "Download"}
        </button>
        <button className="btn-icon flex-none" aria-label="Dismiss" onClick={dismiss}>
          <XIcon size={14} />
        </button>
      </div>
      {error && <div className="text-[length:var(--fs-sm)] text-danger">{error}</div>}
    </div>
  );
}
