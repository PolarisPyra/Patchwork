import type { GitCommit } from "~/api";

/** Lays out the Source Control graph: which lanes run through each commit's row and in what
 * color, read top to bottom from commits in graph order (newest first, children before parents). */

/** A lane waiting for the commit `id` further down, drawn in `color`. It is `pending` while it
 * leads down from a commit that is not pushed yet. */
export interface Lane {
  id: string;
  color: string;
  pending: boolean;
}

export interface GraphRow {
  commit: GitCommit;
  /** Lanes entering the row from above. */
  input: Lane[];
  /** Lanes leaving the row below. */
  output: Lane[];
  /** The lane the commit's dot sits in, and its color. */
  lane: number;
  color: string;
}

/** The checked-out branch's lane is the theme's accent. */
export const HEAD_COLOR = "var(--accent)";
/** Every other lane takes a muted shade of it, in the order lanes open. */
const LANE_COLORS = [
  "color-mix(in srgb, var(--accent) 45%, var(--text-subtle))",
  "var(--text-muted)",
  "color-mix(in srgb, var(--accent) 20%, var(--text-subtle))",
  "var(--text-subtle)",
];

const isHead = (commit: GitCommit) => commit.refs.some((r) => r.kind === "head");

export function layoutGraph(commits: GitCommit[]): GraphRow[] {
  const rows: GraphRow[] = [];
  let colorIndex = -1;
  const nextColor = () => {
    colorIndex = (colorIndex + 1) % LANE_COLORS.length;
    return LANE_COLORS[colorIndex];
  };
  for (const commit of commits) {
    const input = rows.at(-1)?.output ?? [];
    const output: Lane[] = [];
    const pending = !commit.pushed;
    let firstParentAdded = false;
    if (commit.parents.length > 0) {
      for (const lane of input) {
        if (lane.id !== commit.hash) {
          output.push(lane);
        } else if (!firstParentAdded) {
          // The first lane reaching this commit carries on to its first parent; any others end here.
          const color = isHead(commit) ? HEAD_COLOR : lane.color;
          output.push({ id: commit.parents[0], color, pending });
          firstParentAdded = true;
        }
      }
    }
    // A branch tip that no lane reached yet, and each further parent of a merge, opens a lane.
    for (let i = firstParentAdded ? 1 : 0; i < commit.parents.length; i++) {
      const color = i === 0 && isHead(commit) ? HEAD_COLOR : nextColor();
      output.push({ id: commit.parents[i], color, pending });
    }
    let lane = input.findIndex((l) => l.id === commit.hash);
    if (lane === -1) lane = input.length;
    const color =
      output[lane]?.color ?? input[lane]?.color ?? (isHead(commit) ? HEAD_COLOR : LANE_COLORS[0]);
    rows.push({ commit, input, output, lane, color });
  }
  return rows;
}
