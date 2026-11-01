import {
  lazy,
  Suspense,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from "react";
import type { FileChange, Message } from "~/api";
import type { CanvasTab } from "~/features/canvas/Canvas";

/** A file opened beside the chat. */
export interface CanvasState {
  /** Bumped on every open so the canvas re-initialises its tab. */
  seq: number;
  projectId: string;
  path: string | null;
  message: Message | null;
  tab: CanvasTab;
}

const CANVAS_MIN_PX = 340;
const CANVAS_MAX_FRACTION = 0.7;

// The canvas (with its syntax highlighter) is only shown on demand, so it loads as a separate
// chunk instead of making the first screen wait for it. Once loaded, its module is kept and
// rendered directly: a lazy component suspends on every first render even when loaded, and
// React holds a suspended reveal for most of a second.
let canvasModule: typeof import("~/features/canvas/Canvas") | null = null;
export const loadCanvas = () => import("~/features/canvas/Canvas").then((m) => (canvasModule = m));
const LazyCanvas = lazy(loadCanvas);

/** The file panel right of the chat, with the handle that resizes it. Its width is kept while
 * it is closed and opened again. */
export default function CanvasArea({
  canvas,
  bodyRef,
  changes,
  onClose,
  onChangesUpdated,
}: {
  canvas: CanvasState | null;
  /** The row the panel sits at the right of, which bounds its width. */
  bodyRef: RefObject<HTMLDivElement | null>;
  changes: FileChange[];
  onClose: () => void;
  onChangesUpdated: () => void;
}) {
  const Canvas = canvasModule?.default ?? LazyCanvas;
  const [width, setWidth] = useState<number | null>(null);

  function startResize(e: MouseEvent) {
    const body = bodyRef.current;
    if (!body) return;
    e.preventDefault();
    const right = body.getBoundingClientRect().right;
    const max = body.clientWidth * CANVAS_MAX_FRACTION;
    const onMove = (ev: globalThis.MouseEvent) =>
      setWidth(Math.min(max, Math.max(CANVAS_MIN_PX, right - ev.clientX)));
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }

  function resizeByKey(e: KeyboardEvent) {
    const body = bodyRef.current;
    const step = e.key === "ArrowLeft" ? 16 : e.key === "ArrowRight" ? -16 : 0;
    if (!body || !step) return;
    e.preventDefault();
    const current = e.currentTarget.nextElementSibling?.getBoundingClientRect().width ?? 0;
    const max = body.clientWidth * CANVAS_MAX_FRACTION;
    setWidth(Math.min(max, Math.max(CANVAS_MIN_PX, current + step)));
  }

  if (!canvas) return null;
  return (
    <>
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize file panel"
        aria-valuenow={width === null ? undefined : Math.round(width)}
        aria-valuemin={CANVAS_MIN_PX}
        aria-valuemax={
          bodyRef.current
            ? Math.round(bodyRef.current.clientWidth * CANVAS_MAX_FRACTION)
            : undefined
        }
        tabIndex={0}
        onKeyDown={resizeByKey}
        className="relative w-1 flex-none cursor-col-resize after:absolute after:top-0 after:bottom-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-border after:content-['']"
        onMouseDown={startResize}
      />
      <div
        className="flex w-[max(420px,46%)] min-h-0 min-w-0 flex-none [&>*]:min-w-0 [&>*]:flex-1"
        style={width !== null ? { width } : undefined}
      >
        <Suspense fallback={null}>
          <Canvas
            key={canvas.seq}
            projectId={canvas.projectId}
            path={canvas.path}
            message={canvas.message}
            initialTab={canvas.tab}
            changes={changes}
            onClose={onClose}
            onChangesUpdated={onChangesUpdated}
          />
        </Suspense>
      </div>
    </>
  );
}
