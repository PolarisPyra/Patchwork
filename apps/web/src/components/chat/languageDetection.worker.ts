// Detects the language of pasted code with the model VS Code uses for untitled files
// (@vscode/vscode-languagedetection, guesslang on TensorFlow.js), off the main thread.
import bundle from "@vscode/vscode-languagedetection/dist/lib/index.js?raw";
import cpuBackend from "@vscode/vscode-languagedetection/dist/lib/979.js?raw";
import modelUrl from "@vscode/vscode-languagedetection/model/model.json?url";
import weightsUrl from "@vscode/vscode-languagedetection/model/group1-shard1of1.bin?url";
import type { ModelOperations, ModelResult } from "@vscode/vscode-languagedetection";
import { grammarRelevance } from "~/components/canvas/highlight";

/** VS Code's minimum confidence for a detected language. */
const MIN_CONFIDENCE = 0.2;

/**
 * Programming languages the model's best guess is taken for below that confidence, when the code
 * also matches enough of the language's grammar. A few lines are too little for the model to be
 * sure; text formats that plain text is mistaken for are never taken this way.
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
 * VS Code's corrections to the model's confidence: common languages get a little more, and
 * languages that simple text is often mistaken for need much more.
 */
function adjusted({ languageId, confidence }: ModelResult): number {
  if (["js", "html", "json", "ts", "css", "py", "xml", "php"].includes(languageId)) {
    return confidence + 0.05;
  }
  if (["cpp", "sh", "java", "cs", "c"].includes(languageId)) return confidence + 0.025;
  if (["bat", "ini", "makefile", "sql", "csv", "toml"].includes(languageId)) return confidence - 0.5;
  return confidence;
}

/**
 * Runs the package's webpack bundle, which loads its CPU backend with a CommonJS `require` that
 * the browser lacks, as VS Code's loader does.
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

self.onmessage = async (e: MessageEvent<{ id: number; text: string }>) => {
  const { id, text } = e.data;
  let language: string | null = null;
  try {
    // VS Code reads the first 10,000 characters.
    const sample = text.slice(0, 10_000);
    const [top] = await getModel().runModel(sample);
    const guess = top ? (LANGUAGES[top.languageId] ?? null) : null;
    if (guess && adjusted(top) >= MIN_CONFIDENCE) language = guess;
    else if (
      guess &&
      SHORT_CODE_LANGUAGES.has(guess) &&
      grammarRelevance(sample, guess) >= MIN_GRAMMAR_RELEVANCE
    ) {
      language = guess;
    }
  } catch {
    language = null;
  }
  self.postMessage({ id, language });
};
