import { memo, useRef, type KeyboardEvent, type MouseEvent } from "react";
import {
  ArchiveIcon,
  CheckIcon,
  PushPinIcon,
  ShieldWarningIcon,
  StarIcon,
} from "@phosphor-icons/react";
import { AGENT_LABELS, type Thread } from "~/api";
import ProviderLogos from "~/components/ProviderLogos";
import Spinner from "~/components/Spinner";
import { useIsRowSelected } from "~/features/sidebar/lib/selectionState";
import { isPlainPress, selectionFill, type PickModifiers } from "~/features/sidebar/lib/targets";
import type { Target } from "~/features/sidebar/types";
import type { RowDrag } from "~/features/sidebar/useRowReorder";
import { isBusy } from "~/lib/threadEvents";
import { displayTitle, formatRelative } from "~/lib/util";

/**
 * One chat or task in the task list: its agent's icon, its title, and what it is doing. Memoized on
 * what it shows, so a stream in the open chat does not redraw the list around it.
 */

/** Name field shown in place of a title while renaming. Commits once, on Enter or blur. */
export function InlineName({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (name: string) => void;
  onCancel: () => void;
}) {
  // Enter, Escape and the blur that follows them must not each act.
  const done = useRef(false);
  const finish = (action: () => void) => {
    if (done.current) return;
    done.current = true;
    action();
  };
  const keyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === "Enter") finish(() => onCommit(e.currentTarget.value));
    else if (e.key === "Escape") finish(onCancel);
  };
  return (
    <input
      className="h-[28px] min-w-0 flex-1 border border-border-focus rounded-[var(--radius-sm)] bg-input px-1 text-fg outline-none"
      aria-label="Name"
      defaultValue={initial}
      autoFocus
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.stopPropagation()}
      onKeyDown={keyDown}
      onBlur={(e) => finish(() => onCommit(e.currentTarget.value))}
    />
  );
}

/** What a row does when used. These keep one identity across renders, so a row is not re-rendered
 * just because the tree around it was. */
export interface RowActions {
  onSelectThread: (id: string) => void;
  onArchiveThread: (thread: Thread) => void;
  onPick: (e: PickModifiers, target: Target) => boolean;
  onContextMenu: (e: MouseEvent, target: Target) => void;
  onCommitRename: (target: Target, name: string, current: string) => void;
  onCancelRename: () => void;
}

export interface ThreadRowProps {
  thread: Thread;
  /** The open chat. */
  selected: boolean;
  /** Being renamed. */
  editing: boolean;
  /** The current minute, so the row's relative time is redrawn when it changes. */
  minute: number;
  actions: RowActions;
  drag: RowDrag;
  /** 0 for a chat, 1 for a task, 2 for a task in a folder: sets how far the row is indented. */
  depth: 0 | 1 | 2;
}

/** A row redraws only when something it shows changes. `drag.attrs` is left out of the comparison:
 * its handlers are rebuilt every render but act on refs and fixed ids, so an older copy works. */
function sameRow(a: ThreadRowProps, b: ThreadRowProps): boolean {
  return (
    a.thread === b.thread &&
    a.selected === b.selected &&
    a.editing === b.editing &&
    a.minute === b.minute &&
    a.actions === b.actions &&
    a.depth === b.depth &&
    a.drag.edge === b.drag.edge &&
    a.drag.dragging === b.drag.dragging
  );
}

/** Chats whose turn ended while another chat was open. Each shows a check until it is opened. The set
 * sits outside the rows, so a row that scrolls away or redraws keeps its mark. */
const finishedUnseen = new Set<string>();

