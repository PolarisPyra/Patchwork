import { type ReactNode } from "react";

/**
 * A part of a row that opens: it grows its row height from 0fr to 1fr, so the rows below slide
 * down with it instead of jumping. `overflow-hidden` lets the height be the only thing animating.
 */
export default function Expander({ children }: { children: ReactNode }) {
  return (
    <div className="timeline-expand timeline-expand-enter overflow-hidden">
      <div className="flex min-w-0 flex-col">{children}</div>
    </div>
  );
}
