import { useEffect, useRef } from "react";
import { api } from "~/api";

/** How often a shown log is read for new lines, as VS Code's Output view follows a log file. */
const FOLLOW_MS = 500;

/** Lines kept on screen; older ones are dropped as new ones come in. */
const MAX_LINES = 20_000;

const LEVEL_CLASS: Record<string, string> = {
  trace: "text-fg-subtle",
  debug: "text-fg-subtle",
  info: "text-accent-text",
  warning: "text-warning",
  error: "text-danger",
};

/** A log, followed as it grows. Lines are added to the page directly, not through React, since a
 * busy log can add hundreds a second. */
export default function OutputView({ channel }: { channel: string | null }) {
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = box.current;
    if (!el || !channel) return;
    el.replaceChildren();
    let offset = 0;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const follow = async () => {
      try {
        const chunk = await api.readLog(channel, offset);
        if (stopped) return;
        if (chunk.reset) el.replaceChildren();
        if (chunk.text) append(el, chunk.text);
        offset = chunk.offset;
      } catch {
        // The log is not written yet; it is read again shortly.
      }
      if (!stopped) timer = setTimeout(() => void follow(), FOLLOW_MS);
    };
    void follow();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [channel]);

  return (
    <div
      ref={box}
      role="log"
      aria-label="Output"
      aria-live="off"
      className="h-full overflow-auto px-3 py-1 font-mono text-[length:var(--fs-sm)] leading-[1.5] whitespace-pre select-text"
    />
  );
}

/** Adds lines, keeping the view at the bottom if it was there. */
function append(el: HTMLElement, text: string) {
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 4;
  const lines = document.createDocumentFragment();
  for (const line of text.split("\n")) {
    if (line) lines.append(lineNode(line));
  }
  el.append(lines);
  while (el.childElementCount > MAX_LINES) el.firstElementChild?.remove();
  if (atBottom) el.scrollTop = el.scrollHeight;
}

/** `2026-10-26 12:17:29.562 [info] …`, with the time dimmed and the level colored. */
function lineNode(line: string): HTMLElement {
  const row = document.createElement("div");
  const match = /^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d{3}) \[(\w+)\]/.exec(line);
  if (!match) {
    row.textContent = line;
    return row;
  }
  const time = document.createElement("span");
  time.className = "text-fg-subtle";
  time.textContent = match[1];
  const level = document.createElement("span");
  level.className = LEVEL_CLASS[match[2]] ?? "";
  level.textContent = `[${match[2]}]`;
  row.append(time, " ", level, line.slice(match[0].length));
  return row;
}
