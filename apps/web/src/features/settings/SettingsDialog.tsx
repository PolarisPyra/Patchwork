import { memo, useState } from "react";
import {
  ArrowCircleUpIcon,
  BellIcon,
  BooksIcon,
  CheckCircleIcon,
  ChatCircleIcon,
  ClockIcon,
  DownloadSimpleIcon,
  FolderSimpleIcon,
  GaugeIcon,
  KeyboardIcon,
  PaletteIcon,
  PlugIcon,
  PlugsConnectedIcon,
  PuzzlePieceIcon,
  RobotIcon,
  ScrollIcon,
  ShieldCheckIcon,
  TerminalIcon,
  TerminalWindowIcon,
  XIcon,
  type Icon,
} from "@phosphor-icons/react";
import AgentsPane from "~/features/settings/agents/AgentsPane";
import AppearancePane from "~/features/settings/app/AppearancePane";
import ChatPane from "~/features/settings/app/ChatPane";
import ChecksPane from "~/features/settings/system/ChecksPane";
import ClaudePane from "~/features/settings/providers/ClaudePane";
import CodexPane from "~/features/settings/providers/CodexPane";
import EnvironmentPane from "~/features/settings/system/EnvironmentPane";
import ImportsPane from "~/features/settings/system/ImportsPane";
import KeybindingsPane from "~/features/settings/app/KeybindingsPane";
import LoggingPane from "~/features/settings/system/LoggingPane";
import McpServersPane from "~/features/settings/providers/McpServersPane";
import NotificationsPane from "~/features/settings/app/NotificationsPane";
import PermissionsPane, { type PermissionsFocus } from "~/features/settings/system/PermissionsPane";
import PluginsPane from "~/features/settings/agents/PluginsPane";
import ProjectsPane from "~/features/settings/system/ProjectsPane";
import ProvidersPane from "~/features/settings/providers/ProvidersPane";
import ScheduledPane from "~/features/settings/agents/ScheduledPane";
import SkillsPane from "~/features/settings/agents/SkillsPane";
import TerminalPane from "~/features/settings/app/TerminalPane";
import TokensPane from "~/features/settings/app/TokensPane";
import UpdatesPane from "~/features/settings/system/UpdatesPane";
import { logoUrl } from "~/features/settings/logo";
import type { PaneProps } from "~/features/settings/types";

/**
 * Settings: the dialog around the panes — its list, its title, and which pane is open. Each pane
 * draws itself in its own file, and the controls they share live in `settings/controls`.
 */

type Section =
  | "appearance"
  | "chat"
  | "providers"
  | "permissions"
  | "mcp"
  | "tokens"
  | "notifications"
  | "imports"
  | "environment"
  | "terminal"
  | "skills"
  | "hooks"
  | "scheduled"
  | "claude"
  | "codex"
  | "custom-agents"
  | "projects"
  | "checks"
  | "shortcuts"
  | "logging"
  | "updates";

/** The panes, in groups under a heading, so the list reads in a few short runs. */
const GROUPS: { label: string; sections: { id: Section; label: string; Icon: Icon }[] }[] = [
  {
    label: "General",
    sections: [
      { id: "appearance", label: "Appearance", Icon: PaletteIcon },
      { id: "chat", label: "Chats", Icon: ChatCircleIcon },
      { id: "notifications", label: "Notifications", Icon: BellIcon },
      { id: "shortcuts", label: "Shortcuts", Icon: KeyboardIcon },
      { id: "updates", label: "Updates", Icon: ArrowCircleUpIcon },
    ],
  },
  {
    label: "Agents",
    sections: [
      { id: "providers", label: "Providers", Icon: PlugsConnectedIcon },
      { id: "mcp", label: "MCP Servers", Icon: PlugIcon },
      { id: "custom-agents", label: "Agents", Icon: RobotIcon },
      { id: "skills", label: "Skills", Icon: BooksIcon },
      { id: "hooks", label: "Hooks", Icon: PuzzlePieceIcon },
      { id: "claude", label: "Claude CLI", Icon: TerminalWindowIcon },
      { id: "codex", label: "Codex CLI", Icon: TerminalWindowIcon },
      { id: "permissions", label: "Permissions", Icon: ShieldCheckIcon },
      { id: "tokens", label: "AI Tokenomics", Icon: GaugeIcon },
    ],
  },
  {
    label: "Projects",
    sections: [
      { id: "projects", label: "Projects", Icon: FolderSimpleIcon },
      { id: "checks", label: "Checks", Icon: CheckCircleIcon },
      { id: "scheduled", label: "Scheduled", Icon: ClockIcon },
      { id: "imports", label: "Imports", Icon: DownloadSimpleIcon },
    ],
  },
  {
    label: "System",
    sections: [
      { id: "environment", label: "Environment", Icon: TerminalIcon },
      { id: "terminal", label: "Terminal", Icon: TerminalWindowIcon },
      { id: "logging", label: "Logging", Icon: ScrollIcon },
    ],
  },
];

