import { useEffect, useMemo, useState } from "react";
import { ArrowUUpLeftIcon } from "@phosphor-icons/react";
import { confirm } from "@tauri-apps/plugin-dialog";
import { api, type CommitReply, type FileChange, type GitCommit, type Thread } from "~/api";
import ContextMenu, { type MenuItem } from "~/components/ContextMenu";
import ProviderLogos from "~/components/ProviderLogos";
import { errorText } from "~/lib/util";
import { HEAD_COLOR, layoutGraph, type GraphRow } from "~/features/sidebar/lib/graph";

/** The commit history of the checked-out branch and its upstream, drawn as lanes. A commit not
 * pushed yet is a hollow dot, and the lines down from it are dashed. */

/** Commits read per page. */
const PAGE = 50;
/** Row height, lane width, and the radius of the bends where a lane shifts over. */
const ROW = 22;
const LANE = 11;
const BEND = 5;

const x = (lane: number) => LANE * (lane + 1);

/** A reply's request, cut to fit a menu item. */
const shorten = (text: string) => (text.length > 48 ? `${text.slice(0, 47)}…` : text);

/** One row's lanes and its commit's dot. */
function RowGraph({ row }: { row: GraphRow }) {
  const { commit, input, output, lane: circle, color } = row;
  const mid = ROW / 2;
  const paths: { d: string; color: string; dashed: boolean }[] = [];

  let out = 0;
  input.forEach((lane, i) => {
    if (lane.id === commit.hash) {
      // Another lane waiting for this commit joins its dot here.
      if (i !== circle) {
        paths.push({
          d: `M ${x(i)} 0 A ${LANE} ${LANE} 0 0 1 ${x(i) - LANE} ${mid} H ${x(circle)}`,
          color: lane.color,
          dashed: lane.pending,
        });
      } else out++;
    } else if (out < output.length && lane.id === output[out].id) {
      paths.push({
        d:
          i === out
            ? `M ${x(i)} 0 V ${ROW}`
            : `M ${x(i)} 0 V 6 A ${BEND} ${BEND} 0 0 1 ${x(i) - BEND} ${mid} H ${x(out) + BEND} A ${BEND} ${BEND} 0 0 0 ${x(out)} ${mid + BEND} V ${ROW}`,
        color: lane.color,
        dashed: lane.pending,
      });
      out++;
    }
  });
  // A merge's further parents branch off to their own lanes.
  for (const parent of commit.parents.slice(1)) {
    const p = output.map((l) => l.id).lastIndexOf(parent);
    if (p === -1) continue;
    paths.push({
      d: `M ${x(p) - LANE} ${mid} A ${LANE} ${LANE} 0 0 1 ${x(p)} ${ROW} M ${x(p) - LANE} ${mid} H ${x(circle)}`,
      color: output[p].color,
      dashed: output[p].pending,
    });
  }
  if (circle < input.length) {
    paths.push({
      d: `M ${x(circle)} 0 V ${mid}`,
      color: input[circle].color,
      dashed: input[circle].pending,
    });
  }
  if (commit.parents.length > 0) {
    paths.push({ d: `M ${x(circle)} ${mid} V ${ROW}`, color, dashed: !commit.pushed });
  }

  const width = LANE * (Math.max(input.length, output.length, 1) + 1);
  const r = commit.refs.some((ref) => ref.kind === "head") ? 4.5 : 3.5;
  return (
    <svg width={width} height={ROW} className="flex-none" aria-hidden="true">
      {paths.map((p, i) => (
        <path
          key={i}
          d={p.d}
          style={{ stroke: p.color }}
          strokeWidth={1.5}
          strokeDasharray={p.dashed ? "2 2.5" : undefined}
          fill="none"
          strokeLinecap="round"
        />
      ))}
      {commit.pushed ? (
        <circle cx={x(circle)} cy={mid} r={r} style={{ fill: color }} />
      ) : (
        <circle
          cx={x(circle)}
          cy={mid}
          r={r}
          style={{ fill: "var(--bg-panel)", stroke: color }}
          strokeWidth={1.5}
        />
      )}
    </svg>
  );
}

/** The chat or task a commit was made from, as its agent's logo; a live one opens on click. */
function ThreadMark({
  thread,
  archived,
  onSelect,
}: {
  thread: Thread;
  archived: boolean;
  onSelect: (id: string) => void;
}) {
  const logo = <ProviderLogos kind={thread.agent} model={thread.model} size={12} />;
  if (archived) {
    const label = `Committed from ${thread.title} (archived)`;
    return (
      <span role="img" aria-label={label} title={label} className="flex flex-none">
        {logo}
      </span>
    );
  }
  const label = `Committed from ${thread.title}`;
  return (
    <button
      className="btn-icon !h-4 !w-4"
      aria-label={label}
      title={label}
      onClick={(e) => {
        e.stopPropagation();
        onSelect(thread.id);
      }}
    >
      {logo}
    </button>
  );
}

