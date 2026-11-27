//! The pages of Settings, in the order the list shows them, with the words the search finds on
//! each and where the ids of the old panes land.

// Rust guideline compliant 2026-10-09

/// One page: its id, the group it is listed under, its name, its icons, the line under its
/// title, and the settings on it the search can find.
pub(super) struct Page {
    pub(super) id: &'static str,
    pub(super) group: &'static str,
    pub(super) label: &'static str,
    pub(super) icon: &'static str,
    /// The filled icon shown while the page is open; the plain one when there is no filled one.
    pub(super) icon_on: &'static str,
    /// The names of the rows and sections on the page. Each is a row's or heading's title as the
    /// page draws it, so a search result can scroll to it and light it.
    pub(super) words: &'static [&'static str],
}

/// Every page, grouped as the list shows them.
pub(super) const PAGES: &[Page] = &[
    Page {
        id: "general",
        group: "App",
        label: "General",
        icon: "gear-six",
        icon_on: "gear-six-fill",
        words: &[
            "New chats",
            "Default agent",
            "Open tasks in",
            "Transcript",
            "Show thoughts",
            "Click answers twice to send",
            "Enter steers a running reply",
            "Granular chats",
            "Agents can start and message chats",
            "Stop agent after 200 tool steps",
            "Token limit per turn",
            "Resume interrupted replies at launch",
            "Terminal",
            "Starting folder",
            "Inherit environment",
            "UTF-8 locale",
            "Environment variables",
            "Terminal text",
            "Font size",
            "Font weight",
            "Bold font weight",
            "Line height",
            "Letter spacing",
            "Cursor",
            "Cursor blinking",
            "Cursor width",
            "Minimum contrast ratio",
            "Bold text in bright colors",
            "Terminal behavior",
            "Scrollback",
            "Copy on selection",
            "Right click",
            "Alt-click moves the cursor",
            "Option is Meta",
            "Smooth scrolling",
            "Scroll speed",
            "Fast scroll speed",
        ],
    },
    Page {
        id: "appearance",
        group: "App",
        label: "Appearance",
        icon: "palette",
        icon_on: "palette-fill",
        words: &[
            "Mode",
            "Accent",
            "Light palette",
            "Dark palette",
            "Palette",
            "Switch with",
            "Generate with",
            "Syntax theme",
            "Orchestrator theme",
            "Chat theme",
            "Quilts theme",
            "Interface",
            "Text size",
            "Reduce motion",
            "Sync backgrounds",
            "App icon",
        ],
    },
    Page {
        id: "shortcuts",
        group: "App",
        label: "Shortcuts",
        icon: "keyboard",
        icon_on: "keyboard-fill",
        words: &["Keyboard shortcuts", "Reset all"],
    },
    Page {
        id: "notifications",
        group: "App",
        label: "Notifications",
        icon: "bell",
        icon_on: "bell",
        words: &[
            "Asks something",
            "Finished",
            "Goal met",
            "Failed",
            "Usage runs out",
            "Update available",
            "Send to chat apps",
            "Away after",
            "Muted",
        ],
    },
    Page {
        id: "providers",
        group: "Agents",
        label: "Providers",
        icon: "sparkle",
        icon_on: "sparkle-fill",
        words: &[
            "Agent CLIs",
            "API providers",
            "Vision model",
            "Add a provider",
            "Local providers",
        ],
    },
    Page {
        id: "permissions",
        group: "Agents",
        label: "Permissions",
        icon: "shield-check",
        icon_on: "shield-check-fill",
        words: &[
            "Rules",
            "Let agents use Android devices",
            "Let the agent use the screen",
            "Let the agent see the screen",
            "Restore defaults",
        ],
    },
    Page {
        id: "agents",
        group: "Agents",
        label: "Agents",
        icon: "robot",
        icon_on: "robot-fill",
        words: &["Custom agents", "New agent"],
    },
    Page {
        id: "skills",
        group: "Agents",
        label: "Skills",
        icon: "magic-wand",
        icon_on: "magic-wand-fill",
        words: &["Skills", "SKILL.md"],
    },
    Page {
        id: "hooks",
        group: "Agents",
        label: "Hooks",
        icon: "wrench",
        icon_on: "wrench-fill",
        words: &["Hooks"],
    },
    Page {
        id: "mcp",
        group: "Agents",
        label: "MCP",
        icon: "plugs-connected",
        icon_on: "plugs-connected-fill",
        words: &["MCP servers"],
    },
    Page {
        id: "projects",
        group: "Workspace",
        label: "Projects",
        icon: "folder-simple",
        icon_on: "folder-simple-fill",
        words: &[
            "Imports",
            "Run checks after every task",
            "Worktrees",
            "Give tasks their own worktree",
            "Fork into a new worktree",
            "Delete with deleted tasks",
            "Delete merged",
            "Delete inactive",
            "Delete archived",
            "Delete unchanged",
            "Keep when",
        ],
    },
    Page {
        id: "scheduled",
        group: "Workspace",
        label: "Scheduled",
        icon: "clock",
        icon_on: "clock-fill",
        words: &["Scheduled tasks", "Weekly report"],
    },
    Page {
        id: "ssh",
        group: "Workspace",
        label: "SSH hosts",
        icon: "hard-drives",
        icon_on: "hard-drives-fill",
        words: &["SSH hosts"],
    },
    Page {
        id: "chat-apps",
        group: "Workspace",
        label: "Chat apps",
        icon: "chats",
        icon_on: "chats-fill",
        words: &[
            "Access",
            "Telegram",
            "Discord",
            "Slack",
            "Matrix",
            "Mattermost",
            "Signal",
            "QQ",
            "Pairings",
        ],
    },
    Page {
        id: "usage",
        group: "Data",
        label: "Usage",
        icon: "chart-bar",
        icon_on: "chart-bar-fill",
        words: &[
            "Ask before resuming a large task",
            "Generate chat and task titles",
            "Only local models write titles",
            "Show token counts",
            "Warn above",
            "Write titles with",
            "Costs",
            "Spend",
            "Today",
            "Daily average",
            "All time",
            "Last 30 days",
            "This month",
            "By day",
            "By provider",
            "By model",
            "By project",
            "Most expensive chats",
            "By source",
            "Subagents",
            "Saved by cache",
            "Export CSV",
        ],
    },
    Page {
        id: "vault",
        group: "Data",
        label: "Vault",
        icon: "fingerprint",
        icon_on: "fingerprint-fill",
        words: &[
            "Protected by",
            "New password",
            "Saved secrets",
            "Old copies",
        ],
    },
    Page {
        id: "logs",
        group: "Data",
        label: "Logs",
        icon: "file-text",
        icon_on: "file-text-fill",
        words: &["Log levels", "Open logs in chat menus", "Log folder"],
    },
    Page {
        id: "about",
        group: "Data",
        label: "About",
        icon: "arrow-circle-up",
        icon_on: "arrow-circle-up-fill",
        words: &[
            "Version",
            "Platform",
            "Data folder",
            "Updates",
            "Check for updates every",
            "Release notes",
        ],
    },
];

