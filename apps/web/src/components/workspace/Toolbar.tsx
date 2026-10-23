import { CaretDownIcon, ChatsIcon, FolderPlusIcon } from "@phosphor-icons/react";
import type { Project } from "~/api";
import Dropdown from "~/components/workspace/Dropdown";

const TILE_COLORS = [
  "var(--accent)",
  "var(--syn-string)",
  "var(--syn-property)",
  "var(--syn-keyword)",
  "var(--syn-type)",
];

/** Stable tile color per project name, drawn from the theme palette. */
function tileColor(name: string): string {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  return TILE_COLORS[hash % TILE_COLORS.length];
}

function initials(name: string): string {
  return Array.from(name.trim()).slice(0, 2).join("").toUpperCase();
}

interface ToolbarProps {
  project: Project | null;
  projects: Project[];
  onSelectProject: (id: string) => void;
  onOpenFolder: () => void;
}

export default function Toolbar({
  project,
  projects,
  onSelectProject,
  onOpenFolder,
}: ToolbarProps) {
  const name = project?.name ?? "";

  return (
    <header className="flex min-w-0 items-center gap-1.5 border-b border-border bg-app pr-2 pl-[calc((var(--activity-bar-w)_+_4px_-_20px)/2_-_6px)]">
      <div className="flex min-w-0 items-center gap-1.5">
        <Dropdown
          trigger={({ toggle }) => (
            <button
              className="inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent py-0 pr-1.5 pl-1 hover:bg-hover"
              onClick={toggle}
            >
              {project ? (
                <span
                  className="grid h-5 w-5 flex-none place-items-center rounded-[var(--radius-sm)] text-[10px] font-bold text-on-accent"
                  style={{ background: tileColor(name) }}
                >
                  {initials(name)}
                </span>
              ) : (
                <span className="grid h-5 w-5 flex-none place-items-center rounded-[var(--radius-sm)] bg-accent-soft text-[10px] font-bold text-accent-text">
                  <ChatsIcon size={12} />
                </span>
              )}
              <span className="max-w-[180px] overflow-hidden text-ellipsis whitespace-nowrap font-semibold">
                {project?.name ?? "No project"}
              </span>
              <CaretDownIcon size={14} />
            </button>
          )}
        >
          {(close) => (
            <>
              {projects.map((p) => (
                <button
                  key={p.id}
                  className="flex h-[26px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 text-left text-fg whitespace-nowrap hover:bg-hover"
                  onClick={() => {
                    close();
                    onSelectProject(p.id);
                  }}
                >
                  <span
                    className={`h-1.5 w-1.5 flex-none rounded-[50%] ${p.id === project?.id ? "bg-accent" : "bg-border-strong"}`}
                  />
                  {p.name}
                </button>
              ))}
              {projects.length > 0 && <div className="my-1 h-px bg-border" />}
              <button
                className="flex h-[26px] cursor-pointer items-center gap-1.5 rounded-[var(--radius-sm)] border-none bg-transparent px-2 text-left text-fg whitespace-nowrap hover:bg-hover"
                onClick={() => {
                  close();
                  onOpenFolder();
                }}
              >
                <FolderPlusIcon size={14} />
                Open folder…
              </button>
            </>
          )}
        </Dropdown>
      </div>
    </header>
  );
}
