import { FileIcon, FolderIcon, ImageIcon, XIcon } from "@phosphor-icons/react";

import { type Attachment, type AttachmentKind } from "~/api";
import { usePreviewSrc } from "~/hooks/useImageSrc";

const ICONS: Record<AttachmentKind, typeof FileIcon> = {
  image: ImageIcon,
  file: FileIcon,
  folder: FolderIcon,
};

interface AttachmentChipProps {
  attachment: Attachment;
  /** Shows a remove button; omitted for attachments already sent. */
  onRemove?: () => void;
}

/** A dropped file, folder, or image, shown as a badge with its full path on hover. */
export default function AttachmentChip({ attachment, onRemove }: AttachmentChipProps) {
  const Icon = ICONS[attachment.kind];
  const src = usePreviewSrc(attachment);
  return (
    <span
      className="inline-flex max-w-[280px] items-center gap-1.5 rounded-[var(--radius)] border border-border-strong bg-panel py-0.5 pr-1 pl-2 text-[length:var(--fs-sm)] text-fg"
      title={attachment.path}
    >
      {src ? (
        <img src={src} alt="" className="h-5 w-5 flex-none rounded-sm object-cover" />
      ) : (
        <Icon size={13} className="flex-none text-fg-muted" />
      )}
      <span className="truncate">{attachment.name}</span>
      <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle uppercase">
        {attachment.kind}
      </span>
      {onRemove && (
        <button
          className="grid h-4 w-4 flex-none cursor-pointer place-items-center rounded-full border-none bg-transparent p-0 text-fg-muted hover:bg-hover hover:text-fg"
          onClick={onRemove}
          title="Remove"
          aria-label={`Remove ${attachment.name}`}
        >
          <XIcon size={10} />
        </button>
      )}
    </span>
  );
}

/** A sent image shown large, as a thumbnail; falls back to a chip until it loads. */
export function AttachmentTile({ attachment }: { attachment: Attachment }) {
  const src = usePreviewSrc(attachment);
  if (!src) return <AttachmentChip attachment={attachment} />;
  return (
    <img
      src={src}
      alt={attachment.name}
      title={attachment.path}
      className="max-h-[180px] max-w-[260px] rounded-[var(--radius)] border border-border object-cover"
    />
  );
}
