// Stands in for the Rust backend so the workspace renders without it: default settings,
// and two sample chats, one idle and one streaming, so task rows can be checked.
const settings = { onboarded: true, pluginsDir: null, disabledPlugins: [], computerUse: true, skillsDir: null, disabledSkills: ["review-ui"], theme: "dark", agents: ["claude"], agentPaths: {}, integrations: { github: false, terminal: false, figma: false, browser: false }, projects: [{ id: "qa-project", name: "QA", path: "/tmp/qa" }, { id: "qa-other", name: "Other", path: "/tmp/other" }], activeProjectId: "qa-project", shell: null, notifications: { goalComplete: true, taskComplete: true }, updates: { checkEveryMinutes: 10 }, tokens: { showUsage: false, warnAbove: 150000, askBeforeResume: true, defaultModels: {} }, terminal: { shellArgs: null, env: {}, inheritEnv: true, cwd: null, detectLocale: "auto", fontFamily: null, fontSize: 14, lineHeight: 1.1, letterSpacing: 0, fontWeight: "normal", fontWeightBold: "bold", cursorStyle: "block", cursorBlinking: false, cursorWidth: 1, scrollback: 1000, copyOnSelection: false, rightClickBehavior: "default", altClickMovesCursor: true, macOptionIsMeta: false, drawBoldTextInBrightColors: true, minimumContrastRatio: 4.5, smoothScrolling: false, mouseWheelScrollSensitivity: 1, fastScrollSensitivity: 5 }, logging: { app: "info", window: "info", agents: "info", imports: "info" }, permissions: { everywhere: { defaults: {}, rules: [] }, chats: { defaults: {}, rules: [] }, projects: {}, seeded: true } };
const now = Date.now();
const message = (status, createdAt, files = []) => ({ id: `m${createdAt}`, role: "assistant", text: "Working on it", parts: [], createdAt, finishedAt: null, status, tools: [], plan: [], files, approvals: [], attachments: [], background: [], error: null, steer: false, queued: false, usage: null });
const chat = (id, title, messages, createdAt) => ({ id, projectId: "qa-project", title, agent: "claude", model: null, accessMode: "full-access", effort: null, sessionId: null, createdAt, messages, goal: null, folderId: null, archivedAt: null });
const threads = [
  { ...chat("qa-idle", "Fix the stripe alignment", [message("complete", now - 3600_000, ["src/a.ts", "src/b.ts"])], now - 3600_000), role: "dev-engineer" },
  { ...chat("qa-working", "Refactor the diff view", [message("streaming", now - 60_000)], now - 60_000), folderId: "qa-folder" },
];
const folders = [{ id: "qa-folder", projectId: "qa-project", name: "Old" }];
const responses = { get_settings: settings, update_status: { currentVersion: "0.8.0", canInstall: true, state: { status: "idle" } }, list_threads: threads, list_folders: folders, get_last_thread: null, save_pasted_image: { kind: "image", path: "/tmp/pasted-image.png", name: "Pasted image.png" }, "plugin:app|version": "0.4.8", save_pasted_text: { kind: "file", path: "/tmp/pasted-text.txt", name: "Pasted text.txt" }, get_collapsed: [], list_api_providers: [{ id: "deepseek", name: "DeepSeek", preset: "deepseek", baseUrl: "https://api.deepseek.com", enabled: true, models: [{ id: "deepseek-chat", name: null }, { id: "deepseek-reasoner", name: null }], headers: [], keyEnv: null, keyStore: "keychain", logo: null, hasKey: true }], fetch_api_models: [{ id: "deepseek-chat", name: null }, { id: "deepseek-reasoner", name: null }], git_changes: [], is_folder: true, list_shells: { shells: ["/usr/bin/bash", "/usr/bin/fish", "/usr/bin/zsh"], default: "/usr/bin/zsh" }, list_skills: { dir: "/home/qa/.config/patchwork/skills", skills: [{ name: "release-notes", description: "Draft release notes from the commits since the last tag", path: "/home/qa/.config/patchwork/skills/release-notes/SKILL.md", enabled: true }, { name: "review-ui", description: "Measure and screenshot UI changes before and after", path: "/home/qa/.config/patchwork/skills/review-ui/SKILL.md", enabled: false }] } };
// What the new panes read: two plugins, a scheduled task, and the Codex CLI.
responses.list_plugins = [
  {
    id: "patchwork",
    name: "Patchwork",
    version: null,
    description: "Patchwork's own hooks, in hooks.json beside this file.",
    path: "/home/qa/.config/patchwork/plugins/patchwork",
    hooks: [{ event: "stop", handler: { type: "command", command: "printf '%s %s\\n' \"$PATCHWORK_AGENT\" \"$PATCHWORK_STATUS\" >> .patchwork-turns.log", timeoutSecs: 10 } }],
    apps: [],
    enabled: true,
  },
  {
    id: "design-sync",
    name: "Design Sync",
    version: "0.3.0",
    description: null,
    path: "/home/qa/.config/patchwork/plugins/design-sync",
    hooks: [
      { event: "userPromptSubmit", handler: { type: "mcpTool", server: "figma", tool: "get_selection" } },
      { event: "stop", handler: { type: "prompt" } },
      { event: "sessionEnd", handler: { type: "unsupported", kind: "webhook" } },
    ],
    apps: [{ name: "figma", id: "asdk_figma", required: true }, { name: "linear", id: "asdk_linear", required: false }],
    enabled: false,
  },
];
const run = (ago, outcome, detail = null) => ({ at: now - ago, threadId: outcome === "started" ? `t${ago}` : null, outcome, detail });
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
    lastRunAt: now - 3_600_000,
    nextRunAt: now + 86_400_000,
    createdAt: now - 86_400_000,
    recentRuns: [run(3_600_000, "started"), run(90_000_000, "failed", "Claude Code is not installed"), run(400_000_000, "skipped", "Its time went by more than a day ago"), run(500_000_000, "started")],
  },
  {
    id: "qa-task-new",
    name: "Dependency check",
    prompt: "List outdated dependencies.",
    schedule: { type: "daily", time: "07:30" },
    projectId: null,
    agent: "claude",
    model: null,
    accessMode: "supervised",
    enabled: true,
    lastRunAt: null,
    nextRunAt: now + 43_200_000,
    createdAt: now - 3_600_000,
    recentRuns: [],
  },
];
// The panel's terminal starts a shell; nothing runs, but the panel renders as it would.
responses.terminal_spawn = { id: 1, name: "zsh", cwd: "/tmp/qa" };
responses.terminal_write = null;
responses.terminal_resize = null;
responses.terminal_kill = null;
// What the Screen pane shows: a drawn stand-in for a 2560×1440 screen, captured at half size.
responses.computer_screenshot = {
  dataUrl: "data:image/svg+xml," + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720"><rect width="1280" height="720" fill="#2b3a55"/><rect x="80" y="60" width="760" height="480" rx="8" fill="#f4f4f5"/><rect x="80" y="60" width="760" height="36" rx="8" fill="#d4d4d8"/><text x="110" y="160" font-family="sans-serif" font-size="40" fill="#18181b">A window on the screen</text></svg>'),
  width: 1280,
  height: 720,
  screenWidth: 2560,
  screenHeight: 1440,
};
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
