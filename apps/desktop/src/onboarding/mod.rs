//! First-run setup (`features/onboarding/Onboarding.tsx`): connect the agents, import their
//! conversations, pick a project, then start. The window shows it instead of the workspace until
//! `settings.onboarded` is set, which "Skip setup" and the last step do, and "Run setup again" in
//! Settings → Projects clears.

mod agent_step;
mod frame;
mod imports_step;
mod projects_step;
mod ready_step;

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    AnyElement, Context, Div, EventEmitter, FocusHandle, FontWeight, Role, ScrollHandle,
    SharedString, Stateful, Toggled, Window, div, prelude::*, px,
};
use patchwork_server::Server;
use patchwork_server::import::{ImportScan, ImportSource};
use patchwork_server::provider::{AgentKind, AgentStatus};
use patchwork_server::settings::Settings;

use crate::chat::text_area::{Changed, Submitted, TextArea};
use crate::theme::{FS_LG, FS_MD, FS_SM, RADIUS, RADIUS_SM, Theme, theme};
use crate::ui::{icon, mix};

/// The agent CLIs setup offers, in the order it lists them (`CLI_KINDS`).
const AGENT_KINDS: [AgentKind; 4] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
];

struct Step {
    label: &'static str,
    title: &'static str,
    subtitle: &'static str,
    helper: &'static str,
}

const STEPS: [Step; 4] = [
    Step {
        label: "Agent",
        title: "Connect your coding agents",
        subtitle: "Connect the agents already installed on this computer. You can pick which one runs each task.",
        helper: "Connect at least one agent to start your first task.",
    },
    Step {
        label: "Imports",
        title: "Import your conversations",
        subtitle: "Bring in conversations you've had with these agents outside Patchwork. Follow-ups continue the same session.",
        helper: "Importing is optional. You can import later from Settings.",
    },
    Step {
        label: "Projects",
        title: "Set up your projects",
        subtitle: "Choose where Patchwork should work. You can add and manage projects later.",
        helper: "Projects are optional. You can add more later.",
    },
    Step {
        label: "Ready",
        title: "You're all set",
        subtitle: "Check your choices, then start your first task.",
        helper: "You can change any of this later in Settings.",
    },
];

const IMPORTS_STEP: usize = 1;
const PROJECTS_STEP: usize = 2;
const LAST_STEP: usize = STEPS.len() - 1;

/// The logo's colours, which the steps' circles and lines are filled with.
const LOGO_BLUE: u32 = 0x5bc0eb;
const LOGO_VIOLET: u32 = 0x9b5de5;
const LOGO_PINK: u32 = 0xff7eb3;

/// What setup tells the window: the settings changed, or the theme button was pressed.
pub enum OnboardingEvent {
    Changed,
    ToggleTheme,
}

/// One agent's detection.
#[derive(Clone, Default)]
struct Probe {
    checking: bool,
    status: Option<AgentStatus>,
}

impl Probe {
    /// An agent is usable only when it is installed and signed in.
    fn ready(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(|s| s.installed && s.signed_in)
    }
}

/// The provider being imported now, and where it falls in the run.
#[derive(Clone, Copy)]
struct ImportProgress {
    kind: ImportSource,
    index: usize,
    total: usize,
}

/// What the Imports step brought in: threads added, and projects made for their folders.
#[derive(Clone, Copy)]
struct ImportResult {
    threads: u32,
    projects: usize,
}

pub struct Onboarding {
    server: Arc<Server>,
    settings: Settings,
    step: usize,
    /// The furthest step reached; later ones can't be picked in the stepper.
    reached: usize,
    saving: bool,
    probes: HashMap<AgentKind, Probe>,
    /// Connected agents, in `AGENT_KINDS` order, once every probe has answered.
    agents: Option<Vec<AgentKind>>,
    /// The agent new tasks start with; saved first in `settings.agents`, which is what marks it.
    default_kind: Option<AgentKind>,
    /// Each source's scan once it answers; `None` marks a failed one.
    scans: Vec<(ImportSource, Option<ImportScan>)>,
    /// Counts the scans started, so an older run's answers are dropped.
    scan_run: usize,
    /// Providers to import from; defaults to every one with something new.
    import_kinds: Option<Vec<ImportSource>>,
    import_error: Option<String>,
    importing: Option<ImportProgress>,
    /// Threads imported so far, and the project count before the first import, so a retry after
    /// a failure still counts everything.
    tally: Option<(u32, usize)>,
    imported: Option<ImportResult>,
    creating: bool,
    name: gpui::Entity<TextArea>,
    busy: bool,
    project_error: Option<String>,
    focus: FocusHandle,
    heading: FocusHandle,
    /// The default agent's tiles, so the arrow keys can move between them.
    default_tiles: HashMap<AgentKind, FocusHandle>,
    runs_on: [FocusHandle; 2],
    scroll: ScrollHandle,
}

