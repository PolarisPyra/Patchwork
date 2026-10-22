import { useState } from "react";
import { DotsThreeIcon } from "@phosphor-icons/react";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "~/api";

// Markdown loads with the chat, not on first use: its plain-text placeholder had another height,
// so swapping it in moved the messages.
import Markdown from "~/components/chat/Markdown";

/** The plan's first heading, which names it. */
export function planTitle(markdown: string): string {
  return /^\s{0,3}#{1,6}\s+(.+)$/m.exec(markdown)?.[1].trim() || "Proposed plan";
}

/** The plan without its title heading, and without a "Summary" heading right after it. */
function planBody(markdown: string): string {
  const lines = markdown.trim().split("\n");
  if (/^\s{0,3}#{1,6}\s/.test(lines[0] ?? "")) lines.shift();
  const next = lines.findIndex((l) => l.trim() !== "");
  if (next !== -1 && /^\s{0,3}#{1,6}\s+summary\s*$/i.test(lines[next])) lines.splice(next, 1);
  return lines.join("\n").trim();
}

/** A file name from the title: lowercase words joined by dashes. */
function planFileName(title: string): string {
  const slug = title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${slug || "plan"}.md`;
}

/**
 * A plan the agent proposed in plan mode. A long plan shows its start until expanded; the menu
 * copies or saves it as Markdown.
 */
export default function ProposedPlanCard({ markdown }: { markdown: string }) {
  const [expanded, setExpanded] = useState(false);
  const [menu, setMenu] = useState(false);
  const [copied, setCopied] = useState(false);
  const title = planTitle(markdown);
  const body = planBody(markdown);
  const canCollapse = body.length > 900 || body.split("\n").length > 20;
  const collapsed = canCollapse && !expanded;
  const preview = collapsed
    ? [
        ...body
          .split("\n")
          .filter((l) => l.trim() !== "")
          .slice(0, 10),
        "",
        "...",
      ].join("\n")
    : body;

  async function copy() {
    await navigator.clipboard.writeText(`${markdown.trimEnd()}\n`);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  async function download() {
    setMenu(false);
    const path = await save({
      defaultPath: planFileName(title),
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (path) await api.saveTextFile(path, `${markdown.trimEnd()}\n`);
  }

  const item =
    "flex w-full cursor-pointer items-center rounded-[var(--radius-sm)] border-none bg-transparent px-2 py-[5px] text-left whitespace-nowrap text-fg hover:bg-hover";
  return (
    <div className="rounded-[16px] border border-border bg-panel p-4">
      <div className="flex items-center gap-3">
        <span className="badge flex-none">Plan</span>
        <h3 className="m-0 min-w-0 flex-1 truncate text-[length:var(--fs-md)] font-medium text-fg">
          {title}
        </h3>
        <div className="relative flex-none">
          <button
            type="button"
            aria-label="Plan actions"
            aria-expanded={menu}
            onClick={() => setMenu((m) => !m)}
            className="grid h-6 w-6 cursor-pointer place-items-center rounded-[var(--radius-sm)] border border-border bg-transparent text-fg-muted hover:bg-hover hover:text-fg"
          >
            <DotsThreeIcon size={14} />
          </button>
          {menu && (
            <div className="absolute top-7 right-0 z-10 flex min-w-[180px] flex-col rounded-[var(--radius)] border border-border bg-panel p-1 text-[length:var(--fs-sm)] shadow-[var(--shadow-popup)]">
              <button type="button" className={item} onClick={() => void copy()}>
                {copied ? "Copied!" : "Copy to clipboard"}
              </button>
              <button type="button" className={item} onClick={() => void download()}>
                Download as markdown
              </button>
            </div>
          )}
        </div>
      </div>
      <div className={`relative mt-4 ${collapsed ? "max-h-104 overflow-hidden" : ""}`}>
        <Markdown text={preview} />
        {collapsed && (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 h-24 bg-gradient-to-t from-[var(--bg-panel)] to-transparent" />
        )}
      </div>
      {canCollapse && (
        <div className="mt-3 flex justify-center">
          <button type="button" className="btn" onClick={() => setExpanded((e) => !e)}>
            {expanded ? "Collapse plan" : "Expand plan"}
          </button>
        </div>
      )}
    </div>
  );
}