const SECTIONS = GROUPS.flatMap((g) => g.sections);

interface SettingsDialogProps extends PaneProps {
  /** Opens on Settings → Permissions, with this scope and rule picked out. */
  focus?: PermissionsFocus | null;
  /** Called after conversations were imported, so threads can be reloaded. */
  onImported: () => void;
  onClose: () => void;
}

// Memoized: the workspace re-renders on every streamed delta, and the open pane should not.
export default memo(function SettingsDialog({
  settings,
  updateSettings,
  focus,
  onImported,
  onClose,
}: SettingsDialogProps) {
  const [section, setSection] = useState<Section>(focus ? "permissions" : "appearance");
  const title = SECTIONS.find((s) => s.id === section)?.label ?? "";
  const paneProps = { settings, updateSettings };

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-[rgba(0,0,0,0.4)]"
      onMouseDown={onClose}
    >
      <div
        className="flex h-[680px] max-h-[calc(100vh_-_48px)] w-[920px] max-w-[calc(100vw_-_48px)] overflow-hidden rounded-[var(--radius-lg)] border border-border-strong bg-elevated shadow-[var(--shadow-popup)]"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <nav
          aria-label="Settings sections"
          className="flex w-[192px] flex-none flex-col overflow-y-scroll border-r border-border bg-panel px-2 py-2.5"
        >
          {GROUPS.map((group) => (
            <div key={group.label} className="flex flex-col gap-0.5 [&+&]:mt-3">
              <div className="px-2 pb-0.5 text-[length:var(--fs-xs)] font-semibold text-fg-subtle">
                {group.label}
              </div>
              {group.sections.map(({ id, label, Icon }) => (
                <button
                  key={id}
                  className={`flex h-8 cursor-pointer items-center gap-2.5 rounded-[var(--radius-sm)] border-none px-2 text-left text-fg ${section === id ? "bg-selected hover:bg-[color-mix(in_srgb,var(--bg-selected),var(--text)_8%)]" : "bg-transparent hover:bg-hover"}`}
                  aria-current={section === id ? "page" : undefined}
                  onClick={() => setSection(id)}
                >
                  <Icon size={18} className="flex-none" />
                  {label}
                </button>
              ))}
            </div>
          ))}
        </nav>
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-12 flex-none items-center justify-between pr-4 pl-5">
            <h2 className="m-0 text-[length:var(--fs-lg)] font-semibold">{title}</h2>
            <button className="btn-icon" title="Close" aria-label="Close" onClick={onClose}>
              <XIcon size={16} />
            </button>
          </div>
          <div
            role="region"
            aria-label={title}
            className="min-h-0 flex-1 overflow-y-auto px-5 pb-5"
          >
            {section === "appearance" && <AppearancePane {...paneProps} />}
            {section === "chat" && <ChatPane {...paneProps} />}
            {section === "providers" && <ProvidersPane {...paneProps} />}
            {section === "permissions" && <PermissionsPane {...paneProps} focus={focus ?? null} />}
            {section === "mcp" && <McpServersPane appIcon={logoUrl(settings.logo)} />}
            {section === "tokens" && <TokensPane {...paneProps} />}
            {section === "notifications" && <NotificationsPane {...paneProps} />}
            {section === "imports" && <ImportsPane {...paneProps} onImported={onImported} />}
            {section === "environment" && <EnvironmentPane {...paneProps} />}
            {section === "terminal" && <TerminalPane {...paneProps} />}
            {section === "skills" && <SkillsPane {...paneProps} />}
            {section === "hooks" && <PluginsPane {...paneProps} />}
            {section === "claude" && <ClaudePane {...paneProps} />}
            {section === "codex" && <CodexPane {...paneProps} />}
            {section === "scheduled" && <ScheduledPane {...paneProps} />}
            {section === "custom-agents" && <AgentsPane />}
            {section === "projects" && <ProjectsPane {...paneProps} />}
            {section === "checks" && <ChecksPane {...paneProps} />}
            {section === "shortcuts" && <KeybindingsPane />}
            {section === "logging" && <LoggingPane {...paneProps} />}
            {section === "updates" && <UpdatesPane {...paneProps} />}
          </div>
        </div>
      </div>
    </div>
  );
});
