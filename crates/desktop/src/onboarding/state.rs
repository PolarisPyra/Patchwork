//! First-run setup's state: its steps, and what each step found or chose.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use gpui::{EventEmitter, FocusHandle, ScrollHandle, Window, prelude::*};
use patchwork_server::Server;
use patchwork_server::import::{ImportScan, ImportSource};
use patchwork_server::provider::{AgentKind, AgentStatus};
use patchwork_server::settings::Settings;
use patchwork_server::system::browse;
use crate::ui::text_area::{Changed, Submitted, TextArea};
use crate::ui::theme::theme;

/// The agent CLIs setup offers, in the order it lists them (`CLI_KINDS`).
pub(super) const AGENT_KINDS: [AgentKind; 4] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
];

pub(super) struct Step {
    pub(super) label: &'static str,
    pub(super) title: &'static str,
    pub(super) subtitle: &'static str,
}

pub(super) const STEPS: [Step; 4] = [
    Step {
        label: "Agent",
        title: "Connect your coding agents",
        subtitle: "Connect the agents on this computer, or download the ones you want. You can pick which one runs each task.",
    },
    Step {
        label: "Imports",
        title: "Import your conversations",
        subtitle: "Bring in conversations you've had with these agents outside Patchwork. Follow-ups continue the same session.",
    },
    Step {
        label: "Projects",
        title: "Set up your projects",
        subtitle: "Choose where Patchwork should work. You can add and manage projects later.",
    },
    Step {
        label: "Ready",
        title: "You're all set",
        subtitle: "Check your choices, then start your first task.",
    },
];

pub(super) const IMPORTS_STEP: usize = 1;

pub(super) const PROJECTS_STEP: usize = 2;

pub(super) const LAST_STEP: usize = STEPS.len() - 1;

/// The logo's colours, which the steps' circles and lines are filled with.
pub(super) const LOGO_BLUE: u32 = 0x005b_c0eb;

pub(super) const LOGO_VIOLET: u32 = 0x009b_5de5;

pub(super) const LOGO_PINK: u32 = 0x00ff_7eb3;

/// What setup tells the window: the settings changed, or the theme button was pressed.
pub enum OnboardingEvent {
    Changed,
    ToggleTheme,
}

/// One agent's detection.
#[derive(Clone, Default)]
pub(super) struct Probe {
    pub(super) checking: bool,
    pub(super) status: Option<AgentStatus>,
}

impl Probe {
    /// An agent is usable only when it is installed and signed in.
    pub(super) fn ready(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(|s| s.installed && s.signed_in)
    }
}

/// The provider being imported now, and where it falls in the run.
#[derive(Clone, Copy)]
pub(super) struct ImportProgress {
    pub(super) kind: ImportSource,
    pub(super) index: usize,
    pub(super) total: usize,
}

/// What the Imports step brought in: threads added, and projects made for their folders.
#[derive(Clone, Copy)]
pub(super) struct ImportResult {
    pub(super) threads: u32,
    pub(super) projects: usize,
}

pub struct Onboarding {
    pub(super) server: Arc<Server>,
    pub(super) settings: Settings,
    pub(super) step: usize,
    /// The furthest step reached; later ones can't be picked in the stepper.
    pub(super) reached: usize,
    pub(super) saving: bool,
    pub(super) probes: HashMap<AgentKind, Probe>,
    /// Agents whose CLI is downloading, and why the last download of each failed.
    pub(super) installing: HashSet<AgentKind>,
    pub(super) install_errors: HashMap<AgentKind, String>,
    /// Connected agents, in `AGENT_KINDS` order, once every probe has answered.
    pub(super) agents: Option<Vec<AgentKind>>,
    /// The agent new tasks start with; saved first in `settings.agents`, which is what marks it.
    pub(super) default_kind: Option<AgentKind>,
    /// Each source's scan once it answers; `None` marks a failed one.
    pub(super) scans: Vec<(ImportSource, Option<ImportScan>)>,
    /// Counts the scans started, so an older run's answers are dropped.
    pub(super) scan_run: usize,
    /// Providers to import from; defaults to every one with something new.
    pub(super) import_kinds: Option<Vec<ImportSource>>,
    pub(super) import_error: Option<String>,
    pub(super) importing: Option<ImportProgress>,
    /// Threads imported so far, and the project count before the first import, so a retry after
    /// a failure still counts everything.
    pub(super) tally: Option<(u32, usize)>,
    pub(super) imported: Option<ImportResult>,
    pub(super) creating: bool,
    pub(super) name: gpui::Entity<TextArea>,
    pub(super) busy: bool,
    pub(super) project_error: Option<String>,
    pub(super) focus: FocusHandle,
    pub(super) heading: FocusHandle,
    /// The default agent's tiles, so the arrow keys can move between them.
    pub(super) default_tiles: HashMap<AgentKind, FocusHandle>,
    /// The SSH hosts in `~/.ssh/config`, once read.
    pub(super) hosts: Option<Vec<String>>,
    /// The "Where tasks run" options: this computer, then each host, or the row saying there is
    /// none.
    pub(super) runs_on: Vec<FocusHandle>,
    pub(super) scroll: ScrollHandle,
}

