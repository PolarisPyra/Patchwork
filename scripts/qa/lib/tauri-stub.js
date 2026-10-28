// Stands in for the Rust backend so the workspace renders without it: default settings,
// and two sample chats, one idle and one streaming, so task rows can be checked.
const settings = { onboarded: true, pluginsDir: null, disabledPlugins: [], computerUse: true, skillsDir: null, disabledSkills: ["review-ui"], theme: "dark", agents: ["claude"], agentPaths: {}, integrations: { github: false, terminal: false, figma: false, browser: false }, projects: [{ id: "qa-project", name: "QA", path: "/tmp/qa" }, { id: "qa-other", name: "Other", path: "/tmp/other" }], activeProjectId: "qa-project", shell: null, notifications: { goalComplete: true, taskComplete: true }, updates: { checkEveryMinutes: 10 }, tokens: { showUsage: false, warnAbove: 150000, askBeforeResume: true, defaultModels: {} }, logging: { app: "info", window: "info", agents: "info", imports: "info" }, permissions: { everywhere: { defaults: {}, rules: [] }, chats: { defaults: {}, rules: [] }, projects: {}, seeded: true } };
const now = Date.now();
const message = (status, createdAt, files = []) => ({ id: `m${createdAt}`, role: "assistant", text: "Working on it", parts: [], createdAt, finishedAt: null, status, tools: [], plan: [], files, approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null });
const chat = (id, title, messages, createdAt) => ({ id, projectId: "qa-project", title, agent: "claude", model: null, accessMode: "full-access", effort: null, sessionId: null, createdAt, messages, goal: null, folderId: null, archivedAt: null });
const threads = [
  { ...chat("qa-idle", "Fix the stripe alignment", [message("complete", now - 3600_000, ["src/a.ts", "src/b.ts"])], now - 3600_000), role: "dev-engineer" },
  { ...chat("qa-working", "Refactor the diff view", [message("streaming", now - 60_000)], now - 60_000), folderId: "qa-folder" },
];
const folders = [{ id: "qa-folder", projectId: "qa-project", name: "Old" }];
const responses = { get_settings: settings, update_status: { currentVersion: "0.8.0", canInstall: true, state: { status: "idle" } }, list_threads: threads, list_folders: folders, get_last_thread: null, save_pasted_image: { kind: "image", path: "/tmp/pasted-image.png", name: "Pasted image.png" }, "plugin:app|version": "0.4.8", save_pasted_text: { kind: "file", path: "/tmp/pasted-text.txt", name: "Pasted text.txt" }, get_collapsed: [], list_api_providers: [{ id: "deepseek", name: "DeepSeek", preset: "deepseek", baseUrl: "https://api.deepseek.com", enabled: true, models: [{ id: "deepseek-chat", name: null }, { id: "deepseek-reasoner", name: null }], headers: [], keyEnv: null, keyStore: "keychain", logo: null, hasKey: true }], fetch_api_models: [{ id: "deepseek-chat", name: null }, { id: "deepseek-reasoner", name: null }], git_changes: [], is_folder: true, list_shells: { shells: ["/usr/bin/bash", "/usr/bin/fish", "/usr/bin/zsh"], default: "/usr/bin/zsh" }, list_skills: { dir: "/home/qa/.config/patchwork/skills", skills: [{ name: "release-notes", description: "Draft release notes from the commits since the last tag", path: "/home/qa/.config/patchwork/skills/release-notes/SKILL.md", enabled: true }, { name: "review-ui", description: "Measure and screenshot UI changes before and after", path: "/home/qa/.config/patchwork/skills/review-ui/SKILL.md", enabled: false }] } };
// What the new panes read: a starter plugin, a scheduled task, and the Codex CLI.
responses.list_plugins = [
  {
    id: "patchwork-review",
    name: "Patchwork Review",
    version: "1.0.0",
    description: "Skills for reviewing a change before it lands, and for writing up work on a schedule.",
    path: "/home/qa/.config/patchwork/plugins/patchwork-review",
    skills: [
      { name: "review-diff", description: "Review the uncommitted change in this project before it is kept.", path: "/home/qa/.config/patchwork/plugins/patchwork-review/skills/review-diff/SKILL.md", enabled: true },
      { name: "scheduled-report", description: "Write the standing report a scheduled task asks for.", path: "/home/qa/.config/patchwork/plugins/patchwork-review/skills/scheduled-report/SKILL.md", enabled: true },
    ],
    mcpServers: [{ name: "review-tools", command: "./run", args: [] }],
    enabled: true,
    bundled: true,
  },
];
responses.list_scheduled_tasks = [
  {
    id: "qa-task",
    name: "Weekly report",
    prompt: "Report on what landed this week.",
    schedule: { type: "weekdays", time: "09:00" },
    projectId: "qa-project",
    agent: "claude",
    model: null,
    accessMode: "supervised",
    enabled: true,
    lastRunAt: null,
    nextRunAt: now + 86_400_000,
    createdAt: now - 86_400_000,
  },
];
responses.codex_info = {
  path: "/usr/lib/chatgpt/resources/codex",
  version: "codex-cli 0.163.0-alpha.11.1",
  features: [
    { name: "browser_use", stage: "stable", enabled: true },
    { name: "computer_use", stage: "stable", enabled: true },
    { name: "goals", stage: "stable", enabled: true },
    { name: "worktrees", stage: "stable", enabled: true },
    { name: "artifact", stage: "under development", enabled: false },
    { name: "chronicle", stage: "under development", enabled: false },
    { name: "sqlite", stage: "removed", enabled: true },
  ],
  models: [
    { slug: "gpt-6.1-sol", displayName: "GPT-6.1-Sol", contextWindow: 500000, efforts: ["low", "medium", "high", "xhigh", "max", "ultra"], speedTiers: ["Fast (2x speed, increased usage)"], multiAgent: "v2" },
    { slug: "gpt-5.6-terra", displayName: "GPT-5.6-Terra", contextWindow: 272000, efforts: ["low", "medium", "high"], speedTiers: [], multiAgent: "v2" },
  ],
};
responses.codex_command = "codex doctor\n\nconfig.toml        ok\nauth.json          ok\nnode               /usr/bin/node\n";
responses.watch_skills = responses.list_skills;
responses.watch_agents = { dir: "/home/qa/.config/patchwork/agents", agents: [{ name: "dev-engineer", title: "DevEngineer", description: "Senior developer: plans, implements features and fixes with explicit risk control.", model: "claude-sonnet-5-5", primary: true, path: "/home/qa/.config/patchwork/agents/dev-engineer.md" }, { name: "doc-scout", title: null, description: "Repository scanner: discovers stack, modules, APIs, DB schema, and documentation gaps.", model: null, primary: false, path: "/home/qa/.config/patchwork/agents/doc-scout.md" }] };
// The project's own agents come with the folder's.
responses.list_agents = [...responses.watch_agents.agents, { name: "release-captain", title: null, description: "Project agent: cuts releases.", model: null, primary: false, path: "/home/qa/project/.patchwork/agents/release-captain.md" }];
responses.list_files = ["README.md", "src/main.rs"];
window.__TAURI_INTERNALS__ = {
  invoke: async (cmd, args) => (cmd === "get_thread" ? threads.find((t) => t.id === args.id) : cmd in responses ? responses[cmd] : null),
  transformCallback: () => 0,
  unregisterCallback: () => {},
  convertFileSrc: (p) => p,
  metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
};
