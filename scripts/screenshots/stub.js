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
    { id: "p1", name: "router-firmware", path: "/home/you/code/router-firmware" },
    { id: "p2", name: "save-editor", path: "/home/you/code/save-editor" },
    { id: "p3", name: "relay-server", path: "/home/you/code/relay-server" },
    { id: "p4", name: "dotfiles", path: "/home/you/code/dotfiles" },
  ];

  /** A message, shaped like the backend's. */
  const msg = (o) => ({
    id: o.id,
    role: o.role,
    text: o.text,
    createdAt: now - (o.ago ?? 0),
    status: o.status ?? "complete",
    tools: o.tools ?? [],
    plan: [],
    files: o.files ?? [],
    error: null,
    approvals: [],
    attachments: [],
    steer: false,
    queued: false,
    background: [],
    cutOff: false,
    refusal: null,
    parts: o.parts ?? [{ type: "text", text: o.text }],
    finishedAt: o.ago ? now - o.ago + 40_000 : null,
    usage: o.usage ?? null,
    turn: o.turn ?? null,
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

  /** A tool call, as the timeline draws it. */
  const tool = (o) => ({
    id: o.id,
    name: o.name,
    summary: o.summary,
    status: o.status ?? "done",
    subagent: null,
    parent: null,
    startedAt: now - (o.ago ?? 60_000),
    endedAt: o.status === "running" ? null : now - (o.ago ?? 60_000) + 4_000,
    output: null,
  });

  // The firmware header's checksum, still being worked out: the work rows, then the words landing.
  const reply = [
    "The checksum at `0x1C` is a CRC-32 over the payload only, not the header. It starts at",
    "`0xFFFFFFFF` and skips the final XOR, which is why the standard `crc32` never matched.",
    "",
    "The unpacker now checks it before extracting anything, and refuses an image whose checksum",
    "is off, with the expected and actual values in the error.",
    "",
    "All three images in `samples/` pass, and a copy with one flipped byte is turned away.",
  ].join("\n");
  const checksum = thread({
    id: "t0",
    title: "Work out the update image checksum",
    ago: 12 * 60_000,
    messages: [
      msg({
        id: "c1",
        role: "user",
        text: "The router's update images start with a 32-byte header we can't validate. Can you work out the checksum so the unpacker can verify an image before extracting it?",
        ago: 12 * 60_000,
      }),
      msg({
        id: "c2",
        role: "assistant",
        status: "streaming",
        ago: 40_000,
        text: reply,
        tools: [
          tool({ id: "k1", name: "Read", summary: "src/header.rs", ago: 38_000 }),
          tool({ id: "k2", name: "Grep", summary: "crc", ago: 30_000 }),
          tool({ id: "k3", name: "Edit", summary: "src/header.rs", ago: 22_000 }),
          tool({ id: "k4", name: "Bash", summary: "cargo test header", status: "running", ago: 8_000 }),
        ],
        parts: [
          { type: "thinking", text: "Find where the header is parsed.", startedAt: now - 39_000, endedAt: now - 36_000 },
          { type: "tool", id: "k1" },
          { type: "tool", id: "k2" },
          { type: "tool", id: "k3" },
          { type: "tool", id: "k4" },
          { type: "text", text: reply },
        ],
      }),
    ],
  });

  // A finished turn, with the files it touched and the tokens it spent.
  const saveFormat = thread({
    id: "t1",
    projectId: "p2",
    title: "Saves from 1.4 crash the inventory",
    ago: 55 * 60_000,
    messages: [
      msg({
        id: "s1",
        role: "user",
        text: "Saves from the 1.4 patch crash the editor as soon as it loads the inventory. Can you figure out what changed in the format?",
        ago: 55 * 60_000,
      }),
      msg({
        id: "s2",
        role: "assistant",
        ago: 40 * 60_000,
        files: ["src/format/inventory.ts", "src/format/version.ts", "docs/save-format.md"],
        turn: { before: "a1b2c3", after: "d4e5f6" },
        usage: {
          input: 3_900,
          cacheRead: 121_000,
          cacheWrite: 16_200,
          output: 1_800,
          context: 124_100,
        },
        text: [
          "1.4 added a two-byte durability value to every item, so an inventory slot is now 14 bytes",
          "instead of 12, and the reader drifted out of step after the first item.",
          "",
          "- The format version at `0x04` went from 7 to 8, and the parser picks the slot size from it.",
          "- Older saves load exactly as before, and 1.4 saves round-trip byte for byte.",
          "- A version the editor doesn't know shows an error instead of crashing.",
        ].join("\n"),
      }),
    ],
  });

  const threads = [
    checksum,
    saveFormat,
    thread({
      id: "t2",
      title: "Map the UART commands in the update handler",
      ago: 5 * 60 * 60_000,
      messages: [
        msg({
          id: "m1",
          role: "user",
          text: "The update handler reads commands from the serial port. Can you map out what each one does?",
          ago: 5 * 60 * 60_000,
        }),
        msg({
          id: "m2",
          role: "assistant",
          ago: 40 * 60_000,
          text: "The handler reads one command byte and jumps through a table of 12 entries. All 12 are named now, and `docs/uart.md` lists their arguments. `0x0B` is an undocumented memory dump that only answers before the bootloader locks flash.",
        }),
      ],
    }),
    thread({
      id: "t3",
      projectId: "p2",
      title: "Diff the 1.3 and 1.4 save routines",
      ago: 1 * DAY,
      messages: [
        msg({
          id: "m5",
          role: "user",
          text: "Compare how 1.3 and 1.4 write the save file, so we know which blocks changed.",
          ago: 1 * DAY,
        }),
        msg({
          id: "m6",
          role: "assistant",
          ago: 1 * DAY - 60_000,
          text: "Only the inventory writer changed: it now writes a durability value after each item's count. The header, the map flags and the quest block are written by the same code in both versions.",
        }),
      ],
    }),
    thread({
      id: "t4",
      projectId: "p3",
      title: "Unknown build ids should 404",
      ago: 2 * DAY,
      messages: [
        msg({
          id: "m3",
          role: "user",
          text: "GET /builds/:id returns an empty 200 for ids that don't exist. It should be a 404.",
          ago: 2 * DAY,
        }),
        msg({
          id: "m4",
          role: "assistant",
          ago: 2 * DAY - 30_000,
          text: "The handler returned whatever the query found, including nothing. It now answers 404 when no build matches, and the same for artifacts and logs, which had the same bug.",
        }),
      ],
    }),
  ];

  const settings = {
    onboarded: true,
    theme: "dark",
    agents: ["claude", "codex"],
    integrations: { github: false, terminal: false, figma: false, browser: false },
    projects,
    activeProjectId: MODE === "new" ? "p4" : "p1",
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
    chatBackground: { kind: "none" },
    chatBackgroundStrength: "medium",
    systemPrompts: {},
    steerWhileWorking: false,
    checks: { afterEveryTask: false, projects: {} },
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
    list_background_images: [],
    list_models: [],
    checkpoints: [],
    git_branch: "main",
    thread_usage: null,
    turn_files: (args) =>
      (args?.paths ?? []).map((path, i) => ({
        path,
        additions: [46, 18, 9][i] ?? 4,
        deletions: [21, 12, 0][i] ?? 0,
        state: "uncommitted",
      })),
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
    git_changes: [
      { path: "src/format/inventory.ts", status: "modified", additions: 46, deletions: 21, staged: false },
      { path: "src/format/version.ts", status: "modified", additions: 18, deletions: 12, staged: false },
      { path: "docs/save-format.md", status: "added", additions: 9, deletions: 0, staged: true },
    ],
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