export default function GraphSection({
  projectId,
  changes,
  threads,
  archivedThreads,
  onSelectThread,
}: {
  projectId: string;
  /** Read again with each change to the project, which includes commits and checkouts. */
  changes: FileChange[];
  /** Chats and tasks, live and archived, for the ones commits were made from. */
  threads: Thread[];
  archivedThreads: Thread[];
  onSelectThread: (id: string) => void;
}) {
  const [commits, setCommits] = useState<GitCommit[]>([]);
  const [more, setMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pages, setPages] = useState(1);
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    hash: string;
    items: MenuItem[];
  } | null>(null);
  const [undoError, setUndoError] = useState<string | null>(null);

  useEffect(() => setPages(1), [projectId]);

  useEffect(() => {
    let live = true;
    api.gitLog(projectId, 0, PAGE * pages).then(
      (list) => {
        if (!live) return;
        setCommits(list);
        setMore(list.length === PAGE * pages);
        setError(null);
      },
      (e: unknown) => live && setError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [projectId, pages, changes]);

  const rows = useMemo(() => layoutGraph(commits), [commits]);
  const dates = useMemo(
    () => new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }),
    [],
  );
  const live = useMemo(() => new Map(threads.map((t) => [t.id, t])), [threads]);
  const archived = useMemo(() => new Map(archivedThreads.map((t) => [t.id, t])), [archivedThreads]);

  /** Takes a reply's changes back out of the files after asking; the commit stays. */
  async function undoReply(threadId: string, reply: CommitReply) {
    setUndoError(null);
    const ok = await confirm(
      `Take the changes of “${reply.prompt || "this reply"}” back out of your files? The commit stays, and the undo shows as uncommitted changes.`,
      { title: "Undo reply", kind: "warning" },
    );
    if (!ok) return;
    try {
      await api.undoReply(threadId, reply.messageId);
    } catch (e) {
      setUndoError(errorText(e));
    }
  }

  /** Lists the replies a commit took in, each to undo, below the point given. */
  function openUndoMenu(x: number, y: number, commit: GitCommit, threadId: string) {
    setMenu({
      x,
      y,
      hash: commit.hash,
      items: commit.replies.map((reply) => ({
        label: reply.prompt ? `Undo “${shorten(reply.prompt)}”` : "Undo reply",
        Icon: ArrowUUpLeftIcon,
        danger: true,
        onSelect: () => void undoReply(threadId, reply),
      })),
    });
  }

  if (error) {
    return (
      <div role="alert" className="px-3 text-[length:var(--fs-sm)] text-danger">
        {error}
      </div>
    );
  }
  if (rows.length === 0) {
    return <div className="px-3 text-[length:var(--fs-sm)] text-fg-muted">No commits</div>;
  }
  return (
    <>
      {undoError && (
        <div role="alert" className="px-3 pb-1 text-[length:var(--fs-xs)] break-words text-danger">
          {undoError}
        </div>
      )}
      <ul aria-label="Commit history" className="m-0 flex list-none flex-col p-0">
        {rows.map((row) => {
          const { commit } = row;
          const head = commit.refs.find((r) => r.kind === "head");
          // The branch is named once, at its commit; a remote only where it is somewhere else.
          const remotes = head
            ? []
            : commit.refs.filter((r) => r.kind === "remote" && !r.name.endsWith("/HEAD"));
          const thread = commit.threadId
            ? (live.get(commit.threadId) ?? archived.get(commit.threadId))
            : undefined;
          // Only a live thread's replies can be undone; an archived one is restored first.
          const undoable = thread && live.has(thread.id) && commit.replies.length > 0;
          const menuOpen = menu?.hash === commit.hash;
          return (
            <li
              key={commit.hash}
              className="group flex h-[22px] items-center gap-1.5 pr-2 pl-1 hover:bg-hover"
              onContextMenu={(e) => {
                if (!undoable) return;
                e.preventDefault();
                openUndoMenu(e.clientX, e.clientY, commit, thread.id);
              }}
              title={`${commit.subject}\n${commit.author}, ${dates.format(commit.time * 1000)}\n${commit.hash.slice(0, 8)}${commit.pushed ? "" : "\nNot pushed"}`}
            >
              <RowGraph row={row} />
              {thread && (
                <ThreadMark
                  thread={thread}
                  archived={!live.has(thread.id)}
                  onSelect={onSelectThread}
                />
              )}
              <span
                className={`min-w-0 truncate text-[length:var(--fs-sm)] ${head ? "font-semibold" : ""}`}
              >
                {commit.subject}
              </span>
              <span className="min-w-0 flex-1 truncate text-[length:var(--fs-xs)] text-fg-subtle">
                {commit.author}
              </span>
              {undoable && (
                <button
                  className={`btn-icon !h-4 !w-4 flex-none ${menuOpen ? "" : "hover-reveal"}`}
                  aria-label="Undo a reply"
                  title="Undo a reply"
                  aria-haspopup="menu"
                  aria-expanded={menuOpen}
                  onClick={(e) => {
                    e.stopPropagation();
                    const r = e.currentTarget.getBoundingClientRect();
                    openUndoMenu(r.left, r.bottom + 4, commit, thread.id);
                  }}
                >
                  <ArrowUUpLeftIcon size={12} />
                </button>
              )}
              {(head || remotes.length > 0) && (
                <span
                  className="max-w-[120px] flex-none truncate text-[length:var(--fs-xs)] font-medium"
                  style={{ color: head ? HEAD_COLOR : row.color }}
                >
                  {head ? head.name : remotes.map((r) => r.name).join(", ")}
                </span>
              )}
            </li>
          );
        })}
        {more && (
          <li className="flex h-[22px] items-center px-3">
            <button
              className="cursor-pointer border-none bg-transparent p-0 text-[length:var(--fs-sm)] text-accent-text hover:underline"
              onClick={() => setPages((p) => p + 1)}
            >
              Load more
            </button>
          </li>
        )}
      </ul>
      {menu && (
        <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />
      )}
    </>
  );
}
