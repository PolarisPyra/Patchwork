import type { ReactNode } from "react";

/** Rows per block. Blocks off screen skip layout and paint, so a file of thousands of lines
 * costs about as much as the part on screen. */
const BLOCK_ROWS = 200;

/** Splits rows into blocks of `BLOCK_ROWS`. */
export function toBlocks<T>(rows: T[]): T[][] {
  const blocks: T[][] = [];
  for (let i = 0; i < rows.length; i += BLOCK_ROWS) blocks.push(rows.slice(i, i + BLOCK_ROWS));
  return blocks;
}

/** Columns a line takes in the monospace font, counting a tab as two (`tab-2`). */
export function columns(text: string): number {
  let tabs = 0;
  for (let i = text.indexOf("\t"); i !== -1; i = text.indexOf("\t", i + 1)) tabs++;
  return text.length + tabs;
}

/** A block of `rows` lines, plus `extraPx` of padding, that the browser renders only near the
 * screen. Its height is estimated until it is first rendered, then remembered. A skipped block
 * has no width, so the view sets its own minimum width from its longest line. */
export function LineBlock({
  rows,
  extraPx = 0,
  children,
}: {
  rows: number;
  extraPx?: number;
  children: ReactNode;
}) {
  return (
    <div
      style={{
        contentVisibility: "auto",
        containIntrinsicHeight: `auto calc(${rows} * 1.65em + ${extraPx}px)`,
      }}
    >
      {children}
    </div>
  );
}
