import { type HTMLAttributes, type ReactNode, type Ref } from "react";
import { useIsCollapsed } from "~/lib/collapsedState";
import { useIsRowSelected } from "~/features/sidebar/lib/selectionState";
import { selectionFill } from "~/features/sidebar/lib/targets";
import type { Target } from "~/features/sidebar/types";
import type { Project } from "~/api";

/**
 * The parts a tree is made of: a twistie that points at the open state, a region that animates its
 * height, a row whose fill follows the selection, and a section's label. Each follows the shared
 * state it draws, so opening a folder re-renders its twistie and not the whole tree.
 */

/** Animates its children open and closed. Stays mounted so the height can transition. */
export function Collapse({
  open,
  children,
  overflowWhenOpen = false,
}: {
  open: boolean;
  children: ReactNode;
  /** Lets an open row's drop line reach past the row's edge. Closing still clips. */
  overflowWhenOpen?: boolean;
}) {
  const clip = open && overflowWhenOpen ? "overflow-visible" : "overflow-hidden";
  return (
    <div
      className={`grid grid-cols-[minmax(0,1fr)] transition-[grid-template-rows] duration-150 ease-[cubic-bezier(0.2,0,0,1)] ${open ? "grid-rows-[1fr]" : "grid-rows-[0fr]"}`}
      inert={!open}
    >
      <div className={`min-h-0 ${clip}`}>{children}</div>
    </div>
  );
}

/** VS Code's tree twistie: the chevron-down codicon, turned to point right while a row is collapsed. */
export function Twistie({ open }: { open: boolean }) {
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 16 16"
      aria-hidden
      className={`transition-transform duration-150 ease-[cubic-bezier(0.2,0,0,1)] will-change-transform ${open ? "" : "-rotate-90"}`}
    >
      <path
        fill="currentColor"
        transform="translate(3 5.25) scale(0.0533333)"
        d="M3.0 16.0 87.0 100.0Q90.0 103.0 94.0 103.0Q98.0 103.0 101.0 100.0L185.0 16.0Q188.0 13.0 188.0 9.0Q188.0 5.0 185.0 2.5Q182.0 0.0 178.0 0.0Q174.0 0.0 172.0 3.0L94.0 80.0L16.0 3.0Q14.0 0.0 10.0 0.0Q6.0 0.0 3.0 2.5Q0.0 5.0 0.0 9.0Q0.0 13.0 3.0 16.0Z"
      />
    </svg>
  );
}

/** The twistie and the collapsible region of a project or folder follow the collapsed set themselves,
 * so opening or closing one re-renders these two and not every row in the tree. */
export function TreeTwistie({ id }: { id: string }) {
  const collapsed = useIsCollapsed(id);
  return <Twistie open={!collapsed} />;
}

export function TreeCollapse({ id, children }: { id: string; children: ReactNode }) {
  const collapsed = useIsCollapsed(id);
  return <Collapse open={!collapsed}>{children}</Collapse>;
}

/** A tree row's box, filled by whether it is picked. It follows the picked rows itself, so a pick
 * re-renders the rows it changes and not the whole tree. */
export function PickRow({
  target,
  expandId,
  className,
  ...rest
}: HTMLAttributes<HTMLDivElement> & {
  target: Target;
  /** The collapsed-set id of the rows it opens and closes, which its expanded state reads. */
  expandId: string;
  className: string;
  ref?: Ref<HTMLDivElement>;
}) {
  const picked = useIsRowSelected(target);
  const collapsed = useIsCollapsed(expandId);
  return (
    <div
      role="treeitem"
      aria-selected={picked}
      aria-expanded={!collapsed}
      {...rest}
      className={`${className} ${selectionFill(picked)}`}
    />
  );
}

/** For projects that share a name, the end of each one's path that tells them apart, by project id.
 * A folder named like its project is left out, since the name already shows it. */
export function telltalePaths(projects: Project[]): Map<string, string> {
  const labels = new Map<string, string>();
  const byName = new Map<string, Project[]>();
  for (const p of projects) byName.set(p.name, [...(byName.get(p.name) ?? []), p]);
  for (const group of byName.values()) {
    if (group.length < 2) continue;
    const parts = group.map((p) => {
      const segs = p.path.split(/[\\/]/).filter(Boolean);
      return segs.at(-1) === p.name ? segs.slice(0, -1) : segs;
    });
    const longest = Math.max(...parts.map((s) => s.length));
    for (let depth = 1; depth <= longest; depth++) {
      const tails = parts.map((s) => s.slice(-depth).join("/"));
      if (new Set(tails).size === tails.length || depth === longest) {
        group.forEach((p, i) => labels.set(p.id, tails[i]));
        break;
      }
    }
  }
  return labels;
}

export function SectionLabel({
  children,
  action,
  ref,
}: {
  children: ReactNode;
  action?: ReactNode;
  ref?: Ref<HTMLDivElement>;
}) {
  return (
    // The negative right margin centers the action's icon on the rows' provider icons.
    <div
      ref={ref}
      className="-mr-0.5 flex items-center justify-between pt-2.5 pl-1.5 first:pt-1 text-[length:var(--fs-xs)] tracking-[0.06em] text-fg-muted uppercase"
    >
      <span>{children}</span>
      {action}
    </div>
  );
}
