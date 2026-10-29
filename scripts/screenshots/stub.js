// Canned backend for the real app, so the workspace renders for a screenshot.
//
// The shots are of the app itself — same components, same CSS — with made-up projects and threads
// instead of a person's real ones. Every command the boot path asks for is answered here; anything
// else returns null.
(function () {
  const MODE = window.__SHOT_MODE__ ?? "goal";
  // The page has no window chrome of its own, so the page itself is the window's background.
  document.addEventListener("DOMContentLoaded", () => {
    document.documentElement.style.background = "#1e1f22";
    document.body.style.margin = "0";
  });
  const now = Date.now();
  const DAY = 86_400_000;

  const projects = [
    { id: "p1", name: "checkout", path: "/home/you/code/checkout" },
    { id: "p2", name: "landing-site", path: "/home/you/code/landing-site" },
    { id: "p3", name: "notes-app", path: "/home/you/code/notes-app" },
  ];

  /** A message, shaped like the backend's. */
  const msg = (o) => ({
    id: o.id,
    role: o.role,
    text: o.text,
    createdAt: now - (o.ago ?? 0),
    status: o.status ?? "complete",
    tools: [],
    plan: [],
    files: [],
    error: null,
    approvals: [],
    attachments: [],
    steer: false,
    queued: false,
    background: [],
    cutOff: false,
    refusal: null,
    parts: [{ type: "text", text: o.text }],
    finishedAt: o.ago ? now - o.ago + 40_000 : null,
    usage: null,
    turn: null,
  });

  const thread = (o) => ({
    id: o.id,
    projectId: o.projectId ?? "p1",
    title: o.title,
    agent: "claude",
    model: null,
    accessMode: "supervised",
    effort: null,
    sessionId: null,
    createdAt: now - (o.ago ?? 0),
    messages: o.messages ?? [],
    goal: o.goal ?? null,
    folderId: null,
    archivedAt: null,
    parent: null,
    planMode: false,
    slashCommands: [],
    importedFrom: null,
    contextPending: false,
    role: null,
  });

  const goalThread = thread({
    id: "t1",
    title: "Tidy up the release scripts",
    ago: 3 * 60_000,
    goal: {
      objective: "Make every release script take the version out of a bundle's name",
      status: "pursuing",
      startedAt: now - 2 * 60_000,
      checks: 1,
      tokensUsed: null,
      tokenBudget: null,
      timeUsedSeconds: null,
    },
    messages: [
      msg({
        id: "m1",
        role: "user",
        text: "/goal Make every release script take the version out of a bundle's name",
        ago: 3 * 60_000,
      }),
      msg({
        id: "m2",
        role: "assistant",
        status: "streaming",
        ago: 2 * 60_000,
        text: "I'll start by reading the release script to see where each bundle gets its name.",
      }),
    ],
  });


  const showcase = thread({
    id: "t0",
    title: "Make the download links stable",
    ago: 40 * 60_000,
    messages: [
      msg({
        id: "s1",
        role: "user",
        text: "The release links break every version. Make the installer names stable.",
        ago: 40 * 60_000,
      }),
      msg({
        id: "s2",
        role: "assistant",
        ago: 30 * 60_000,
        text: [
          "The name came from Tauri, which puts the version in it. I strip it after the build:",
          "",
          "```sh",
          'name=${name//_${version}_/_}',
          "```",
          "",
          "That leaves one name per platform, so the links hold across releases:",
          "",
          "| Platform | File |",
          "| --- | --- |",
          "| Linux | `Patchwork_amd64.AppImage` |",
          "| Windows | `Patchwork_x64-setup.exe` |",
          "| macOS | `Patchwork_aarch64.dmg` |",
          "",
          "The updater picks its file by suffix, so nothing about updating changes.",
        ].join("\n"),
        usage: { input: 4200, cacheRead: 131_000, cacheWrite: 18_400, output: 2_100, context: 131_400 },
        files: ["scripts/bundle-names.sh", "scripts/release.sh"],
      }),
    ],
  });

  const threads = [
    showcase,
    goalThread,
    thread({
      id: "t2",
      title: "Review the diff view",
      ago: DAY,
      messages: [
        msg({ id: "m3", role: "user", text: "Show me what changed in the canvas.", ago: DAY }),
        msg({
          id: "m4",
          role: "assistant",
          ago: DAY - 60_000,
          text: "The canvas lists every file the turn touched, and rejecting one restores it.",
        }),
      ],
    }),
    thread({
      id: "t3",
      projectId: "p2",
      title: "Fix the broken link",
      ago: 2 * DAY,
      messages: [msg({ id: "m5", role: "user", text: "The docs link is 404.", ago: 2 * DAY })],
    }),
  ];

  const settings = {
    onboarded: true,
    theme: "dark",
    agents: ["claude", "codex"],
    integrations: { github: false, terminal: false, figma: false, browser: false },
    projects,
    activeProjectId: MODE === "new" ? "p3" : "p1",
    agent: null,
    importLogDisabled: false,
    agentPaths: {},
    shell: null,
    skillsDir: null,
    disabledSkills: [],
    visionModel: null,
    tokens: {
      showUsage: true,
      warnAbove: 150000,
      askBeforeResume: true,
      claudeCompactWindow: 200000,
      compactWindowDefaulted: true,
      defaultModels: {},
      generateTitles: true,
    },
    notifications: { goalComplete: true, taskComplete: true },
    updates: { checkEveryMinutes: 10 },
    logo: null,
    logging: { app: "info", window: "info", agents: "info", imports: "info" },
    terminal: null,
    permissions: {
      everywhere: { defaults: {}, rules: [] },
      chats: { defaults: {}, rules: [] },
      projects: {},
      seeded: true,
    },
    confirmAnswers: false,
    pluginsDir: null,
    disabledPlugins: [],
    computerUse: false,
  };

  const usage = {
    kind: "claude",
    plan: "max 5x",
    windows: [
      { label: "Session (5h)", usedPercent: 32, resetsAt: now + 2 * 3600_000 },
      { label: "Weekly", usedPercent: 14, resetsAt: now + 5 * DAY },
    ],
    error: null,
    updatedAt: now,
  };

  const responses = {
    // The watchers hand back an initial view; the app reads fields off it straight away.
    watch_agents: { dir: "", agents: [] },
    list_agents: [],
    watch_skills: { dir: "", skills: [] },
    list_skills: { dir: "", skills: [] },
    list_plugins: [],
    list_mcp_servers: [],
    list_cli_mcp_servers: [],
    list_scheduled_tasks: [],
    list_shells: { shells: [], defaultShell: null },
    list_files: ["README.md", "src/index.ts"],
    thread_usage: null,
    turn_files: [],
    is_folder: true,
    missing_folder: false,
    missing_files: [],
    get_settings: settings,
    save_settings: null,
    list_projects: projects,
    list_threads: threads,
    list_folders: [],
    get_last_thread: MODE === "new" ? null : "t0",
    set_last_thread: null,
    get_thread: (args) => threads.find((t) => t.id === args?.id) ?? threads[0],
    get_collapsed: [],
    set_collapsed: null,
    git_changes: [],
    git_status: { branch: "main", ahead: 0, behind: 0, staged: [], unstaged: [], untracked: [] },
    list_agent_status: [
      { kind: "claude", installed: true, version: "2.0.1", signedIn: true, detail: "Signed in" },
      { kind: "codex", installed: true, version: "0.51.0", signedIn: true, detail: "Signed in" },
    ],
    detect_agent: (args) => ({
      kind: args?.kind ?? "claude",
      installed: true,
      version: args?.kind === "codex" ? "0.51.0" : "2.0.1",
      signedIn: true,
      detail: "Signed in",
    }),
    get_usage: (args) => ({ ...usage, kind: args?.kind ?? "claude" }),
    update_status: {
      currentVersion: "26.1.0",
      canInstall: true,
      state: { status: "idle" },
      notes: null,
    },
    check_for_updates: null,
  };

  // Tauri's event module calls into this to drop a listener; without it every listener throws.
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => {},
    emit: () => {},
  };

  const windowMeta = { label: "main", window: { label: "main" } };
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args) => {
      if (cmd === "plugin:event|listen") return 1;
      if (cmd === "plugin:event|unlisten") return null;
      if (cmd === "plugin:event|emit") return null;
      const answer = responses[cmd];
      return typeof answer === "function" ? answer(args) : (answer ?? null);
    },
    transformCallback: () => 0,
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    metadata: {
      currentWindow: windowMeta,
      currentWebview: { label: "main", windowLabel: "main", window: windowMeta },
    },
  };
})();