/// Where the ids of the panes before the redesign land: the page, and the section on it to
/// scroll to when the pane became one section of a larger page. Callers outside Settings, and
/// the agent's `open_settings` tool, still pass these.
const OLD_IDS: &[(&str, &str, Option<&str>)] = &[
    ("chat", "general", None),
    ("terminal", "general", Some("Terminal")),
    ("themes", "appearance", None),
    ("code-theme", "appearance", Some("Syntax theme")),
    (
        "orchestrator-background",
        "appearance",
        Some("Orchestrator theme"),
    ),
    ("chat-background", "appearance", Some("Chat theme")),
    ("quilts-background", "appearance", Some("Quilts theme")),
    ("interface", "appearance", Some("Interface")),
    ("tokens", "usage", None),
    ("costs", "usage", Some("Costs")),
    ("extensions", "agents", None),
    ("worktrees", "projects", Some("Worktrees")),
    ("updates", "about", Some("Updates")),
    ("security", "vault", None),
    ("gateway", "chat-apps", None),
];

/// The page with the id `id`.
pub(super) fn page(id: &str) -> Option<&'static Page> {
    PAGES.iter().find(|p| p.id == id)
}

/// The page `id` names, by its own id or an old pane's, with the section to scroll to; None
/// when it names neither.
pub(super) fn resolve(id: &str) -> Option<(&'static Page, Option<&'static str>)> {
    if let Some(page) = page(id) {
        return Some((page, None));
    }
    let &(_, to, section) = OLD_IDS.iter().find(|(old, _, _)| *old == id)?;
    Some((page(to)?, section))
}

#[cfg(test)]
mod tests {
    use super::{OLD_IDS, PAGES, page, resolve};

    #[test]
    fn every_old_id_lands_on_a_page() {
        for (old, to, _) in OLD_IDS {
            assert!(page(to).is_some(), "{old} lands on {to}, which is no page");
        }
        for old in [
            "chat",
            "logs",
            "notifications",
            "updates",
            "themes",
            "code-theme",
            "orchestrator-background",
            "chat-background",
            "interface",
            "shortcuts",
            "providers",
            "tokens",
            "extensions",
            "permissions",
            "security",
            "projects",
            "scheduled",
            "terminal",
            "ssh",
            "gateway",
            "costs",
            "mcp",
            "worktrees",
            "quilts-background",
        ] {
            assert!(resolve(old).is_some(), "{old} lands nowhere");
        }
    }

    #[test]
    fn page_ids_are_unique() {
        for (at, p) in PAGES.iter().enumerate() {
            assert!(
                PAGES[at + 1..].iter().all(|q| q.id != p.id),
                "{} twice",
                p.id
            );
        }
    }
}
