import { useEffect, useMemo, useState } from "react";
import { columns, LineBlock, toBlocks } from "~/components/canvas/LineBlocks";
import { highlightLinesAsync } from "~/components/canvas/highlightAsync";
import { SYNTAX_CLASSES } from "~/components/canvas/syntaxClasses";
import {
  describeFile,
  highlightLines,
  MAX_HIGHLIGHT_LINES,
  splitLines,
} from "~/components/canvas/highlight";

/** Files up to this long are highlighted on the main thread; it takes a few milliseconds. */
const INLINE_HIGHLIGHT_LINES = 1000;

interface CodeViewProps {
  path: string;
  content: string;
}

export default function CodeView({ path, content }: CodeViewProps) {
  const lines = useMemo(() => splitLines(content), [content]);
  const grammar = describeFile(path).grammar;
  const highlightable = !!grammar && lines.length <= MAX_HIGHLIGHT_LINES;
  // Short files are highlighted at once; longer ones show plain until the worker's result arrives,
  // so opening them does not stall the UI.
  const inline = highlightable && lines.length <= INLINE_HIGHLIGHT_LINES;
  const inlineHtml = useMemo(
    () => (inline && grammar ? highlightLines(lines, grammar) : null),
    [inline, grammar, lines],
  );
  const [workerHtml, setWorkerHtml] = useState<{ lines: string[]; html: string[] } | null>(null);
  useEffect(() => {
    if (!highlightable || inline || !grammar) return;
    let current = true;
    void highlightLinesAsync(lines, grammar).then((html) => {
      if (current && html) setWorkerHtml({ lines, html });
    });
    return () => {
      current = false;
    };
  }, [highlightable, inline, grammar, lines]);
  const highlighted = inlineHtml ?? (workerHtml?.lines === lines ? workerHtml.html : null);
  // Line indexes in blocks; blocks off screen are not rendered, so the widest line sets a floor
  // for the view's width.
  const blocks = useMemo(() => toBlocks(lines.map((_, i) => i)), [lines]);
  const minWidth = useMemo(() => {
    const widest = lines.reduce((max, line) => Math.max(max, columns(line)), 0);
    // The line number column is at least 48px wide.
    return `max(100%, calc(${widest}ch + 48px))`;
  }, [lines]);

  if (lines.length === 0) {
    return (
      <div className="flex items-center justify-center h-full min-h-[120px] p-6 text-center text-fg-muted">
        This file is empty.
      </div>
    );
  }

  return (
    <div
      className={`w-max min-w-full py-3 font-[family-name:var(--font-mono)] text-[length:var(--fs-md)] leading-[1.65] text-fg tab-2 select-text ${SYNTAX_CLASSES}`}
      style={{ minWidth }}
    >
      {blocks.map((block, b) => (
        <LineBlock key={b} rows={block.length}>
          {block.map((i) => (
            <div
              className="flex hover:bg-[color:color-mix(in_srgb,var(--bg-hover)_40%,transparent)]"
              key={i}
            >
              <span className="flex-none min-w-[48px] pr-4 text-right text-fg-subtle select-none tabular-nums">
                {i + 1}
              </span>
              {highlighted ? (
                // hljs output escapes the source text, so this HTML is safe.
                <span
                  className="flex-[1_0_auto] whitespace-pre"
                  dangerouslySetInnerHTML={{ __html: highlighted[i] }}
                />
              ) : (
                <span className="flex-[1_0_auto] whitespace-pre">{lines[i]}</span>
              )}
            </div>
          ))}
        </LineBlock>
      ))}
    </div>
  );
}
