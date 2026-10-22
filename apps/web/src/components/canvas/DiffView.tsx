import { useMemo } from "react";
import { columns, LineBlock, toBlocks } from "~/components/canvas/LineBlocks";

interface DiffRow {
  kind: "hunk" | "add" | "del" | "ctx";
  text: string;
  oldNo?: number;
  newNo?: number;
}

const HUNK_RE = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

/** Parses a single-file unified diff. Header lines before the first hunk are dropped. */
function parseDiff(diff: string): DiffRow[] {
  const rows: DiffRow[] = [];
  let oldNo = 0;
  let newNo = 0;
  let inHunk = false;
  for (const line of diff.split("\n")) {
    const hunk = HUNK_RE.exec(line);
    if (hunk) {
      oldNo = Number(hunk[1]);
      newNo = Number(hunk[2]);
      inHunk = true;
      rows.push({ kind: "hunk", text: line });
    } else if (!inHunk) {
      continue;
    } else if (line.startsWith("+")) {
      rows.push({ kind: "add", text: line.slice(1), newNo });
      newNo += 1;
    } else if (line.startsWith("-")) {
      rows.push({ kind: "del", text: line.slice(1), oldNo });
      oldNo += 1;
    } else if (line.startsWith(" ")) {
      rows.push({ kind: "ctx", text: line.slice(1), oldNo, newNo });
      oldNo += 1;
      newNo += 1;
    }
    // "\ No newline at end of file" and trailing blank lines fall through.
  }
  return rows;
}

const MARKERS: Record<Exclude<DiffRow["kind"], "hunk">, string> = {
  add: "+",
  del: "−",
  ctx: " ",
};

const ROW_BG: Record<Exclude<DiffRow["kind"], "hunk">, string> = {
  add: "bg-[color:var(--diff-add-bg)]",
  del: "bg-[color:var(--diff-del-bg)]",
  ctx: "",
};

export default function DiffView({ diff }: { diff: string }) {
  const rows = useMemo(() => parseDiff(diff), [diff]);
  const blocks = useMemo(() => toBlocks(rows), [rows]);
  // The widest line, as a floor for the view's width: blocks off screen are not measured.
  const minWidth = useMemo(() => {
    let line = 0;
    let hunk = 0;
    for (const row of rows) {
      if (row.kind === "hunk") hunk = Math.max(hunk, columns(row.text));
      else line = Math.max(line, columns(row.text));
    }
    // Rows have two 48px number columns and a 20px marker; hunk headers have 16px padding each side.
    return `max(100%, calc(${line}ch + 116px), calc(${hunk}ch + 32px))`;
  }, [rows]);

  if (rows.length === 0) {
    return (
      <div className="flex items-center justify-center h-full min-h-[120px] p-6 text-center text-fg-muted">
        No changes in this file.
      </div>
    );
  }

  return (
    <div
      className="w-max min-w-full py-3 font-[family-name:var(--font-mono)] text-[length:var(--fs-md)] leading-[1.65] text-fg tab-2 select-text"
      style={{ minWidth }}
    >
      {blocks.map((block, b) => (
        <LineBlock
          key={b}
          rows={block.length}
          extraPx={block.filter((row) => row.kind === "hunk").length * 8}
        >
          {block.map((row, i) =>
            row.kind === "hunk" ? (
              <div className="py-1 px-4 bg-panel text-fg-subtle whitespace-pre" key={i}>
                {row.text}
              </div>
            ) : (
              <div className={`flex ${ROW_BG[row.kind]}`} key={i}>
                <span className="flex-none min-w-[48px] pr-2 text-right text-fg-subtle select-none tabular-nums">
                  {row.oldNo ?? ""}
                </span>
                <span className="flex-none min-w-[48px] pr-2 text-right text-fg-subtle select-none tabular-nums">
                  {row.newNo ?? ""}
                </span>
                <span className="flex-none w-5 text-center text-fg-subtle select-none">
                  {MARKERS[row.kind]}
                </span>
                <span className="flex-[1_0_auto] whitespace-pre">{row.text}</span>
              </div>
            ),
          )}
        </LineBlock>
      ))}
    </div>
  );
}
