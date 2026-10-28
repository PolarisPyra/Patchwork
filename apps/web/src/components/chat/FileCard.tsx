import { useState } from "react";
import { ArrowSquareOutIcon, FileIcon } from "@phosphor-icons/react";
import { api, type FileChange, type FileMedia } from "~/api";
import { useImageSrc } from "~/hooks/useImageSrc";
import type { ReplyFiles } from "~/hooks/useReplyFiles";
import { baseName, formatBytes } from "~/lib/util";

/** Most cards a reply shows, as the summary lists most of its files; the rest are in the Changes view. */
const SHOWN_CARDS = 4;

/**
 * The cards for a reply's produced files: a thumbnail for each image, and a card with its type and
 * size for the rest. Text files get no card; the reply's summary still lists every file.
 */
export function FileCards({
  projectId,
  changes,
  files,
  onOpen,
  onShow,
}: {
  projectId: string | null;
  changes: FileChange[];
  files: ReplyFiles;
  onOpen: (path: string) => void;
  onShow: () => void;
}) {
  const { media, turnFiles, missing } = files;
  const cards = media?.filter((file) => file.kind !== "text") ?? [];
  if (!projectId || cards.length === 0) return null;
  const shown = cards.slice(0, SHOWN_CARDS);
  return (
    <div className="flex flex-wrap items-start gap-2">
      {shown.map((file) => {
        const deleted =
          turnFiles?.some((f) => f.path === file.path && f.state === "deleted") === true ||
          changes.some((c) => c.path === file.path && c.status === "deleted") ||
          missing.has(file.path);
        return (
          <ProducedFile
            key={file.path}
            projectId={projectId}
            file={file}
            deleted={deleted}
            onOpen={() => onOpen(file.path)}
          />
        );
      })}
      {cards.length > shown.length && (
        <button
          className="flex min-h-6 cursor-pointer items-center rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-[length:var(--fs-sm)] text-fg-muted hover:bg-hover hover:text-fg"
          onClick={onShow}
        >
          +{cards.length - shown.length} more
        </button>
      )}
    </div>
  );
}

/**
 * One produced file. An image shows its thumbnail, opened in the canvas on click; a file the turn
 * deleted, or that is gone, shows as a placeholder with `Deleted` and never as a broken image.
 */
function ProducedFile({
  projectId,
  file,
  deleted,
  onOpen,
}: {
  projectId: string;
  file: FileMedia;
  deleted: boolean;
  onOpen: () => void;
}) {
  const [broken, setBroken] = useState(false);
  const src = useImageSrc(file.kind === "image" && !deleted ? file.absolute : null);
  const showImage = src !== null && !broken;
  const name = baseName(file.path);
  const dot = name.lastIndexOf(".");
  const type = dot === -1 ? "" : name.slice(dot + 1).toLowerCase();
  return (
    <div className="flex min-w-0 max-w-[288px] flex-col overflow-hidden rounded-[var(--radius)] border border-border bg-panel text-[length:var(--fs-sm)]">
      {showImage && (
        // A neutral backing keeps a transparent image visible on both themes.
        <button
          className="grid cursor-pointer place-items-center border-none bg-hover p-0"
          title={file.path}
          onClick={onOpen}
        >
          <img
            src={src}
            alt=""
            className="max-h-[320px] max-w-[288px] object-contain"
            onError={() => setBroken(true)}
          />
        </button>
      )}
      <div className="flex min-w-0 items-center gap-2 px-2.5 py-1.5">
        {!showImage && <FileIcon size={15} className="flex-none text-fg-muted" />}
        <button
          className={`min-w-0 flex-1 truncate border-none bg-transparent p-0 text-left ${deleted ? "cursor-default text-fg-muted line-through" : "cursor-pointer text-fg hover:underline"}`}
          title={file.path}
          disabled={deleted}
          onClick={onOpen}
        >
          {name}
        </button>
        {type && (
          <span className="flex-none rounded-[var(--radius-sm)] bg-hover px-1 text-[length:var(--fs-xs)] text-fg-muted">
            {type}
          </span>
        )}
        <span className="flex-none text-[length:var(--fs-xs)] whitespace-nowrap text-fg-subtle tabular-nums">
          {deleted ? "Deleted" : formatBytes(file.size)}
        </span>
        {!deleted && (
          <button
            className="grid h-6 w-6 flex-none cursor-pointer place-items-center rounded-[var(--radius-sm)] border-none bg-transparent p-0 text-fg-muted hover:bg-hover hover:text-fg"
            title="Open in its app"
            aria-label={`Open ${name} in its app`}
            onClick={() => api.openFile(projectId, file.path).catch(() => {})}
          >
            <ArrowSquareOutIcon size={14} />
          </button>
        )}
      </div>
    </div>
  );
}
