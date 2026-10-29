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
    { id: "p1", name: "acme-storefront", path: "/home/you/code/acme-storefront" },
    { id: "p2", name: "acme-api", path: "/home/you/code/acme-api" },
    { id: "p3", name: "acme-docs", path: "/home/you/code/acme-docs" },
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

  // The cart's promo code, still being written: the work rows, then the words landing.
  const promoCode = thread({
    id: "t0",
    title: "Add a promo code field to the cart",
    ago: 12 * 60_000,
    messages: [
      msg({
        id: "c1",
        role: "user",
        text: "Can you add a promo code field to the cart? It should tell people straight away if a code is wrong.",
        ago: 12 * 60_000,
      }),
      msg({
        id: "c2",
        role: "assistant",
        status: "streaming",
        ago: 40_000,
        text: [
          "The cart now takes a code above the total, and checks it as it is typed, so a wrong one",
          "shows an inline error rather than waiting for the customer to reach checkout.",
          "",
          "It reads the same `discounts` table the checkout already uses, so a code that works in",
          "the cart is honoured there too, and an expired one is refused in both.",
          "",
          "Nothing else about the total changes: the code is applied to the price, not the tax, and",
          "the cart keeps it if the customer refreshes or comes back later.",
        ].join("\n"),
        tools: [
          tool({ id: "k1", name: "Read", summary: "app/cart/page.tsx", ago: 38_000 }),
          tool({ id: "k2", name: "Grep", summary: "discounts", ago: 30_000 }),
          tool({ id: "k3", name: "Edit", summary: "app/cart/page.tsx", ago: 22_000 }),
          tool({ id: "k4", name: "Bash", summary: "npm test -- cart", status: "running", ago: 8_000 }),
        ],
        parts: [
          { type: "thinking", text: "Find where the cart renders the total.", startedAt: now - 39_000, endedAt: now - 36_000 },
          { type: "tool", id: "k1" },
          { type: "tool", id: "k2" },
          { type: "tool", id: "k3" },
          { type: "tool", id: "k4" },
          {
            type: "text",
            text: [
              "The cart now takes a code above the total, and checks it as it is typed, so a wrong one",
              "shows an inline error rather than waiting for the customer to reach checkout.",
              "",
              "It reads the same `discounts` table the checkout already uses, so a code that works in",
              "the cart is honoured there too, and an expired one is refused in both.",
              "",
              "Nothing else about the total changes: the code is applied to the price, not the tax, and",
              "the cart keeps it if the customer refreshes or comes back later.",
            ].join("\n"),
          },
        ],
      }),
    ],
  });

  // A finished turn, with the files it touched and the tokens it spent.
  const searchSpeed = thread({
    id: "t1",
    title: "Speed up product search",
    ago: 55 * 60_000,
    messages: [
      msg({
        id: "s1",
        role: "user",
        text: "Product search takes about a second on the catalogue page. Can you make it faster?",
        ago: 55 * 60_000,
      }),
      msg({
        id: "s2",
        role: "assistant",
        ago: 40 * 60_000,
        files: ["app/catalogue/search.ts", "app/catalogue/page.tsx", "db/migrations/0042_search_index.sql"],
        usage: {
          input: 3_900,
          cacheRead: 121_000,
          cacheWrite: 16_200,
          output: 1_800,
          context: 124_100,
        },
        text: [
          "Search was scanning every product row on each keystroke. Results now come from indexed",
          "columns, and requests wait until you pause typing, so the page answers in about 40ms.",
          "",
          "- Results keep their relevance order, and paging still works.",
          "- If the index is missing the old scan runs instead, so a checkout deploys before the",
          "  migration cannot break search.",
          "- A search of one letter no longer queries the database at all.",
        ].join("\n"),
      }),
    ],
  });

  const threads = [
    promoCode,
    searchSpeed,
    thread({
      id: "t2",
      title: "Fix the pay button in Safari",
      ago: 5 * 60 * 60_000,
      messages: [
        msg({
          id: "m1",
          role: "user",
          text: "The pay button does nothing in Safari, but it is fine in Chrome.",
          ago: 5 * 60 * 60_000,
        }),
        msg({
          id: "m2",
          role: "assistant",
          ago: 40 * 60_000,
          text: "Safari drops the form submission because the pay button is removed from the page while it is still handling the click. Switching it to a disabled state fixes it in both browsers.",
        }),
      ],
    }),
    thread({
      id: "t3",
      projectId: "p2",
      title: "Unknown ids should 404",
      ago: 2 * DAY,
      messages: [
        msg({
          id: "m3",
          role: "user",
          text: "GET /orders/:id returns an empty 200 for ids that do not exist. It should be a 404.",
          ago: 2 * DAY,
        }),
        msg({
          id: "m4",
          role: "assistant",
          ago: 2 * DAY - 30_000,
          text: "The handler returned whatever the query found, including nothing. It now answers 404 when no order matches, and the same for customers and invoices, which had the same bug.",
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
    git_changes: [
      { path: "app/catalogue/search.ts", status: "modified", additions: 46, deletions: 21, staged: false },
      { path: "app/catalogue/page.tsx", status: "modified", additions: 18, deletions: 12, staged: false },
      { path: "db/migrations/0042_search_index.sql", status: "added", additions: 9, deletions: 0, staged: true },
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