export const ThreadRow = memo(function ThreadRow({
  thread,
  selected,
  editing,
  actions,
  drag,
  depth,
}: ThreadRowProps) {
  const { onSelectThread, onArchiveThread, onPick, onContextMenu, onCommitRename, onCancelRename } =
    actions;
  const target: Target = { kind: "thread", id: thread.id };
  const picked = useIsRowSelected(target);
  const goalActive = thread.goal?.status === "active";
  const last = thread.messages[thread.messages.length - 1];
  const needsApproval = last?.approvals.some((a) => a.status === "pending") ?? false;
  const busy = isBusy(thread);
  const wasBusy = useRef(busy);
  if (wasBusy.current && !busy && !selected) finishedUnseen.add(thread.id);
  wasBusy.current = busy;
  if (selected) finishedUnseen.delete(thread.id);
  // Tree rows are 28px tall. The indent and the empty chevron column sit in the margin, so the highlight starts just before the title and the titles still line up with project and folder names.
  // Chats sit in the padded list, tasks in their project's unpadded group, so both end at the same edge.
  const margins = ["-ml-[2px] -mr-1", "ml-[30px] mr-1", "ml-[38px] mr-1"][depth];
  const spacing = `h-[28px] rounded-[var(--radius-sm)] pr-2 pl-2 ${margins}`;
  // The picked rows are the selection, and the open chat is always among them.
  const fill = selectionFill(picked);
  return (
    <div
      {...drag.attrs}
      data-thread-id={thread.id}
      role="treeitem"
      aria-level={depth + 1}
      aria-selected={picked}
      aria-current={selected ? "page" : undefined}
      aria-label={editing ? undefined : displayTitle(thread.title)}
      onPointerDown={(e) => {
        drag.attrs.onPointerDown(e);
        if (!editing && isPlainPress(e)) onPick(e, target);
      }}
      className={`group flex cursor-pointer items-center gap-1.5 border-none select-none ${spacing} text-left text-fg ${fill} ${goalActive ? "goal-ring" : ""} ${drag.edge} ${drag.dragging ? "opacity-50" : ""}`}
      onClick={(e) => {
        if (!onPick(e, target)) onSelectThread(thread.id);
      }}
      onContextMenu={(e) => onContextMenu(e, { kind: "thread", id: thread.id })}
    >
      {/* A favorite is listed under Favorites, where its pin does not apply, so it shows the star alone. */}
      {thread.favorite ? (
        <span role="img" aria-label="Favorite" className="inline-flex flex-none text-fg-muted">
          <StarIcon size={13} weight="fill" />
        </span>
      ) : (
        thread.pinned && (
          <span role="img" aria-label="Pinned" className="inline-flex flex-none text-fg-muted">
            <PushPinIcon size={13} weight="fill" />
          </span>
        )
      )}
      {editing ? (
        <InlineName
          initial={thread.title}
          onCommit={(name) => onCommitRename({ kind: "thread", id: thread.id }, name, thread.title)}
          onCancel={onCancelRename}
        />
      ) : (
        <span className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap [mask-image:linear-gradient(to_right,#000_calc(100%-28px),rgb(0_0_0/0.45))]">
          {displayTitle(thread.title)}
        </span>
      )}
      {/* A right-aligned slot as wide as the widest time ("59m", "30d"), so every title ends at the same edge. It holds the time, a spinner or approval shield, and the archive button in their place on hover. */}
      <span className="flex w-8 flex-none items-center justify-end">
        <span className="flex items-center group-hover:hidden">
          {needsApproval ? (
            <span
              title="Waiting for approval"
              role="img"
              aria-label="Waiting for approval"
              className="inline-flex flex-none text-warning"
            >
              <ShieldWarningIcon size={15} />
            </span>
          ) : busy ? (
            <span title="Working" role="img" aria-label="Working" className="inline-flex flex-none">
              <Spinner className="text-accent-text" />
            </span>
          ) : finishedUnseen.has(thread.id) ? (
            <span
              title="Finished"
              role="img"
              aria-label="Finished"
              className="inline-flex flex-none text-success"
            >
              <CheckIcon size={14} weight="bold" />
            </span>
          ) : (
            <span
              className={`text-[length:var(--fs-xs)] ${selected ? "text-fg-muted" : "text-fg-subtle"}`}
            >
              {formatRelative(last?.createdAt ?? thread.createdAt)}
            </span>
          )}
        </span>
        <button
          className="btn-icon hover-reveal h-5 w-5 flex-none hover:bg-border-strong"
          title="Archive"
          aria-label={`Archive ${displayTitle(thread.title)}`}
          onClick={(e) => {
            e.stopPropagation();
            onArchiveThread(thread);
          }}
        >
          <ArchiveIcon size={15} />
        </button>
      </span>
      <span
        className="inline-flex h-4 w-4 flex-none items-center justify-center"
        title={AGENT_LABELS[thread.agent]}
      >
        <ProviderLogos kind={thread.agent} model={thread.model} size={14} />
      </span>
    </div>
  );
}, sameRow);
