import { FolderIcon } from "@phosphor-icons/react";
import { STRIP_ACTION } from "~/components/chat/GoalBanner";

interface MissingFolderPromptProps {
  path: string;
  onChangeFolder: () => void;
}

/** Above the composer, while the project's folder has been moved or deleted. */
export default function MissingFolderPrompt({ path, onChangeFolder }: MissingFolderPromptProps) {
  return (
    <div className="composer-strip mx-2 mb-1.5 overflow-hidden rounded-[var(--radius-sm)] border border-border-strong bg-panel text-[length:var(--fs-sm)] shadow-sm">
      <div className="flex h-9 min-w-0 items-center gap-2 bg-app px-2.5">
        <FolderIcon size={13} className="flex-none text-fg-subtle" />
        <span className="flex-none whitespace-nowrap font-semibold text-fg">Folder not found</span>
        <span
          className="min-w-0 flex-1 truncate text-[length:var(--fs-xs)] text-fg-subtle"
          title={path}
        >
          {path}
        </span>
        <button type="button" className={STRIP_ACTION} onClick={onChangeFolder}>
          Change folder
        </button>
      </div>
    </div>
  );
}
