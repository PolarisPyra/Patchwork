// Runs language detection in a worker. One worker serves every request, started on first use.
let worker: Worker | null = null;
let nextId = 1;
const pending = new Map<number, (language: string | null) => void>();

function getWorker(): Worker {
  if (worker) return worker;
  const w = new Worker(new URL("./languageDetection.worker.ts", import.meta.url), {
    type: "module",
  });
  w.onmessage = (e: MessageEvent<{ id: number; language: string | null }>) => {
    pending.get(e.data.id)?.(e.data.language);
    pending.delete(e.data.id);
  };
  // A failed worker leaves code unlabelled rather than stuck; the next request starts a new one.
  w.onerror = () => {
    for (const resolve of pending.values()) resolve(null);
    pending.clear();
    worker = null;
  };
  worker = w;
  return w;
}

/** Whether the text is a JSON object or array, which the model misses when it is short. */
function isJson(text: string): boolean {
  try {
    const value: unknown = JSON.parse(text);
    return typeof value === "object" && value !== null;
  } catch {
    return false;
  }
}

/** The language of a piece of code as VS Code would detect it, or null when it is unclear. */
export function detectLanguage(text: string): Promise<string | null> {
  if (isJson(text)) return Promise.resolve("json");
  return new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, resolve);
    getWorker().postMessage({ id, text });
  });
}
