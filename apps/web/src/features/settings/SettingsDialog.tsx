import { memo, useState, type ReactNode } from "react";
import {
  ArrowCircleUpIcon,
  BooksIcon,
  ClockIcon,
  DownloadSimpleIcon,
  FolderSimpleIcon,
  GaugeIcon,
  GearSixIcon,
  KeyboardIcon,
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
import CliPane from "~/features/settings/providers/CliPane";
import GeneralPane from "~/features/settings/app/GeneralPane";
import ImportsPane from "~/features/settings/system/ImportsPane";
import KeybindingsPane from "~/features/settings/app/KeybindingsPane";
import LoggingPane from "~/features/settings/system/LoggingPane";
import McpServersPane from "~/features/settings/providers/mcp/McpServersPane";
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

interface SettingsDialogProps extends PaneProps {
  /** Opens on Settings → Permissions, with this scope and rule picked out. */
  focus?: PermissionsFocus | null;
  /** The section it opens on, by id; null opens on General. */
  section?: string | null;
  /** Called after conversations were imported, so threads can be reloaded. */
  onImported: () => void;
  /** Deletes a background picture, and moves the chats and Settings that showed it back to their
   * defaults. */
  onRemoveBackgroundImage: (file: string) => Promise<() => Promise<void>>;
  onClose: () => void;
}

type Pane = (props: Omit<SettingsDialogProps, "onClose">) => ReactNode;

/** The panes, in groups under a heading, so the list reads in a few short runs. */
const GROUPS: {
  label: string;
  sections: { id: string; label: string; Icon: Icon; Pane: Pane }[];
}[] = [
  {
    label: "General",
    sections: [
      { id: "general", label: "General", Icon: GearSixIcon, Pane: GeneralPane },
      { id: "shortcuts", label: "Shortcuts", Icon: KeyboardIcon, Pane: () => <KeybindingsPane /> },
      { id: "updates", label: "Updates", Icon: ArrowCircleUpIcon, Pane: UpdatesPane },
    ],
  },
  {
    label: "Agents",
    sections: [
      { id: "providers", label: "Providers", Icon: PlugsConnectedIcon, Pane: ProvidersPane },
      {
        id: "mcp",
        label: "MCP Servers",
        Icon: PlugIcon,
        Pane: ({ settings }) => <McpServersPane appIcon={logoUrl(settings.logo)} />,
      },
      { id: "custom-agents", label: "Agents", Icon: RobotIcon, Pane: () => <AgentsPane /> },
      { id: "skills", label: "Skills", Icon: BooksIcon, Pane: SkillsPane },
      { id: "hooks", label: "Hooks", Icon: PuzzlePieceIcon, Pane: PluginsPane },
      { id: "clis", label: "CLIs", Icon: TerminalIcon, Pane: () => <CliPane /> },
      {
        id: "permissions",
        label: "Permissions",
        Icon: ShieldCheckIcon,
        Pane: ({ focus, ...props }) => <PermissionsPane {...props} focus={focus ?? null} />,
      },
      { id: "tokens", label: "AI Tokenomics", Icon: GaugeIcon, Pane: TokensPane },
    ],
  },
  {
    label: "Projects",
    sections: [
      { id: "projects", label: "Projects", Icon: FolderSimpleIcon, Pane: ProjectsPane },
      { id: "scheduled", label: "Scheduled", Icon: ClockIcon, Pane: ScheduledPane },
      { id: "imports", label: "Imports", Icon: DownloadSimpleIcon, Pane: ImportsPane },
    ],
  },
  {
    label: "System",
    sections: [
      { id: "terminal", label: "Terminal", Icon: TerminalWindowIcon, Pane: TerminalPane },
      { id: "logging", label: "Logging", Icon: ScrollIcon, Pane: LoggingPane },
    ],
  },
];

const SECTIONS = GROUPS.flatMap((g) => g.sections);

// Memoized: the workspace re-renders on every streamed delta, and the open pane should not.
export default memo(function SettingsDialog({ onClose, ...props }: SettingsDialogProps) {
  const [section, setSection] = useState(
    props.focus ? "permissions" : (props.section ?? "general"),
  );
  const open = SECTIONS.find((s) => s.id === section) ?? SECTIONS[0];

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
                  className={`flex h-8 cursor-pointer items-center gap-2.5 rounded-[var(--radius-sm)] border-none px-2 text-left text-fg ${open.id === id ? "bg-selected hover:bg-[color-mix(in_srgb,var(--bg-selected),var(--text)_8%)]" : "bg-transparent hover:bg-hover"}`}
                  aria-current={open.id === id ? "page" : undefined}
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
            <h2 className="m-0 text-[length:var(--fs-lg)] font-semibold">{open.label}</h2>
            <button className="btn-icon" title="Close" aria-label="Close" onClick={onClose}>
              <XIcon size={16} />
            </button>
          </div>
          <div
            role="region"
            aria-label={open.label}
            className="min-h-0 flex-1 overflow-y-auto px-5 pb-5 [scrollbar-gutter:stable]"
          >
            <open.Pane {...props} />
          </div>
        </div>
      </div>
    </div>
  );
});
