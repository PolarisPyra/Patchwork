/** Bottom status bar: project and branch, open thread, change count, prompt mode, and agent status. */
import { CaretRightIcon, CircleIcon, CircleNotchIcon, GitBranchIcon } from "@phosphor-icons/react";
import { AGENT_LABELS, type AgentKind, type Project } from "~/api";

interface StatusBarProps {
  project: Project | null;
  branch: string | null;
  /** Title of the open thread, if one is selected. */
  threadTitle: string | null;
  changeCount: number;
  agent: AgentKind;
  working: boolean;
  /** Plan mode of the open thread; off when no thread is selected. */
  planMode: boolean;
  onOpenChanges: () => void;
}

export default function StatusBar({
  project,
  branch,
  threadTitle,
  changeCount,
  agent,
  working,
  planMode,
  onOpenChanges,
}: StatusBarProps) {
  return (
    <footer className="flex min-w-0 items-center gap-3 border-t border-border bg-app px-2.5 text-[length:var(--fs-xs)] leading-4 text-fg-muted">
      <div className="flex min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap">
        <span>{project?.name ?? "No project"}</span>
        {branch && (
          <>
            <CaretRightIcon size={12} />
            <GitBranchIcon size={12} />
            <span>{branch}</span>
          </>
        )}
      </div>
      <span className="flex-1" />
      {threadTitle && (
        <span className="inline-flex h-5 min-w-0 max-w-[40%] items-center gap-1 pr-1.5">
          <CircleIcon size={8} weight="fill" className="flex-none text-accent" />
          <span className="truncate">{threadTitle}</span>
        </span>
      )}
      <button
        className="inline-flex h-5 cursor-pointer items-center gap-1 rounded-[3px] border-none bg-transparent px-1.5 hover:bg-hover hover:text-fg"
        onClick={onOpenChanges}
      >
        {changeCount} changes
      </button>
      <span className="inline-flex h-5 items-center px-1.5">
        Chat Mode: {planMode ? "Plan" : "Default"}
      </span>
      <span className="inline-flex h-5 items-center gap-1 px-1.5">
        {working ? (
          <>
            <CircleNotchIcon size={12} className="spin" />
            {AGENT_LABELS[agent]} working…
          </>
        ) : (
          "Idle"
        )}
      </span>
    </footer>
  );
}
