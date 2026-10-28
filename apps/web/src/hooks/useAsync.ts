import { useEffect, useState } from "react";

/**
 * The result of `load`, run again whenever `deps` change. Null until a load settles, and whenever
 * `load` is null. The last result stays while the next one loads, so a list does not flicker, and
 * a load that settles after its inputs changed is dropped.
 */
export function useAsync<T>(load: (() => Promise<T>) | null, deps: readonly unknown[]): T | null {
  const [value, setValue] = useState<T | null>(null);
  useEffect(() => {
    if (!load) {
      setValue(null);
      return;
    }
    let live = true;
    load().then(
      (found) => {
        if (live) setValue(found);
      },
      () => {},
    );
    return () => {
      live = false;
    };
    // The caller lists what `load` reads.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return value;
}
