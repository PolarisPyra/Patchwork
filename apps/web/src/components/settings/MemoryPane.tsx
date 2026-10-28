import { useEffect, useState } from "react";
import {
  CaretDownIcon,
  CaretRightIcon,
  FileTextIcon,
  FolderSimpleIcon,
  SparkleIcon,
} from "@phosphor-icons/react";
import { api, type MemoryNode, type Settings } from "~/api";
import type { UpdateSettings } from "~/app/App";
import { home } from "~/components/settings/McpServersPane";
import { TokenRow } from "~/components/settings/SettingsDialog";
import { errorText } from "~/lib/util";

/**
 * Settings → Memory: every place an agent takes standing instructions from, as a tree, with the
 * file under it. This is what is really loaded — a project's `AGENTS.md`, the user's global notes,
 * the skills folder, and each plugin's skills — rather than a second copy of it.
 */
export default function MemoryPane(_props: { settings: Settings; updateSettings: UpdateSettings }) {
  const [tree, setTree] = useState<MemoryNode[] | null>(null);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [picked, setPicked] = useState<MemoryNode | null>(null);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setError(null);
    try {
      const next = await api.memoryTree();
      setTree(next);
      // Every heading and each project start open, so the pane shows what is loaded straight away.
      setOpen((current) => (current.size > 0 ? current : new Set(headings(next))));
    } catch (e) {
      setError(errorText(e));
    }
  }

  useEffect(() => {
    void load();
  }, []);

  async function read(node: MemoryNode) {
    setPicked(node);
    setText(null);
    if (!node.path) return;
    try {
      setText(await api.readMemory(node.path));
    } catch (e) {
      setError(errorText(e));
    }
  }

  const toggle = (id: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const files = tree ? countFiles(tree) : 0;

  return (
    <>
      <div className="flex items-center gap-1.5 pb-3">
        <span className="text-[length:var(--fs-sm)] text-fg-muted">
          {tree === null ? "Reading…" : `${files} instruction files and skills`}
        </span>
        <button className="btn ml-auto flex-none" onClick={() => void load()}>
          Refresh
        </button>
        {picked?.path && (
          <button
            className="btn flex-none"
            onClick={() =>
              api.openMemoryFile(picked.path!).catch((e: unknown) => setError(errorText(e)))
            }
          >
            Open in editor
          </button>
        )}
      </div>

      <div className="flex min-h-[360px] gap-2">
        <div className="w-[46%] min-w-0 overflow-auto rounded-[var(--radius-md)] border border-border p-1">
          {tree?.map((node) => (
            <Branch
              key={key(node)}
              node={node}
              depth={0}
              open={open}
              picked={picked}
              onToggle={toggle}
              onPick={(node) => void read(node)}
            />
          ))}
          {tree && tree.length === 0 && (
            <div className="p-3 text-center text-fg-muted">
              No instruction files yet. An <code>AGENTS.md</code> in a project folder is read by
              every agent.
            </div>
          )}
        </div>
        <div className="min-w-0 flex-1 overflow-auto rounded-[var(--radius-md)] border border-border">
          {picked === null && <div className="p-3 text-fg-muted">Pick a file to read it.</div>}
          {picked && !picked.path && (
            <div className="p-3 text-fg-muted">
              {picked.name} is a heading; pick a file under it.
            </div>
          )}
          {picked?.path && text === null && !error && (
            <div className="p-3 text-fg-subtle">Reading…</div>
          )}
          {picked?.path && text !== null && (
            <>
              <div className="flex items-center gap-1.5 border-b border-border px-3 py-1 text-[length:var(--fs-xs)] text-fg-subtle">
                <FileTextIcon size={12} />
                <span className="truncate" title={picked.path}>
                  {home(picked.path)}
                </span>
              </div>
              <pre className="m-0 px-3 py-2 font-[family-name:var(--font-mono)] text-[12px] leading-[1.5] whitespace-pre-wrap">
                {text}
              </pre>
            </>
          )}
        </div>
      </div>

      {error && <div className="mt-2 text-[length:var(--fs-sm)] text-danger">{error}</div>}

      <div className="mt-3">
        <TokenRow
          title="How memory works"
          desc="Agents read these files themselves: AGENTS.md or CLAUDE.md in the project and its folders, the agents' global notes, and every enabled skill."
        >
          <button className="btn" onClick={() => void load()}>
            Refresh
          </button>
        </TokenRow>
      </div>
    </>
  );
}

/** The ids of the nodes that start open: the headings, and each project under them. */
function headings(nodes: MemoryNode[]): string[] {
  const ids: string[] = [];
  for (const node of nodes) {
    if (node.children.length === 0) continue;
    ids.push(key(node));
    // A project's own children are rarely what someone opens the pane for; a heading's are.
    if (!node.path) ids.push(...headings(node.children));
  }
  return ids;
}

/** A stable id for a node: its path, or its name for a heading. */
function key(node: MemoryNode): string {
  return node.path ?? `folder:${node.name}`;
}

function countFiles(nodes: MemoryNode[]): number {
  return nodes.reduce((sum, node) => sum + (node.path ? 1 : 0) + countFiles(node.children), 0);
}

function Branch({
  node,
  depth,
  open,
  picked,
  onToggle,
  onPick,
}: {
  node: MemoryNode;
  depth: number;
  open: Set<string>;
  picked: MemoryNode | null;
  onToggle: (id: string) => void;
  onPick: (node: MemoryNode) => void;
}) {
  const id = key(node);
  const isOpen = open.has(id);
  const hasChildren = node.children.length > 0;
  const selected = picked !== null && picked.path === node.path;
  const Icon =
    node.kind === "skill"
      ? SparkleIcon
      : node.path && hasChildren
        ? FolderSimpleIcon
        : FileTextIcon;
  return (
    <>
      <button
        className={`flex w-full min-w-0 cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border-0 bg-transparent py-0.5 pr-1.5 text-left text-[length:var(--fs-sm)] ${
          selected ? "bg-selected text-fg" : "text-fg-muted hover:bg-hover hover:text-fg"
        }`}
        style={{ paddingLeft: 4 + depth * 12 }}
        title={node.note ?? node.path ?? node.name}
        onClick={() => {
          if (hasChildren && !node.path) onToggle(id);
          else if (node.path) onPick(node);
          else if (hasChildren) onToggle(id);
        }}
        onDoubleClick={() => hasChildren && onToggle(id)}
      >
        <span className="flex w-3 flex-none justify-center">
          {hasChildren ? (
            isOpen ? (
              <CaretDownIcon size={10} className="text-fg-subtle" />
            ) : (
              <CaretRightIcon size={10} className="text-fg-subtle" />
            )
          ) : null}
        </span>
        <Icon size={12} className="flex-none" />
        <span className="min-w-0 flex-1 truncate">{node.name}</span>
        {node.size !== null && (
          <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle">
            {node.size > 1024 ? `${Math.round(node.size / 1024)}k` : `${node.size}b`}
          </span>
        )}
      </button>
      {hasChildren && (isOpen || !node.path) && (
        <div>
          {node.children.map((child) => (
            <Branch
              key={key(child)}
              node={child}
              depth={node.path ? depth : depth + 1}
              open={open}
              picked={picked}
              onToggle={onToggle}
              onPick={onPick}
            />
          ))}
        </div>
      )}
    </>
  );
}
