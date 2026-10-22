// Stands in for the Rust backend so the workspace renders without it: default settings,
// and two sample chats, one idle and one streaming, so task rows can be checked.
const settings = { onboarded: true, theme: "dark", agents: ["claude"], integrations: { github: false, terminal: false, figma: false, browser: false }, projects: [{ id: "qa-project", name: "QA", path: "/tmp/qa" }, { id: "qa-other", name: "Other", path: "/tmp/other" }], activeProjectId: "qa-project", shell: null, notifications: { goalComplete: true, taskComplete: true }, tokens: { showUsage: false, warnAbove: 150000, askBeforeResume: true, autoCompact: true, autoCompactAbove: 200000, defaultModels: {} } };
const now = Date.now();
const message = (status, createdAt, files = []) => ({ id: `m${createdAt}`, role: "assistant", text: "Working on it", parts: [], createdAt, finishedAt: null, status, tools: [], plan: [], files, approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null });
const chat = (id, title, messages, createdAt) => ({ id, projectId: "qa-project", title, agent: "claude", model: null, accessMode: "full-access", effort: null, sessionId: null, createdAt, messages, goal: null, folderId: null, archivedAt: null });
const threads = [
  chat("qa-idle", "Fix the stripe alignment", [message("complete", now - 3600_000, ["src/a.ts", "src/b.ts"])], now - 3600_000),
  { ...chat("qa-working", "Refactor the diff view", [message("streaming", now - 60_000)], now - 60_000), folderId: "qa-folder" },
];
const folders = [{ id: "qa-folder", projectId: "qa-project", name: "Old" }];
const responses = { get_settings: settings, list_threads: threads, list_folders: folders, get_last_thread: null, save_pasted_image: { kind: "image", path: "/tmp/pasted-image.png", name: "Pasted image.png" }, get_collapsed: [], git_changes: [], list_shells: { shells: ["/usr/bin/bash", "/usr/bin/fish", "/usr/bin/zsh"], default: "/usr/bin/zsh" } };
window.__TAURI_INTERNALS__ = {
  invoke: async (cmd, args) => (cmd === "get_thread" ? threads.find((t) => t.id === args.id) : cmd in responses ? responses[cmd] : null),
  transformCallback: () => 0,
  unregisterCallback: () => {},
  convertFileSrc: (p) => p,
  metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
};
