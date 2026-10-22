// Runs `highlightLines` in a worker. One worker serves every request, started on first use.
let worker: Worker | null = null;
let nextId = 1;
const pending = new Map<number, (html: string[] | null) => void>();

function getWorker(): Worker {
  if (worker) return worker;
  const w = new Worker(new URL("./highlight.worker.ts", import.meta.url), { type: "module" });
  w.onmessage = (e: MessageEvent<{ id: number; html: string[] }>) => {
    pending.get(e.data.id)?.(e.data.html);
    pending.delete(e.data.id);
  };
  // A failed worker leaves files plain rather than stuck; the next request starts a new one.
  w.onerror = () => {
    for (const resolve of pending.values()) resolve(null);
    pending.clear();
    worker = null;
  };
  worker = w;
  return w;
}

/** One HTML string per line, or null if highlighting failed. */
export function highlightLinesAsync(lines: string[], grammar: string): Promise<string[] | null> {
  return new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, resolve);
    getWorker().postMessage({ id, lines, grammar });
  });
}
