import type { AgentKind, Settings, TerminalSettings } from "~/api/types";

const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
const isLinux = /Linux/.test(navigator.platform);

/** VS Code's terminal defaults, as `TerminalSettings::default` sets them. */
export function defaultTerminalSettings(): TerminalSettings {
  return {
    shellArgs: null,
    env: {},
    inheritEnv: true,
    cwd: null,
    detectLocale: "auto",
    fontFamily: null,
    fontSize: isMac ? 12 : 14,
    lineHeight: isLinux ? 1.1 : 1,
    letterSpacing: 0,
    fontWeight: "normal",
    fontWeightBold: "bold",
    cursorStyle: "block",
    cursorBlinking: false,
    cursorWidth: 1,
    scrollback: 1000,
    copyOnSelection: false,
    rightClickBehavior: isMac ? "selectWord" : isLinux ? "default" : "copyPaste",
    altClickMovesCursor: true,
    macOptionIsMeta: false,
    drawBoldTextInBrightColors: true,
    minimumContrastRatio: 4.5,
    smoothScrolling: false,
    mouseWheelScrollSensitivity: 1,
    fastScrollSensitivity: 5,
  };
}

export const DEFAULT_SETTINGS: Settings = {
  onboarded: false,
  theme: "dark",
  agents: [],
  integrations: { github: false, terminal: false, figma: false, browser: false },
  projects: [],
  activeProjectId: null,
  agent: null,
  importLogDisabled: false,
  agentPaths: {},
  systemPrompts: {},
  shell: null,
  skillsDir: null,
  disabledSkills: [],
  visionModel: null,
  tokens: {
    showUsage: true,
    warnAbove: 150_000,
    askBeforeResume: true,
    claudeCompactWindow: 200_000,
    compactWindowDefaulted: true,
    apiCompactWindow: 128_000,
    codexCompactWindow: 0,
    opencodeCompactWindow: 0,
    antigravityCompactWindow: 0,
    defaultModels: {},
    generateTitles: true,
  },
  notifications: { goalComplete: true, taskComplete: true },
  updates: { checkEveryMinutes: 10 },
  logo: null,
  logging: { app: "info", window: "info", agents: "info", imports: "info" },
  terminal: defaultTerminalSettings(),
  permissions: {
    everywhere: { defaults: {}, rules: [] },
    chats: { defaults: {}, rules: [] },
    projects: {},
    seeded: false,
  },
  confirmAnswers: false,
  pluginsDir: null,
  disabledPlugins: [],
  computerUse: false,
};

/** Agent used for new tasks: the first connected one, else Claude Code. */
export function defaultAgentOf(settings: Settings): AgentKind {
  return settings.agents[0] ?? "claude";
}
