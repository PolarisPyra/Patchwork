import { FolderIcon, PaperclipIcon } from "@phosphor-icons/react";
import { open } from "@tauri-apps/plugin-dialog";

/** A toolbar button that opens a native picker. */
function AttachButton({
  title,
  Icon,
  disabled,
  onPick,
}: {
  title: string;
  Icon: typeof PaperclipIcon;
  disabled: boolean;
  onPick: () => void;
}) {
  return (
    <button
      type="button"
      className="grid h-8 w-8 flex-none cursor-pointer place-items-center rounded-[var(--radius)] border-none bg-transparent text-fg-muted hover:bg-hover hover:text-fg disabled:cursor-default disabled:opacity-50"
      // Keeps the focus in the editor.
      onMouseDown={(e) => e.preventDefault()}
      onClick={onPick}
      disabled={disabled}
      title={title}
      aria-label={title}
    >
      <Icon size={16} />
    </button>
  );
}

/** The composer's buttons that attach files or folders picked in the system's dialog. */
export default function AttachButtons({
  disabled,
  onAttachPaths,
}: {
  disabled: boolean;
  onAttachPaths: (paths: string[]) => void;
}) {
  // Several files or several folders at once; the dialog cannot pick both kinds together.
  async function pickAttachments(directory: boolean) {
    const picked = await open({ multiple: true, directory });
    if (picked) onAttachPaths(picked);
  }
  return (
    <div className="flex items-center gap-0.5">
      <AttachButton
        title="Attach files"
        Icon={PaperclipIcon}
        disabled={disabled}
        onPick={() => pickAttachments(false)}
      />
      <AttachButton
        title="Attach folders"
        Icon={FolderIcon}
        disabled={disabled}
        onPick={() => pickAttachments(true)}
      />
    </div>
  );
}
