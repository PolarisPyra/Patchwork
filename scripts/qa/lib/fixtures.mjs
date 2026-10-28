// Canned data and command mocks for the checks. `installMocks` runs before the workspace loads, so
// it replaces the Tauri backend for the commands a check needs and leaves the rest to the stub.

/** A chat message with every field the UI reads, so a check states only what it shows. */
export function message(fields) {
  return {
    plan: [], approvals: [], attachments: [], background: [], error: null, steer: false,
    queued: false, usage: null, tools: [], files: [], parts: [], ...fields,
  };
}

/**
 * Mocks the page's Tauri `invoke` for the next page load.
 * - `mocks` maps a command to `(args, data) => result`. The functions run in the page, so they
 *   read `args` and `data` and define anything else they need inside themselves.
 * - `data` is plain JSON that every mock and `convertFileSrc` receives as its last argument.
 * - `threads` maps a thread id to the messages `list_threads` reports for it.
 * - `convertFileSrc` maps a path to the URL the webview loads it from: `(path, data) => url`.
 * Call it before `page.reload`, after `openWorkspace`.
 */
export async function installMocks(page, { mocks = {}, data = {}, threads = {}, convertFileSrc = null }) {
  await page.addInitScript(applyMocks, {
    mocks: Object.fromEntries(Object.entries(mocks).map(([cmd, fn]) => [cmd, fn.toString()])),
    data,
    threads,
    convertFileSrc: convertFileSrc?.toString() ?? null,
  });
}

// Runs in the page, where the functions arrive as source text.
function applyMocks({ mocks, data, threads, convertFileSrc }) {
  const internals = window.__TAURI_INTERNALS__;
  const invoke = internals.invoke;
  const fns = Object.fromEntries(
    Object.entries(mocks).map(([cmd, src]) => [cmd, new Function(`return (${src});`)()]),
  );
  if (convertFileSrc) {
    const convert = new Function(`return (${convertFileSrc});`)();
    internals.convertFileSrc = (path) => convert(path, data);
  }
  internals.invoke = async (cmd, args) => {
    if (cmd in fns) return fns[cmd](args, data);
    const result = await invoke(cmd, args);
    if (cmd !== "list_threads") return result;
    return result.map((t) => (t.id in threads ? { ...t, messages: threads[t.id] } : t));
  };
}
