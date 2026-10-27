import { memo } from "react";
import { ArchiveIcon, ChatsIcon, GearSixIcon, GitDiffIcon, type Icon } from "@phosphor-icons/react";
import { keyLabel, useKeybindings } from "~/lib/shortcuts";
import type { SidebarView } from "./Sidebar";

/** The activity bar: the column of icons left of the sidebar, one per sidebar view. */
const ACTIVITIES: { view: SidebarView; label: string; Icon: Icon; command: string }[] = [
  { view: "tasks", label: "Tasks", Icon: ChatsIcon, command: "sidebar.showTasks" },
  { view: "changes", label: "Changes", Icon: GitDiffIcon, command: "sidebar.showChanges" },
  { view: "archived", label: "Archived", Icon: ArchiveIcon, command: "sidebar.showArchived" },
];

interface ActivityBarProps {
  view: SidebarView | null;
  onSelect: (view: SidebarView) => void;
  onOpenSettings: () => void;
}

function ActivityBar({ view, onSelect, onOpenSettings }: ActivityBarProps) {
  const { rules } = useKeybindings();
  return (
    <nav className="flex w-[var(--activity-bar-w)] flex-none flex-col items-center gap-1 border-r border-border bg-app py-1.5">
      {ACTIVITIES.map(({ view: v, label, Icon, command }) => (
        <button
          key={v}
          className={`grid h-7 w-7 cursor-pointer place-items-center rounded-[var(--radius)] border-none p-0 ${view === v ? "bg-selected text-fg" : "bg-transparent text-fg-muted hover:bg-hover hover:text-fg"}`}
          title={keyLabel(rules, command) ? `${label} (${keyLabel(rules, command)})` : label}
          aria-label={label}
          aria-pressed={view === v}
          onClick={() => onSelect(v)}
        >
          <Icon size={20} />
        </button>
      ))}
      <button
        className="mt-auto grid h-7 w-7 cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-transparent p-0 text-fg-muted hover:bg-hover hover:text-fg"
        title="Settings"
        aria-label="Settings"
        onClick={onOpenSettings}
      >
        <GearSixIcon size={20} />
      </button>
    </nav>
  );
}

// Its parent re-renders as replies stream; nothing it shows changes then.
export default memo(ActivityBar);
