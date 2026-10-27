import { useLayoutEffect, useRef, useState } from "react";

/**
 * Stand-ins for `handlers` that keep their identity and call the latest render's versions,
 * so a memoized child can skip renders without holding stale closures. Keys must not change.
 */
export function useStableHandlers<T extends Record<string, (...args: never[]) => unknown>>(
  handlers: T,
): T {
  const latest = useRef(handlers);
  useLayoutEffect(() => {
    latest.current = handlers;
  });
  const [stable] = useState(
    () =>
      Object.fromEntries(
        Object.keys(handlers).map((key) => [
          key,
          (...args: never[]) => latest.current[key](...args),
        ]),
      ) as T,
  );
  return stable;
}
