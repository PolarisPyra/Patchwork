import { memo, useState, type MouseEvent } from "react";
import { LockOpenIcon, LockSimpleIcon, MagnifyingGlassIcon } from "@phosphor-icons/react";
import ContextMenu from "~/components/ContextMenu";
import ArchivedView from "~/features/sidebar/components/ArchivedView";
import ChangesView from "~/features/sidebar/components/ChangesView";
import TasksView from "~/features/sidebar/components/TasksView";
import type { SidebarProps, SidebarView, Target } from "~/features/sidebar/types";
import UsagePopover from "~/features/usage/UsagePopover";
import { formatKey, keyFor, useKeybindings } from "~/lib/shortcuts";

export type { SidebarProps, SidebarView, Target };

/**
 * The sidebar: its width, kept between launches and dragged on its right edge, the search button,
 * and the view it shows. Each view is its own file — the task list, the changes, and the archive —
 * and the parts they share are in `tree.tsx` and `targets.ts`.
 */

/** The sidebar's width: dragged on its right edge, kept between launches, reset by a double-click. */
const SIDEBAR_WIDTH = 300;
const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 480;
const SIDEBAR_WIDTH_KEY = "patchwork.sidebarWidth";
const SIDEBAR_LOCKED_KEY = "patchwork.sidebarLocked";
const clampWidth = (width: number) => Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, width));

export default memo(function Sidebar(props: SidebarProps) {
  const { rules } = useKeybindings();
  const searchKey = keyFor(rules, "palette.toggle");
  const [width, setWidth] = useState(() =>
    clampWidth(Number(localStorage.getItem(SIDEBAR_WIDTH_KEY)) || SIDEBAR_WIDTH),
  );
  const [locked, setLocked] = useState(() => localStorage.getItem(SIDEBAR_LOCKED_KEY) === "1");
  const [widthMenu, setWidthMenu] = useState<{ x: number; y: number } | null>(null);
  function toggleLocked() {
    localStorage.setItem(SIDEBAR_LOCKED_KEY, locked ? "0" : "1");
    setLocked(!locked);
  }
  function startResize(e: MouseEvent) {
    if (e.button !== 0 || locked) return;
    e.preventDefault();
    const left = e.currentTarget.parentElement?.getBoundingClientRect().left ?? 0;
    // Saved on every move, not only on release: a release outside the window may never reach here.
    const onMove = (ev: globalThis.MouseEvent) => {
      const next = clampWidth(ev.clientX - left);
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(next));
      setWidth(next);
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }
  function resetWidth() {
    if (locked) return;
    localStorage.removeItem(SIDEBAR_WIDTH_KEY);
    setWidth(SIDEBAR_WIDTH);
  }
  return (
    <aside
      className="relative flex min-h-0 flex-none flex-col border-r border-border bg-panel text-[length:var(--fs-md)] [--fs-xs:12px] [--fs-sm:13px] [--fs-md:13px]"
      style={{ width, flex: `0 0 ${width}px` }}
      onContextMenu={(e) => {
        // Rows open their own menus and prevent default; only the sidebar's bare area gets this one.
        if (e.defaultPrevented) return;
        e.preventDefault();
        setWidthMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        className={`absolute top-0 -right-[3px] bottom-0 z-10 w-1.5 ${locked ? "" : "cursor-col-resize hover:bg-accent/40"}`}
        onMouseDown={startResize}
        onDoubleClick={resetWidth}
      />
      {widthMenu && (
        <ContextMenu
          x={widthMenu.x}
          y={widthMenu.y}
          items={[
            {
              label: locked ? "Unlock width" : "Lock width",
              Icon: locked ? LockOpenIcon : LockSimpleIcon,
              onSelect: toggleLocked,
            },
          ]}
          onClose={() => setWidthMenu(null)}
        />
      )}
      <button
        className="mx-2 mt-3 mb-0.5 flex h-8 flex-none cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border border-border bg-input px-2 text-[length:var(--fs-sm)] text-fg-muted hover:border-border-strong hover:text-fg"
        onClick={props.onOpenPalette}
      >
        <MagnifyingGlassIcon size={18} />
        Search…
        {searchKey && <kbd className="kbd ml-auto">{formatKey(searchKey).join(" ")}</kbd>}
      </button>
      {props.view === "tasks" && <TasksView {...props} />}
      {props.view === "changes" && <ChangesView {...props} />}
      {props.view === "archived" && <ArchivedView {...props} />}
      <UsagePopover />
    </aside>
  );
});
