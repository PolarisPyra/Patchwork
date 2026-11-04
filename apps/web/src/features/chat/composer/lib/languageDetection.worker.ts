// Detects the language of pasted code with the guesslang model on TensorFlow.js, off the main
// thread.
import bundle from "@vscode/vscode-languagedetection/dist/lib/index.js?raw";
import cpuBackend from "@vscode/vscode-languagedetection/dist/lib/979.js?raw";
import modelUrl from "@vscode/vscode-languagedetection/model/model.json?url";
import weightsUrl from "@vscode/vscode-languagedetection/model/group1-shard1of1.bin?url";
import type { ModelOperations, ModelResult } from "@vscode/vscode-languagedetection";

/** The least confidence a detected language is taken with. */
const MIN_CONFIDENCE = 0.2;

/** The model's ids, by the id code blocks use. Languages the composer has no label for are left out. */
const LANGUAGES: Record<string, string> = {
  c: "c",
  cpp: "cpp",
  cs: "csharp",
  css: "css",
  dockerfile: "dockerfile",
  go: "go",
  html: "html",
  java: "java",
  js: "javascript",
  json: "json",
  kt: "kotlin",
  lua: "lua",
  makefile: "make",
  md: "markdown",
  php: "php",
  ps1: "powershell",
  py: "python",
  rb: "ruby",
  rs: "rust",
  sh: "bash",
  sql: "sql",
  swift: "swift",
  toml: "toml",
  ts: "typescript",
  xml: "xml",
  yaml: "yaml",
};

/**
 * Corrections to the model's confidence: common languages get a little more, and
 * languages that simple text is often mistaken for need much more.
 */
function adjusted({ languageId, confidence }: ModelResult): number {
  if (["js", "html", "json", "ts", "css", "py", "xml", "php"].includes(languageId)) {
    return confidence + 0.05;
  }
  if (["cpp", "sh", "java", "cs", "c"].includes(languageId)) return confidence + 0.025;
  if (["bat", "ini", "makefile", "sql", "csv", "toml"].includes(languageId))
    return confidence - 0.5;
  return confidence;
}

/**
 * Whether the model's best guess is taken: confident enough before and after the
 * corrections, and clearly ahead of the guesses close behind it.
 */
function isClearWinner([top, ...rest]: ModelResult[]): boolean {
  if (!top || top.confidence < MIN_CONFIDENCE || adjusted(top) < MIN_CONFIDENCE) return false;
  let kept = adjusted(top);
  for (const result of rest) {
    const confidence = adjusted(result);
    if (kept - confidence >= MIN_CONFIDENCE) return true;
    if (confidence <= MIN_CONFIDENCE) return false;
    kept = confidence;
  }
  return false;
}

/**
 * Runs the package's webpack bundle, which loads its CPU backend with a CommonJS `require` that
 * the browser lacks.
 */
function load(source: string, require: (id: string) => unknown): Record<string, unknown> {
  const module = { exports: {} as Record<string, unknown> };
  new Function("module", "exports", "require", source)(module, module.exports, require);
  return module.exports;
}

let model: ModelOperations | null = null;

function getModel(): ModelOperations {
  if (model) return model;
  const backend = load(cpuBackend, () => ({}));
  const exports = load(bundle, (id) => (id === "./979.js" ? backend : {}));
  const Operations = exports.ModelOperations as typeof ModelOperations;
  model = new Operations({
    modelJsonLoaderFunc: async () => (await fetch(modelUrl)).json(),
    weightsLoaderFunc: async () => (await fetch(weightsUrl)).arrayBuffer(),
  });
  return model;
}

/** Replies with the model's guess, and whether that guess is taken. */
self.onmessage = async (e: MessageEvent<{ id: number; text: string }>) => {
  const { id, text } = e.data;
  let reply: { guess: string | null; clear: boolean } = { guess: null, clear: false };
  try {
    // The first 10,000 characters are enough to tell.
    const results = await getModel().runModel(text.slice(0, 10_000));
    const top = results[0];
    reply = {
      guess: top ? (LANGUAGES[top.languageId] ?? null) : null,
      clear: isClearWinner(results),
    };
  } catch {
    // The model did not run, so the code stays unlabelled.
  }
  self.postMessage({ id, ...reply });
};
