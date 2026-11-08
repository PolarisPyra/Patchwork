import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import {
  ArrowUpIcon,
  CaretLeftIcon,
  CaretRightIcon,
  DesktopIcon,
  FolderIcon,
  HardDrivesIcon,
} from "@phosphor-icons/react";
import { api, type DirListing, type Machine } from "~/api";
import Spinner from "~/components/Spinner";
import { useDismiss } from "~/hooks/useDismiss";
import { errorText } from "~/lib/util";

interface AddProjectDialogProps {
  /** Registers the browsed folder as a project; resolves once the sidebar has it. */
  onAdd: (path: string) => Promise<void>;
  onClose: () => void;
}

const ROW =
  "flex h-9 w-full items-center gap-3 rounded-[var(--radius)] border-none px-3 text-left text-[length:var(--fs-md)] text-fg";

/** `path` ending in its separator, so typing after it filters the folders inside. */
function withSep(path: string): string {
  const sep = path.includes("\\") && !path.includes("/") ? "\\" : "/";
  return path.endsWith(sep) ? path : path + sep;
}

/**
 * Picks a project folder from inside the app, laid out like the command palette. The field at the
 * top holds the folder being shown: type after it to filter its folders, or type or paste another
 * path and press Enter to go there. When `~/.ssh/config` names hosts, a machine list comes first;
 * only this computer can be browsed today, so the hosts are listed but disabled.
 */
