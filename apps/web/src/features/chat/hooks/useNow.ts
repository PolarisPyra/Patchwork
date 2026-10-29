import { useEffect, useState } from "react";

/**
 * The current time, ticking each second while `live` and standing still once it is not, so a
 * finished duration is read once instead of counting on after the work has stopped.
 */
export function useNow(live: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [live]);
  return live ? now : Date.now();
}
