import { useCallback, useEffect, useRef, useState } from "react";
import { CameraIcon } from "@phosphor-icons/react";
import { api, type Screenshot } from "~/api";
import Spinner from "~/components/Spinner";
import { errorText } from "~/lib/util";

/** How long Live waits after a capture finishes before taking the next. A capture itself takes
 * about four seconds on Wayland, so a fixed timer would pile them up. */
const LIVE_GAP_MS = 1000;

/** The screen as an agent with computer use sees it. It only looks: every click and keystroke
 * stays with the agent, approved in the chat. */
export default function ScreenView({ enabled }: { enabled: boolean }) {
  const [shot, setShot] = useState<Screenshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [live, setLive] = useState(false);
  const [shownWidth, setShownWidth] = useState(0);
  const image = useRef<HTMLImageElement>(null);

  const capture = useCallback(async () => {
    setBusy(true);
    try {
      setShot(await api.computerScreenshot());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (enabled) void capture();
  }, [enabled, capture]);

  // Live captures again a beat after each capture finishes.
  useEffect(() => {
    if (!enabled || !live || busy) return;
    const timer = setTimeout(() => void capture(), LIVE_GAP_MS);
    return () => clearTimeout(timer);
  }, [enabled, live, busy, capture]);

  useEffect(() => {
    const el = image.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setShownWidth(el.clientWidth));
    observer.observe(el);
    return () => observer.disconnect();
  }, [shot !== null]);

  if (!enabled) {
    return (
      <div className="flex h-full flex-1 items-center justify-center px-3 text-[length:var(--fs-sm)] text-fg-muted">
        Computer use is off. Turn it on under Settings → Permissions.
      </div>
    );
  }

  const scale = shot && shownWidth > 0 ? Math.round((shownWidth / shot.screenWidth) * 100) : 100;
  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="flex h-7 flex-none items-center gap-1.5 px-3 text-[length:var(--fs-sm)]">
        <button className="btn" disabled={busy} onClick={() => void capture()}>
          {busy ? <Spinner /> : <CameraIcon size={13} />}
          Capture
        </button>
        <button
          className="btn aria-pressed:bg-selected-muted aria-pressed:text-fg"
          aria-pressed={live}
          onClick={() => setLive(!live)}
        >
          Live
        </button>
        {error ? (
          <span className="min-w-0 truncate text-danger" title={error}>
            {error}
          </span>
        ) : (
          shot && (
            <span className="min-w-0 truncate text-fg-muted">
              {shot.screenWidth}×{shot.screenHeight}
              {scale !== 100 && ` · shown at ${scale}%`}
            </span>
          )
        )}
      </div>
      <div className="flex min-h-0 flex-1 items-center justify-center overflow-hidden px-3 pb-2">
        {shot && (
          <img
            ref={image}
            src={shot.dataUrl}
            alt="The screen as the agent sees it"
            className="max-h-full max-w-full rounded-[var(--radius-sm)] border border-border"
            onLoad={(e) => setShownWidth(e.currentTarget.clientWidth)}
          />
        )}
      </div>
    </div>
  );
}