export default function AddProjectDialog({ onAdd, onClose }: AddProjectDialogProps) {
  const [machines, setMachines] = useState<Machine[] | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [listing, setListing] = useState<DirListing | null>(null);
  const [draft, setDraft] = useState("");
  const [index, setIndex] = useState(0);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const activeRow = useRef<HTMLButtonElement>(null);
  const listId = useId();
  useDismiss(panel, true, onClose);

  const browse = (path: string | null) => {
    setLoading(true);
    setError(null);
    api.browseDir(path).then(
      (next) => {
        setListing(next);
        setDraft(withSep(next.path));
        setIndex(0);
        setLoading(false);
      },
      (e) => {
        setError(errorText(e));
        setLoading(false);
      },
    );
  };

  // The home folder loads alongside the machine list, so this computer opens at once either way.
  useEffect(() => {
    browse(null);
    api.listMachines().then(
      (list) => {
        setMachines(list);
        if (!list.some((m) => !m.local)) setBrowsing(true);
      },
      () => setBrowsing(true),
    );
  }, []);

  useEffect(() => {
    if (browsing) input.current?.focus();
    else panel.current?.focus();
  }, [browsing, machines]);

  const base = listing ? withSep(listing.path) : "";
  const typed = listing && draft.startsWith(base) ? draft.slice(base.length) : null;
  // Text after the shown folder, with no separator in it, filters that folder's folders.
  const query = typed !== null && !/[\\/]/.test(typed) ? typed.toLowerCase() : null;
  const visible = (listing?.entries ?? []).filter((e) => {
    const name = e.name.toLowerCase();
    // Hidden folders show only once the filter asks for them.
    if (name.startsWith(".") && !query?.startsWith(".")) return false;
    return query === null || name.includes(query);
  });
  const active = Math.max(0, Math.min(index, visible.length - 1));

  useEffect(() => {
    activeRow.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const add = () => {
    if (!listing) return;
    setAdding(true);
    setError(null);
    onAdd(listing.path).then(onClose, (e) => {
      setError(errorText(e));
      setAdding(false);
    });
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setIndex(Math.min(active + 1, visible.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setIndex(Math.max(active - 1, 0));
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      add();
    } else if (e.key === "Enter") {
      e.preventDefault();
      const entry = visible[active];
      if (query !== null && entry) browse(entry.path);
      else if (query === null) browse(draft);
    } else if (e.key === "Backspace" && draft === base && listing?.parent) {
      e.preventDefault();
      browse(listing.parent);
    }
  };

  const local = machines?.find((m) => m.local);
  const hosts = machines?.filter((m) => !m.local) ?? [];
  // The machine list waits for the hosts, so it never flashes before switching to the folders.
  if (machines === null && !browsing) return null;

  return (
    <div className="fixed inset-0 z-[100] flex items-start justify-center pt-[15vh]">
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label="Add a project"
        tabIndex={-1}
        className="flex max-h-[60vh] w-[560px] max-w-[calc(100vw_-_32px)] flex-col overflow-hidden rounded-[var(--radius-lg)] border border-border-strong bg-elevated shadow-[var(--shadow-popup)] outline-none"
      >
        {!browsing ? (
          <>
            <div className="flex h-14 flex-none items-center border-b border-border px-5 text-[length:var(--fs-lg)] font-semibold">
              Add a project
            </div>
            <div role="group" aria-label="Machines" className="min-h-0 flex-1 overflow-y-auto p-2">
              <button
                className={`${ROW} cursor-pointer bg-transparent hover:bg-hover`}
                onClick={() => setBrowsing(true)}
              >
                <DesktopIcon size={16} className="flex-none text-fg-muted" />
                <span className="min-w-0 flex-1 truncate">{local?.name ?? "This computer"}</span>
                <span className="flex-none text-[length:var(--fs-xs)] text-fg-subtle">
                  This computer
                </span>
                <CaretRightIcon size={12} className="flex-none text-fg-subtle" />
              </button>
              <div className="px-3 pt-3 pb-1.5 text-[length:var(--fs-xs)] font-medium tracking-[0.06em] text-fg-subtle uppercase">
                SSH hosts
              </div>
              {hosts.map((m) => (
                <button
                  key={m.name}
                  aria-disabled="true"
                  title="Connecting to SSH hosts is not supported yet"
                  className={`${ROW} cursor-default bg-transparent opacity-50`}
                >
                  <HardDrivesIcon size={16} className="flex-none text-fg-muted" />
                  <span className="min-w-0 flex-1 truncate">{m.name}</span>
                  {m.hostname && (
                    <span className="max-w-[220px] flex-none truncate font-[family-name:var(--font-mono)] text-[length:var(--fs-xs)] text-fg-subtle">
                      {m.hostname}
                    </span>
                  )}
                </button>
              ))}
            </div>
          </>
        ) : (
          <>
            <div className="flex h-14 flex-none items-center gap-1 border-b border-border pr-5 pl-3">
              {hosts.length > 0 && (
                <button
                  className="btn-icon"
                  aria-label="Back to machines"
                  title="Machines"
                  onClick={() => setBrowsing(false)}
                >
                  <CaretLeftIcon size={16} />
                </button>
              )}
              <button
                className="btn-icon"
                aria-label="Parent folder"
                title="Parent folder"
                disabled={!listing?.parent}
                onClick={() => listing?.parent && browse(listing.parent)}
              >
                <ArrowUpIcon size={16} />
              </button>
              <input
                ref={input}
                className="ml-1 h-full min-w-0 flex-1 border-none bg-transparent font-[family-name:var(--font-mono)] text-[length:var(--fs-md)] text-fg outline-none select-text"
                aria-label="Folder path"
                role="combobox"
                aria-expanded="true"
                aria-controls={listId}
                aria-activedescendant={visible[active] ? `${listId}-${active}` : undefined}
                spellCheck={false}
                value={draft}
                onChange={(e) => {
                  setDraft(e.target.value);
                  setIndex(0);
                }}
                onKeyDown={onKeyDown}
              />
              {loading && <Spinner size={14} className="text-fg-subtle" />}
            </div>
            <div
              id={listId}
              role="listbox"
              aria-label="Folders"
              className="h-[320px] min-h-0 overflow-y-auto p-2"
            >
              {!loading && visible.length === 0 && (
                <div className="px-3 py-6 text-center text-[length:var(--fs-md)] text-fg-subtle">
                  {query ? "No matches" : "No folders"}
                </div>
              )}
              {visible.map((entry, i) => (
                <button
                  key={entry.path}
                  id={`${listId}-${i}`}
                  type="button"
                  role="option"
                  aria-selected={i === active}
                  ref={i === active ? activeRow : undefined}
                  tabIndex={-1}
                  className={`${ROW} cursor-pointer ${i === active ? "bg-selected" : "bg-transparent hover:bg-hover"}`}
                  onClick={() => browse(entry.path)}
                >
                  <FolderIcon size={16} className="flex-none text-fg-muted" />
                  <span className="min-w-0 flex-1 truncate">{entry.name}</span>
                </button>
              ))}
            </div>
            {error && (
              <div
                role="alert"
                className="border-t border-border px-5 py-2 text-[length:var(--fs-sm)] text-danger"
              >
                {error}
              </div>
            )}
            <div className="flex h-12 flex-none items-center gap-4 border-t border-border pr-2.5 pl-5 text-[length:var(--fs-xs)] text-fg-subtle">
              <span>↑↓ navigate</span>
              <span>↵ open</span>
              <span>⌫ up</span>
              <button
                className="btn btn-primary ml-auto"
                disabled={!listing || loading || adding}
                onClick={add}
              >
                {adding && <Spinner />}
                Add project
              </button>
            </div>
          </>
        )}
        {!browsing && error && (
          <div
            role="alert"
            className="border-t border-border px-5 py-2 text-[length:var(--fs-sm)] text-danger"
          >
            {error}
          </div>
        )}
      </div>
    </div>
  );
}
