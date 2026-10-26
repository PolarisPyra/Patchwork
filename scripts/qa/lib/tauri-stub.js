// Stands in for the Rust backend so the workspace renders without it: default settings,
// and two sample chats, one idle and one streaming, so task rows can be checked.
const settings = { onboarded: true, skillsDir: null, disabledSkills: ["review-ui"], theme: "dark", agents: ["claude"], agentPaths: {}, integrations: { github: false, terminal: false, figma: false, browser: false }, projects: [{ id: "qa-project", name: "QA", path: "/tmp/qa" }, { id: "qa-other", name: "Other", path: "/tmp/other" }], activeProjectId: "qa-project", shell: null, notifications: { goalComplete: true, taskComplete: true }, updates: { checkEveryMinutes: 10 }, tokens: { showUsage: false, warnAbove: 150000, askBeforeResume: true, autoCompact: true, autoCompactAbove: 200000, defaultModels: {} } };
const now = Date.now();
const message = (status, createdAt, files = []) => ({ id: `m${createdAt}`, role: "assistant", text: "Working on it", parts: [], createdAt, finishedAt: null, status, tools: [], plan: [], files, approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null });
const chat = (id, title, messages, createdAt) => ({ id, projectId: "qa-project", title, agent: "claude", model: null, accessMode: "full-access", effort: null, sessionId: null, createdAt, messages, goal: null, folderId: null, archivedAt: null });
const threads = [
  chat("qa-idle", "Fix the stripe alignment", [message("complete", now - 3600_000, ["src/a.ts", "src/b.ts"])], now - 3600_000),
  { ...chat("qa-working", "Refactor the diff view", [message("streaming", now - 60_000)], now - 60_000), folderId: "qa-folder" },
];
const folders = [{ id: "qa-folder", projectId: "qa-project", name: "Old" }];
const responses = { get_settings: settings, update_status: { currentVersion: "0.8.0", canInstall: true, state: { status: "idle" } }, list_threads: threads, list_folders: folders, get_last_thread: null, save_pasted_image: { kind: "image", path: "/tmp/pasted-image.png", name: "Pasted image.png" }, "plugin:app|version": "0.4.8", save_pasted_text: { kind: "file", path: "/tmp/pasted-text.txt", name: "Pasted text.txt" }, get_collapsed: [], list_api_providers: [{ id: "deepseek", name: "DeepSeek", preset: "deepseek", baseUrl: "https://api.deepseek.com", enabled: true, models: [{ id: "deepseek-chat", name: null }, { id: "deepseek-reasoner", name: null }], headers: [], keyEnv: null, keyStore: "keychain", logo: null, hasKey: true }], fetch_api_models: [{ id: "deepseek-chat", name: null }, { id: "deepseek-reasoner", name: null }], git_changes: [], is_folder: true, list_shells: { shells: ["/usr/bin/bash", "/usr/bin/fish", "/usr/bin/zsh"], default: "/usr/bin/zsh" }, list_skills: { dir: "/home/qa/.config/patchwork/skills", skills: [{ name: "release-notes", description: "Draft release notes from the commits since the last tag", path: "/home/qa/.config/patchwork/skills/release-notes/SKILL.md", enabled: true }, { name: "review-ui", description: "Measure and screenshot UI changes before and after", path: "/home/qa/.config/patchwork/skills/review-ui/SKILL.md", enabled: false }] } };
window.__TAURI_INTERNALS__ = {
  invoke: async (cmd, args) => (cmd === "get_thread" ? threads.find((t) => t.id === args.id) : cmd in responses ? responses[cmd] : null),
  transformCallback: () => 0,
  unregisterCallback: () => {},
  convertFileSrc: (p) => p,
  metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
};
