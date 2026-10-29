import { grammarRelevance } from "~/lib/highlight";

// The model runs in a worker. One worker serves every request, started on first use.
let worker: Worker | null = null;
let nextId = 1;
const pending = new Map<number, { text: string; resolve: (language: string | null) => void }>();

/**
 * Programming languages the model's best guess is taken for below VS Code's confidence, when the
 * code also matches enough of the language's grammar. A few lines are too little for the model to
 * be sure; text formats that plain text is mistaken for are never taken this way.
 */
const SHORT_CODE_LANGUAGES = new Set([
  "c",
  "cpp",
  "csharp",
  "css",
  "go",
  "java",
  "javascript",
  "kotlin",
  "lua",
  "php",
  "powershell",
  "python",
  "ruby",
  "rust",
  "swift",
  "typescript",
]);
/** Grammar matches a short snippet needs for the model's best guess to be taken. */
const MIN_GRAMMAR_RELEVANCE = 2;

/** The language to label code with, from the model's guess and whether VS Code would take it. */
function chooseLanguage(text: string, guess: string | null, clear: boolean): string | null {
  if (!guess) return null;
  if (clear) return guess;
  const sample = text.slice(0, 10_000);
  const grammarMatches =
    SHORT_CODE_LANGUAGES.has(guess) && grammarRelevance(sample, guess) >= MIN_GRAMMAR_RELEVANCE;
  return grammarMatches ? guess : null;
}

function getWorker(): Worker {
  if (worker) return worker;
  const w = new Worker(new URL("./languageDetection.worker.ts", import.meta.url), {
    type: "module",
  });
  w.onmessage = (e: MessageEvent<{ id: number; guess: string | null; clear: boolean }>) => {
    const request = pending.get(e.data.id);
    pending.delete(e.data.id);
    request?.resolve(chooseLanguage(request.text, e.data.guess, e.data.clear));
  };
  // A failed worker leaves code unlabelled rather than stuck; the next request starts a new one.
  w.onerror = () => {
    for (const { resolve } of pending.values()) resolve(null);
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

/**
 * Lines only terminal output has: a stack frame, a Python traceback, a log level tag, a shell
 * prompt, or a colour escape. The model reads the JSON and paths around them as code.
 */
const TERMINAL_LINE = new RegExp(
  [
    String.raw`^\s+at \S.*:\d+(:\d+)?\)?$`,
    String.raw`^Traceback \(most recent call last\):`,
    String.raw`^\s+File ".+", line \d+`,
    String.raw`^\s*\[(ERROR|WARN|WARNING|INFO|DEBUG|TRACE|FATAL|error|warn|warning|info|debug|trace|fatal)\]`,
    String.raw`^\s*(ERROR|WARN|WARNING|INFO|DEBUG|TRACE|FATAL):\s`,
    String.raw`^(\S+@\S+:)?~?\/\S*.*\s[$#%>❯]\s*$`,
    String.raw`^\s*[$❯] \S`,
    String.raw`\x1b\[`,
  ].join("|"),
  "m",
);

/** Whether the text has a line only terminal output has. */
export function isTerminalOutput(text: string): boolean {
  return TERMINAL_LINE.test(text);
}

/** The language of a piece of code as VS Code would detect it, or null when it is unclear. */
export function detectLanguage(text: string): Promise<string | null> {
  if (isJson(text)) return Promise.resolve("json");
  if (isTerminalOutput(text)) return Promise.resolve(null);
  return new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, { text, resolve });
    getWorker().postMessage({ id, text });
  });
}
