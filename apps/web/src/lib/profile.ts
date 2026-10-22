// Opt-in timing for backend calls and startup, recorded as Performance entries. Off unless
// `localStorage.patchworkProfile` is "1"; turn it on in the dev session's devtools, reload,
// then run `patchworkReport()` in the console.
const KEY = "patchworkProfile";

export function profiling(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

/** Times `run` as a measure named `name`, with the size of its JSON result when it has one. */
export async function profile<T>(name: string, run: () => Promise<T>): Promise<T> {
  if (!profiling()) return run();
  const start = performance.now();
  const result = await run();
  const bytes = result === undefined ? undefined : JSON.stringify(result).length;
  performance.measure(name, { start, end: performance.now(), detail: { bytes } });
  return result;
}

/** Marks a startup step, such as the threads reaching the UI. */
export function profileMark(name: string): void {
  if (profiling()) performance.mark(name);
}

function report(): void {
  const measures = (performance.getEntriesByType("measure") as PerformanceMeasure[]).map((e) => ({
    name: e.name,
    ms: Math.round(e.duration),
    kilobytes: ((e.detail as { bytes?: number } | null)?.bytes ?? 0) / 1024,
  }));
  const marks = performance.getEntriesByType("mark").map((e) => ({
    name: e.name,
    atMs: Math.round(e.startTime),
  }));
  const [navigation] = performance.getEntriesByType("navigation") as PerformanceNavigationTiming[];
  console.table(measures);
  console.table(marks);
  if (navigation) console.log("DOMContentLoaded at", Math.round(navigation.domContentLoadedEventEnd), "ms");
}

declare global {
  interface Window {
    patchworkReport: () => void;
  }
}

if (profiling()) window.patchworkReport = report;
