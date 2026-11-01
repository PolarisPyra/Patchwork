import {
  lazy,
  memo,
  Suspense,
  useCallback,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from "react";
import type { Settings } from "~/api";
import type { UpdateSettings } from "~/api";
import type { PanelTab } from "~/features/panel/Panel";
import { useCommand } from "~/lib/shortcuts";

const PANEL_MIN_PX = 120;
const PANEL_MAX_FRACTION = 0.8;
const PANEL_DEFAULT_PX = 260;
const PANEL_HEIGHT_KEY = "patchwork-panel-height";

// The panel brings in the terminal emulator, so it loads as its own chunk, shortly after the
// workspace shows, so the first terminal opens without waiting for it.
export const loadPanel = () => import("~/features/panel/Panel");
const Panel = lazy(loadPanel);

/** The terminal and Output panel under the chat, with the handle that resizes it, and the
 * shortcuts that open, close, and switch it. It stays mounted while closed, so it keeps its tab. */
export default memo(function PanelArea({
  columnRef,
  settings,
  updateSettings,
  cwd,
}: {
  /** The column the panel sits at the bottom of, which bounds its height. */
  columnRef: RefObject<HTMLDivElement | null>;
  settings: Settings;
  updateSettings: UpdateSettings;
  cwd: string | null;
}) {
  const [panel, setPanel] = useState<{ open: boolean; tab: PanelTab }>({
    open: false,
    tab: "terminal",
  });
  const [height, setHeight] = useState(
    () => Number(localStorage.getItem(PANEL_HEIGHT_KEY)) || PANEL_DEFAULT_PX,
  );
  const [newTerminal, setNewTerminal] = useState(0);
  const [focusTerminal, setFocusTerminal] = useState(0);
  const close = useCallback(() => setPanel((p) => ({ ...p, open: false })), []);

  // As VS Code's: the terminal key opens the terminal, focuses it if it is shown elsewhere, and
  // closes the panel from inside it.
  useCommand("terminal.toggle", () => {
    const inTerminal = document.activeElement?.closest(".xterm") != null;
    if (panel.open && panel.tab === "terminal" && inTerminal) {
      setPanel((p) => ({ ...p, open: false }));
      return;
    }
    setPanel({ open: true, tab: "terminal" });
    setFocusTerminal((n) => n + 1);
  });
  useCommand("terminal.new", () => {
    setPanel({ open: true, tab: "terminal" });
    setNewTerminal((n) => n + 1);
  });
  useCommand("panel.toggle", () => setPanel((p) => ({ ...p, open: !p.open })));
  useCommand("output.toggle", () =>
    setPanel((p) =>
      p.open && p.tab === "output" ? { ...p, open: false } : { open: true, tab: "output" },
    ),
  );

  function startResize(e: MouseEvent) {
    const column = columnRef.current;
    if (!column) return;
    e.preventDefault();
    const bottom = column.getBoundingClientRect().bottom;
    const max = column.clientHeight * PANEL_MAX_FRACTION;
    let next = height;
    const onMove = (ev: globalThis.MouseEvent) => {
      next = Math.min(max, Math.max(PANEL_MIN_PX, bottom - ev.clientY));
      setHeight(next);
    };
    const onUp = () => {
      localStorage.setItem(PANEL_HEIGHT_KEY, String(Math.round(next)));
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }

  function resizeByKey(e: KeyboardEvent) {
    const column = columnRef.current;
    const step = e.key === "ArrowUp" ? 16 : e.key === "ArrowDown" ? -16 : 0;
    if (!column || !step) return;
    e.preventDefault();
    const max = column.clientHeight * PANEL_MAX_FRACTION;
    const next = Math.min(max, Math.max(PANEL_MIN_PX, height + step));
    localStorage.setItem(PANEL_HEIGHT_KEY, String(Math.round(next)));
    setHeight(next);
  }

  if (!panel.open) return null;
  return (
    <>
      <div
        role="separator"
        aria-orientation="horizontal"
        aria-label="Resize panel"
        aria-valuenow={Math.round(height)}
        aria-valuemin={PANEL_MIN_PX}
        aria-valuemax={
          columnRef.current
            ? Math.round(columnRef.current.clientHeight * PANEL_MAX_FRACTION)
            : undefined
        }
        tabIndex={0}
        onKeyDown={resizeByKey}
        className="relative z-[1] -mb-1 h-1 flex-none cursor-row-resize"
        onMouseDown={startResize}
      />
      <div className="min-h-0 flex-none" style={{ height }}>
        <Suspense fallback={null}>
          <Panel
            tab={panel.tab}
            onTab={(tab) => setPanel({ open: true, tab })}
            onClose={close}
            settings={settings}
            updateSettings={updateSettings}
            cwd={cwd}
            newTerminal={newTerminal}
            focusTerminal={focusTerminal}
          />
        </Suspense>
      </div>
    </>
  );
});