impl EventEmitter<OnboardingEvent> for Onboarding {}

impl Onboarding {
    pub fn new(server: Arc<Server>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let t = *theme(cx);
        let name = cx.new(|cx| {
            let mut field = TextArea::new("my-project", "Project name", cx);
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = crate::ui::theme::input_placeholder();
            field.selection_color = t.accent.opacity(0.3);
            field
        });
        cx.subscribe(&name, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        cx.subscribe(&name, |this, _, _: &Submitted, cx| this.create_in(cx))
            .detach();
        let settings = server.store.settings();
        let heading = cx.focus_handle();
        window.focus(&heading, cx);
        let mut this = Self {
            server,
            default_kind: settings.agents.first().copied(),
            settings,
            step: 0,
            reached: 0,
            saving: false,
            probes: HashMap::new(),
            installing: HashSet::new(),
            install_errors: HashMap::new(),
            agents: None,
            scans: Vec::new(),
            scan_run: 0,
            import_kinds: None,
            import_error: None,
            importing: None,
            tally: None,
            imported: None,
            creating: false,
            name,
            busy: false,
            project_error: None,
            focus: cx.focus_handle(),
            heading,
            default_tiles: AGENT_KINDS
                .into_iter()
                .map(|k| (k, cx.focus_handle()))
                .collect(),
            hosts: None,
            runs_on: vec![cx.focus_handle(), cx.focus_handle()],
            scroll: ScrollHandle::new(),
        };
        this.check(cx);
        let task = cx.background_spawn(async move { browse::machines() });
        cx.spawn(async move |this, cx| {
            let machines = task.await;
            this.update(cx, |this, cx| {
                let hosts = machines
                    .into_iter()
                    .filter(|m| !m.local)
                    .map(|m| m.name)
                    .collect::<Vec<_>>();
                this.runs_on = (0..=hosts.len().max(1))
                    .map(|_| cx.focus_handle())
                    .collect();
                this.hosts = Some(hosts);
                cx.notify();
            })
        })
        .detach();
        this
    }

    /// Saves a change to the settings, and tells the window.
    pub(super) fn save(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        if let Err(e) = self.server.store.update_settings(change) {
            log::warn!(error:% = e; "could not save the setup's settings: {{error}}");
        }
        self.settings = self.server.store.settings();
        cx.emit(OnboardingEvent::Changed);
        cx.notify();
    }

    pub(super) fn can_continue(&self) -> bool {
        !self.saving
            && (self.step != 0 || self.agents.as_ref().is_some_and(|a| !a.is_empty()))
            && (self.step != IMPORTS_STEP || !self.scanning())
    }

    pub(super) fn go_to(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.step = index;
        self.reached = self.reached.max(index);
        if index == IMPORTS_STEP {
            self.scan(cx);
        }
        // The heading takes focus, so screen readers and agents hear that the page changed and
        // start reading from the top of it.
        window.focus(&self.heading, cx);
        self.scroll.set_offset(gpui::Point::default());
        cx.notify();
    }

    pub(super) fn complete(&mut self, cx: &mut Context<Self>) {
        self.save(|s| s.onboarded = true, cx);
    }

    pub(super) fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_continue() {
            return;
        }
        if self.step == LAST_STEP {
            return self.complete(cx);
        }
        if self.step == 0 && self.agents.is_some() {
            let agents = self.ordered_agents();
            self.save(|s| s.agents = agents, cx);
        }
        if self.step == IMPORTS_STEP {
            return self.import_selected(window, cx);
        }
        self.go_to(self.step + 1, window, cx);
    }

    /// Enter continues from anywhere that is not a field or a control, so it never takes the
    /// project name's Enter or a focused button's.
    pub(super) fn on_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let on_page = self.heading.is_focused(window)
            || self.focus.is_focused(window)
            || !self.focus.contains_focused(window, cx);
        if on_page && self.can_continue() {
            self.next(window, cx);
        }
    }
}