impl EventEmitter<OnboardingEvent> for Onboarding {}

impl Onboarding {
    pub fn new(server: Arc<Server>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let t = *theme(cx);
        let name = cx.new(|cx| {
            let mut field = TextArea::new("my-project", "Project name", cx);
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = crate::theme::input_placeholder();
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
            runs_on: [cx.focus_handle(), cx.focus_handle()],
            scroll: ScrollHandle::new(),
        };
        this.check(cx);
        this
    }

    /// Saves a change to the settings, and tells the window.
    fn save(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        if let Err(e) = self.server.store.update_settings(change) {
            log::warn!("could not save the setup's settings: {e}");
        }
        self.settings = self.server.store.settings();
        cx.emit(OnboardingEvent::Changed);
        cx.notify();
    }

    fn can_continue(&self) -> bool {
        !self.saving
            && (self.step != 0 || self.agents.as_ref().is_some_and(|a| !a.is_empty()))
            && (self.step != IMPORTS_STEP || !self.scanning())
    }

    fn go_to(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.step = index;
        self.reached = self.reached.max(index);
        if index == IMPORTS_STEP {
            self.scan(cx);
        }
        // The heading takes focus, so screen readers and agents hear that the page changed and
        // start reading from the top of it.
        window.focus(&self.heading, cx);
        self.scroll.set_offset(Default::default());
        cx.notify();
    }

    fn complete(&mut self, cx: &mut Context<Self>) {
        self.save(|s| s.onboarded = true, cx);
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn on_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let on_page = self.heading.is_focused(window)
            || self.focus.is_focused(window)
            || !self.focus.contains_focused(window, cx);
        if on_page && self.can_continue() {
            self.next(window, cx);
        }
    }
}

/// A step's small heading.
fn heading(text: &'static str) -> Div {
    div()
        .text_size(px(FS_MD))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// A picked setup choice: a plain, slightly lighter fill and a stronger border, no colour.
fn picked(row: Stateful<Div>, t: &Theme) -> Stateful<Div> {
    row.border_color(t.border_strong)
        .bg(mix(t.text, t.bg_editor, 6.))
}

/// A row to tick (`AgentRow`, and a provider to import from): a logo tile, and a name with a line
/// under it. Its owner adds what sits at the end with `row_end`. Unless `enabled`, it can't be
/// ticked.
fn choice_row(
    id: SharedString,
    name: &str,
    detail: String,
    logo: AnyElement,
    checked: bool,
    enabled: bool,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    let row = div()
        .id(id)
        .role(Role::CheckBox)
        .aria_label(SharedString::from(name.to_string()))
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .flex()
        .w_full()
        .items_center()
        .gap(px(16.))
        .rounded(px(RADIUS))
        .border_1()
        .p(px(16.))
        .text_color(t.text)
        .child(
            div()
                .size(px(44.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS))
                .bg(t.bg_selected_muted)
                .child(logo),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(FS_LG))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name.to_string()),
                )
                .child(
                    div()
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(detail),
                ),
        );
    let row = if checked {
        picked(row, t)
    } else {
        row.border_color(t.border)
    };
    if enabled {
        let row = row.tab_index(0).cursor_pointer();
        if checked {
            row
        } else {
            row.hover(move |s| s.bg(hover))
        }
    } else {
        row
    }
}

/// What sits at a choice row's end: its badges and tick.
fn row_end(end: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .children(end)
}

/// The square tick at a choice's end.
fn tick(checked: bool, t: &Theme) -> AnyElement {
    div()
        .size(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .border_1()
        .text_color(t.text_on_accent)
        .map(|b| {
            if checked {
                b.border_color(t.accent)
                    .bg(t.accent)
                    .child(icon("check-bold", 12.))
            } else {
                b.border_color(t.border_strong)
            }
        })
        .into_any_element()
}

/// `SharedString` for an element id made of a prefix and a key.
fn key_id(prefix: &str, key: impl std::fmt::Debug) -> SharedString {
    SharedString::from(format!("{prefix}-{key:?}"))
}
