import { useEffect, useState } from "react";

/** Where `useAsync` keeps a result, so a component that mounts again shows it at once. */
export interface AsyncCache<T> {
  store: Map<string, T>;
  /** Names everything `load` reads, as the deps do. */
  key: string;
}

// How many versions of the changes a hook keeps results for: enough to go back and forth between
// a few projects without asking again.
const KEPT_VERSIONS = 8;

// Each `changes` list's contents as text, worked out once per list.
const versions = new WeakMap<object, string>();

/**
 * A store for each version of the `changes` a hook is given: results are kept while the files
 * stand as they did, and read again once they change. Opening a project reads its changes again
 * into a new list, so the store follows what the list says rather than which list it is.
 */
export function cachePerChanges<T>(): (changes: object) => Map<string, T> {
  const stores = new Map<string, Map<string, T>>();
  return (changes) => {
    let version = versions.get(changes);
    if (version === undefined) versions.set(changes, (version = JSON.stringify(changes)));
    let store = stores.get(version);
    if (store) return store;
    stores.set(version, (store = new Map()));
    if (stores.size > KEPT_VERSIONS) stores.delete(stores.keys().next().value!);
    return store;
  };
}

/**
 * The result of `load`, run again whenever `deps` change. Null until a load settles, and whenever
 * `load` is null. The last result stays while the next one loads, so a list does not flicker, and
 * a load that settles after its inputs changed is dropped. With a `cache`, a result already kept
 * there is used instead of loading.
 */
export function useAsync<T>(
  load: (() => Promise<T>) | null,
  deps: readonly unknown[],
  cache?: AsyncCache<T>,
): T | null {
  const [value, setValue] = useState<T | null>(() =>
    load && cache ? (cache.store.get(cache.key) ?? null) : null,
  );
  useEffect(() => {
    if (!load) {
      setValue(null);
      return;
    }
    const kept = cache?.store.get(cache.key);
    if (kept !== undefined) {
      setValue(kept);
      return;
    }
    let live = true;
    load().then(
      (found) => {
        cache?.store.set(cache.key, found);
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
