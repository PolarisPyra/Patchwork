import { useState } from "react";
import { ArrowSquareOutIcon, FileIcon } from "@phosphor-icons/react";
import { api, type FileChange, type FileMedia } from "~/api";
import { useImageSrc } from "~/hooks/useImageSrc";
import type { ReplyFiles } from "~/features/chat/hooks/useReplyFiles";
import { baseName, formatBytes } from "~/lib/util";

/** Most files the list shows, as the summary lists most of its files; the rest are in the Changes view. */
const SHOWN_CARDS = 3;

/**
 * The list of a reply's produced files, one row each with its type and size, and a small thumbnail
 * for an image. Text files get no row; the reply's summary still lists every file.
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
  // One file over the limit is shown rather than hidden behind "1 more".
  const shown = cards.length > SHOWN_CARDS + 1 ? cards.slice(0, SHOWN_CARDS) : cards;
  return (
    // One grid for all rows, as in the summary below, so every type, size and button sits in the
    // same column; the last column keeps its width on a deleted file, which has no button.
    <ul
      className="m-0 grid list-none grid-cols-[auto_minmax(0,1fr)_auto_auto_1.5rem] gap-x-2 rounded-[var(--radius)] border border-border bg-panel px-3 py-2 text-[length:var(--fs-sm)]"
      aria-label="Files from this reply"
    >
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
        <li className="col-span-full">
          <button
            className="flex min-h-6 w-full cursor-pointer items-center rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-left text-fg-muted hover:bg-hover hover:text-fg"
            onClick={onShow}
          >
            {cards.length - shown.length} more
          </button>
        </li>
      )}
    </ul>
  );
}

/**
 * One produced file, opened in the canvas on click. An image shows a small thumbnail; a file the
 * turn deleted, or that is gone, shows `Deleted` and never a broken image.
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
    <li className="col-span-full grid min-w-0 grid-cols-subgrid items-center">
      <button
        className={`col-span-4 grid min-h-7 min-w-0 grid-cols-subgrid items-center rounded-[var(--radius-sm)] border-none bg-transparent px-1 text-left ${deleted ? "cursor-default" : "cursor-pointer hover:bg-hover"}`}
        title={file.path}
        disabled={deleted}
        onClick={onOpen}
      >
        {showImage ? (
          // A neutral backing keeps a transparent image visible on both themes.
          <img
            src={src}
            alt=""
            className="h-6 w-6 rounded-[var(--radius-sm)] bg-hover object-cover"
            onError={() => setBroken(true)}
          />
        ) : (
          <span className="grid h-6 w-6 place-items-center">
            <FileIcon size={15} className="text-fg-muted" />
          </span>
        )}
        <span className={`min-w-0 truncate ${deleted ? "text-fg-muted line-through" : "text-fg"}`}>
          {name}
        </span>
        {type && (
          <span className="col-start-3 max-w-[12ch] justify-self-end truncate rounded-[var(--radius-sm)] bg-hover px-1 text-[length:var(--fs-xs)] text-fg-muted">
            {type}
          </span>
        )}
        <span className="col-start-4 justify-self-end text-[length:var(--fs-xs)] whitespace-nowrap text-fg-subtle tabular-nums">
          {deleted ? "Deleted" : formatBytes(file.size)}
        </span>
      </button>
      {!deleted && (
        <button
          className="grid h-6 w-6 cursor-pointer place-items-center rounded-[var(--radius-sm)] border-none bg-transparent p-0 text-fg-muted hover:bg-hover hover:text-fg"
          title="Open in its app"
          aria-label={`Open ${name} in its app`}
          onClick={() => api.openFile(projectId, file.path).catch(() => {})}
        >
          <ArrowSquareOutIcon size={14} />
        </button>
      )}
    </li>
  );
}
