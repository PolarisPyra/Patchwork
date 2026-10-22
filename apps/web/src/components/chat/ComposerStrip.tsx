import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

/** Keep the last visible strip mounted until its closing transition has finished. */
export function ComposerStripPresence({
  present,
  children,
}: {
  present: boolean;
  children: ReactNode;
}) {
  const lastContent = useRef(children);
  const [retained, setRetained] = useState(present);

  useLayoutEffect(() => {
    if (present) lastContent.current = children;
  }, [present, children]);

  useLayoutEffect(() => {
    if (present) {
      setRetained(true);
      return;
    }
    // A short buffer after the CSS transition also covers reduced-motion and interrupted exits.
    const timer = window.setTimeout(() => setRetained(false), 220);
    return () => window.clearTimeout(timer);
  }, [present]);

  return (
    <div
      className="composer-strip-presence"
      data-open={present}
      aria-hidden={!present}
      inert={!present}
    >
      <div className="composer-strip-clip">
        {present ? children : retained ? lastContent.current : null}
      </div>
    </div>
  );
}

/** Animate the content's natural height without unmounting it when collapsed. */
export function ComposerStripBody({ open, children }: { open: boolean; children: ReactNode }) {
  return (
    <div className="composer-strip-disclosure" data-open={open} aria-hidden={!open} inert={!open}>
      <div className="composer-strip-clip">{children}</div>
    </div>
  );
}
