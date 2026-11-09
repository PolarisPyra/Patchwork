//! The menus the word before the caret opens (`useMentionMenu.ts`, `useSlashMenu.ts`,
//! `MentionMenu.tsx`, `SlashMenu.tsx`): project files and folders for "@", enabled skills for
//! "$", custom agents for "&" and commands for "/", listed above the composer. Up and Down move
//! the highlight, Tab or Enter picks it, and Escape hides the menu until the draft changes.

use std::collections::HashSet;
use std::ops::Range;

use gpui::{
    AnyElement, Context, FontWeight, PromptLevel, Role, ScrollHandle, SharedString, Window,
    deferred, div, prelude::*, px, relative,
};
use patchwork_server::provider::{AgentKind, ProviderSlashCommand};

use super::{Composer};
use super::menus::{Menu, Roles};
use crate::ui::sigils::{SigilKind, Sigils, default_sigil};
use crate::ui::theme::{FONT_MONO, FS_SM, RADIUS, Theme};
use crate::ui::{button, icon, popup_shadow};

/// Most files the menu offers at once.
const FILE_LIMIT: usize = 8;

/// When a command runs: as soon as it is picked, when the message is sent, or by the agent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Runs {
    Pick,
    Send,
    Agent,
}

/// `SLASH_COMMANDS`: the commands Patchwork offers for every agent.
const SLASH_COMMANDS: [(&str, &str, Runs); 7] = [
    (
        "/model",
        "Switch the response model for this chat",
        Runs::Pick,
    ),
    ("/plan", "Switch this chat into plan mode", Runs::Pick),
    (
        "/default",
        "Switch this chat back to normal mode",
        Runs::Pick,
    ),
    (
        "/goal",
        "Keep working until a condition is met",
        Runs::Agent,
    ),
    ("/new", "Start a fresh chat in this project", Runs::Send),
    (
        "/clear",
        "Delete this chat's messages and start the agent fresh",
        Runs::Send,
    ),
    (
        "/compact",
        "Summarise the conversation to free context",
        Runs::Send,
    ),
];

/// The command Patchwork runs itself when `text` is the whole message, if it is one.
pub fn local_command(text: &str) -> Option<&'static str> {
    SLASH_COMMANDS
        .iter()
        .find(|&&(name, _, runs)| runs != Runs::Agent && name == text)
        .map(|&(name, _, _)| name)
}

/// How well a command name matches the typed query (without the sigil): lower is better, and
/// `None` drops the command. Prefixes rank above matches that start a word, which rank above any
/// match.
fn command_score(name: &str, query: &str) -> Option<u8> {
    let bare = name.get(1..).unwrap_or_default();
    if bare == query {
        Some(0)
    } else if bare.starts_with(query) {
        Some(2)
    } else if bare
        .match_indices(query)
        .any(|(i, _)| i > 0 && bare[..i].ends_with(['-', '_', '/']))
    {
        Some(4)
    } else {
        bare.contains(query).then_some(6)
    }
}

/// The "@word", "$word", "&word" or "/word" the caret is at the end of, at the start of a line
/// or after a space.
#[derive(Clone)]
struct Token {
    kind: SigilKind,
    /// What was typed, with the default sigil in place of the user's.
    text: String,
    range: Range<usize>,
    /// The token is the whole draft, so a pick replaces the draft and can run a local command.
    whole: bool,
}

impl Token {
    fn query(&self) -> String {
        self.text[1..].to_lowercase()
    }
}

fn token(content: &str, caret: usize, sigils: &Sigils) -> Option<Token> {
    let before = content.get(..caret)?;
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let raw = &before[start..];
    let first = raw.chars().next()?;
    let kind = sigils.kind_of(first)?;
    Some(Token {
        kind,
        text: format!("{}{}", default_sigil(kind), &raw[first.len_utf8()..]),
        range: start..caret,
        whole: content == raw,
    })
}

/// One entry: a path, or a command, skill or agent with the default sigil.
struct Item {
    name: String,
    description: String,
}

/// Folders, written with a trailing "/", are listed with the files. Paths starting with what was
/// typed come first, then the shallower ones, so a folder drilled into lists what is directly in
/// it.
fn file_items(files: &[String], query: &str) -> Vec<Item> {
    let mut seen = HashSet::new();
    let folders: Vec<&str> = files
        .iter()
        .flat_map(|path| path.match_indices('/').map(move |(i, _)| &path[..=i]))
        .filter(|folder| seen.insert(*folder))
        .collect();
    let mut paths: Vec<&str> = folders
        .into_iter()
        .chain(files.iter().map(String::as_str))
        .filter(|path| path.to_lowercase().contains(query))
        .collect();
    paths.sort_by_key(|path| {
        (
            !path.to_lowercase().starts_with(query),
            path.trim_end_matches('/').split('/').count(),
        )
    });
    paths
        .into_iter()
        .take(FILE_LIMIT)
        .map(|path| Item {
            name: path.to_string(),
            description: String::new(),
        })
        .collect()
}

/// Patchwork's commands, then the agent's own, leaving out the ones Patchwork has and Claude's
/// copies of Patchwork's skills, which are under "$".
fn commands(agent: AgentKind, provider: &[ProviderSlashCommand]) -> Vec<Item> {
    let builtin: Vec<&str> = SLASH_COMMANDS
        .iter()
        .map(|(name, _, _)| &name[1..])
        .collect();
    let fallback = if agent == AgentKind::OpenCode {
        "OpenCode command"
    } else {
        "Claude Code command"
    };
    SLASH_COMMANDS
        .iter()
        // Patchwork does not run /goal in OpenCode.
        .filter(|(name, _, _)| agent != AgentKind::OpenCode || *name != "/goal")
        .map(|&(name, description, _)| Item {
            name: name.into(),
            description: description.into(),
        })
        .chain(provider.iter().filter_map(|c| {
            let name = c.name.strip_prefix('/').unwrap_or(&c.name);
            (!builtin.contains(&name) && !name.starts_with("claude:")).then(|| Item {
                name: format!("/{name}"),
                description: c.description.clone().unwrap_or_else(|| fallback.into()),
            })
        }))
        .collect()
}

/// The entries whose names match `query`, best first.
fn scored(items: Vec<Item>, query: &str) -> Vec<Item> {
    let mut scored: Vec<(u8, Item)> = items
        .into_iter()
        .filter_map(|item| Some((command_score(&item.name.to_lowercase(), query)?, item)))
        .collect();
    scored.sort_by_key(|(score, _)| *score);
    scored.into_iter().map(|(_, item)| item).collect()
}

#[derive(Default)]
pub struct SigilMenu {
    /// Escape hid the menu; it shows again once the draft changes.
    closed: bool,
    highlight: usize,
    /// The kind of word last under the caret: its list is read again each time a menu opens.
    kind: Option<SigilKind>,
    files: Vec<String>,
    /// The enabled skills' names and descriptions.
    skills: Vec<(String, Option<String>)>,
    /// The word and entries of the open menu, as last drawn.
    open: Option<(Token, Vec<Item>)>,
    scroll: ScrollHandle,
}

impl Composer {
    /// The draft changed: the menu shows again, from the top, for the word now typed.
    pub(super) fn draft_changed(&mut self) {
        self.sigil.closed = false;
        self.sigil.highlight = 0;
    }

    fn load_files(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let w = workspace.read(cx);
        let Some(project) = w.project().map(|p| p.id.clone()) else {
            self.sigil.files.clear();
            return;
        };
        let server = w.server.clone();
        let task = cx.background_spawn(async move { server.list_files(&project) });
        cx.spawn(async move |this, cx| {
            let files = task.await;
            this.update(cx, |this, cx| {
                match files {
                    Ok(files) => this.sigil.files = files,
                    Err(e) => log::warn!("could not list the project's files: {e}"),
                }
                cx.notify();
            })
        })
        .detach();
    }

    fn load_skills(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let settings = workspace.read(cx).settings.clone();
        let task = cx.background_spawn(async move { patchwork_server::skills::all(&settings) });
        cx.spawn(async move |this, cx| {
            let skills = task.await;
            this.update(cx, |this, cx| {
                this.sigil.skills = skills
                    .into_iter()
                    .filter(|s| s.enabled)
                    .map(|s| (s.name, s.description))
                    .collect();
                cx.notify();
            })
        })
        .detach();
    }

    /// Works out the menu for the word before the caret, reading its list when it opens, and
    /// draws it when it shows.
    pub(super) fn sigil_menu(
        &mut self,
        agent: AgentKind,
        provider: &[ProviderSlashCommand],
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let sigils = cx.try_global::<Sigils>().cloned().unwrap_or_default();
        let editor = self.editor.read(cx);
        let token = editor
            .caret()
            .and_then(|caret| token(&editor.content, caret, &sigils))
            // Antigravity reads the message as a prompt, so there are no commands or skills to
            // offer, but an agent summoned with "&" goes with any message.
            .filter(|tk| {
                agent != AgentKind::Antigravity
                    || matches!(tk.kind, SigilKind::Files | SigilKind::Agents)
            });
        let kind = token.as_ref().map(|tk| tk.kind);
        if kind != self.sigil.kind {
            self.sigil.kind = kind;
            match kind {
                Some(SigilKind::Files) => self.load_files(cx),
                Some(SigilKind::Skills) => self.load_skills(cx),
                Some(SigilKind::Agents) => self.load_roles(cx),
                _ => {}
            }
        }
        let open = token.filter(|_| !self.sigil.closed).and_then(|tk| {
            let query = tk.query();
            let items = match tk.kind {
                SigilKind::Files => return Some((file_items(&self.sigil.files, &query), tk)),
                SigilKind::Commands => commands(agent, provider),
                SigilKind::Skills => self
                    .sigil
                    .skills
                    .iter()
                    .map(|(name, description)| Item {
                        name: format!("${name}"),
                        description: description.clone().unwrap_or_else(|| "Skill".into()),
                    })
                    .collect(),
                SigilKind::Agents => match &self.roles {
                    Roles::Loaded(agents) => agents
                        .iter()
                        .map(|a| Item {
                            name: format!("&{}", a.name),
                            description: a.description.clone().unwrap_or_else(|| "Agent".into()),
                        })
                        .collect(),
                    Roles::Loading => Vec::new(),
                },
            };
            let items = scored(items, &query);
            // Later in a draft, a "/" that matches nothing is a path, so no menu shows for it.
            (tk.whole || !items.is_empty()).then_some((items, tk))
        });
        let has_items = open.as_ref().is_some_and(|(items, _)| !items.is_empty());
        self.editor.update(cx, |e, _| e.menu_keys = has_items);
        let Some((items, tk)) = open else {
            self.sigil.open = None;
            return None;
        };
        self.sigil.highlight = self.sigil.highlight.min(items.len().saturating_sub(1));
        let drawn = self.draw_sigil_menu(tk.kind, &items, &sigils, t, cx);
        self.sigil.open = Some((tk, items));
        Some(drawn)
    }

    fn draw_sigil_menu(
        &self,
        kind: SigilKind,
        items: &[Item],
        sigils: &Sigils,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (label, empty) = match kind {
            SigilKind::Files => ("Files", "No matching files"),
            SigilKind::Commands => ("Slash commands", "No matching command."),
            SigilKind::Skills => ("Skills", "No matching skill."),
            SigilKind::Agents => ("Agents", "No matching agent."),
        };
        let sigil = sigils.of(kind);
        let rows = items.iter().enumerate().map(|(i, item)| {
            let active = i == self.sigil.highlight;
            let hover = t.bg_hover;
            let shown = format!("{sigil}{}", item.name.get(1..).unwrap_or_default());
            let row = button(
                SharedString::from(format!("sigil-item-{i}")),
                if kind == SigilKind::Files {
                    item.name.clone()
                } else {
                    shown.clone()
                },
            )
            .role(Role::ListBoxOption)
            .aria_selected(active)
            .w_full()
            .flex_none()
            .px(px(8.))
            .py(px(6.))
            .rounded(px(RADIUS))
            .text_color(t.text)
            .map(|b| {
                if active {
                    b.bg(t.bg_selected)
                } else {
                    b.hover(move |s| s.bg(hover))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| this.sigil_select(i, window, cx)));
            if kind == SigilKind::Files {
                row.font_family(FONT_MONO)
                    .text_size(px(FS_SM))
                    .when(item.name.ends_with('/'), |r| {
                        r.child(div().flex_none().mr(px(6.)).child(icon("folder", 13.)))
                    })
                    .child(div().min_w_0().truncate().child(item.name.clone()))
            } else {
                row.items_start()
                    .gap(px(10.))
                    .child(div().flex_none().font_family(FONT_MONO).child(shown))
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child(item.description.clone()),
                    )
            }
        });
        let list = div()
            .id("sigil-menu")
            .role(Role::ListBox)
            .aria_label(label)
            .occlude()
            .flex()
            .flex_col()
            .gap(px(1.))
            .max_h(px(512.))
            .overflow_y_scroll()
            .track_scroll(&self.sigil.scroll)
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .p(px(4.))
            .shadow(popup_shadow(t))
            .font_weight(FontWeight::NORMAL)
            .when(items.is_empty(), |list| {
                list.child(
                    div()
                        .px(px(8.))
                        .py(px(6.))
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(empty),
                )
            })
            .children(rows);
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(relative(1.))
            .pb(px(6.))
            .child(deferred(list).with_priority(1))
            .into_any_element()
    }

    /// Up or Down while the menu has the keys.
    pub(super) fn sigil_arrow(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.sigil.open.as_ref().map_or(0, |(_, items)| items.len());
        if count > 0 {
            let next = (self.sigil.highlight as isize + step).rem_euclid(count as isize) as usize;
            self.sigil.highlight = next;
            self.sigil.scroll.scroll_to_item(next);
            cx.notify();
        }
    }

    /// Tab: a folder opens, listing what is in it; anything else is picked.
    pub(super) fn sigil_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tk, items)) = &self.sigil.open else {
            return;
        };
        let Some(item) = items.get(self.sigil.highlight) else {
            return;
        };
        if tk.kind == SigilKind::Files && item.name.ends_with('/') {
            let sigils = cx.try_global::<Sigils>().cloned().unwrap_or_default();
            let text = format!("{}{}", sigils.of(SigilKind::Files), item.name);
            let range = tk.range.clone();
            self.editor
                .update(cx, |e, cx| e.replace(range, &text, window, cx));
            return;
        }
        let name = item.name.clone();
        self.complete(&name, window, cx);
    }

    /// Enter picks the highlighted entry, unless nothing is listed or the command is typed in
    /// full, which sends it. True when it picked.
    pub(super) fn sigil_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((tk, items)) = &self.sigil.open else {
            return false;
        };
        let Some(item) = items.get(self.sigil.highlight) else {
            return false;
        };
        if tk.kind != SigilKind::Files && tk.text.to_lowercase() == item.name {
            return false;
        }
        let name = item.name.clone();
        self.complete(&name, window, cx);
        true
    }

    /// Escape hides the open menu until the draft changes. True when one was open.
    pub(super) fn sigil_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.sigil.open.is_none() {
            return false;
        }
        self.sigil.closed = true;
        cx.notify();
        true
    }

    /// A row was clicked: a command that runs on pick runs at once, and anything else goes in.
    fn sigil_select(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tk, items)) = &self.sigil.open else {
            return;
        };
        let Some(item) = items.get(i) else {
            return;
        };
        let name = item.name.clone();
        let picked = SLASH_COMMANDS
            .iter()
            .any(|&(n, _, runs)| n == name && runs == Runs::Pick);
        let local = local_command(&name).is_some();
        if tk.kind == SigilKind::Commands && (tk.whole || local) && picked {
            self.run_local(&name, window, cx);
            return;
        }
        self.complete(&name, window, cx);
    }

    /// Puts the pick in the draft. Later in the draft it replaces just the word typed; a pick
    /// for a draft that is only that word, or a local command, which only runs as the whole
    /// message, replaces the draft.
    fn complete(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tk, _)) = &self.sigil.open else {
            return;
        };
        let range = if tk.kind == SigilKind::Files || (!tk.whole && local_command(name).is_none()) {
            tk.range.clone()
        } else {
            0..self.editor.read(cx).content.len()
        };
        let text = format!("{name} ");
        self.editor
            .update(cx, |e, cx| e.replace(range, &text, window, cx));
        self.focus(window, cx);
    }

    /// Runs a command Patchwork handles itself (`runLocalCommand`), clearing the draft.
    pub(super) fn run_local(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        if name == "/clear" {
            return self.clear_chat(window, cx);
        }
        self.editor.update(cx, |e, cx| e.clear(window, cx));
        match name {
            "/compact" => workspace.update(cx, |w, cx| w.send_command("/compact", cx)),
            "/model" => self.open_menu(Menu::Model, window, cx),
            "/plan" | "/default" => {
                let on = name == "/plan";
                let picks = self.picks(workspace.read(cx));
                let thread = workspace.read(cx).thread.as_ref().map(|t| t.id.clone());
                if picks.agent == AgentKind::Antigravity {
                    workspace.update(cx, |w, cx| {
                        w.error = Some("Plan mode is not supported by Antigravity.".into());
                        cx.notify();
                    });
                } else if let Some(id) = thread {
                    workspace.update(cx, |w, cx| {
                        if let Err(e) = w.server.set_thread_plan_mode(&id, on) {
                            w.error = Some(e);
                        }
                        w.reload(cx);
                    });
                } else {
                    self.draft.plan_mode = on;
                }
            }
            // /new starts a fresh chat; the current one is kept in the sidebar.
            "/new" => workspace.update(cx, |w, cx| w.new_chat(cx)),
            _ => {}
        }
        cx.notify();
    }

    /// /clear asks first: the messages are deleted and cannot be recovered.
    fn clear_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(thread) = workspace.read(cx).thread.clone() else {
            self.editor.update(cx, |e, cx| e.clear(window, cx));
            return;
        };
        let detail = format!(
            "Clear {}? Its messages are deleted and the agent forgets this conversation.",
            crate::ui::display_title(&thread.title)
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            "Clear chat",
            Some(&detail),
            &["Clear", "Cancel"],
            cx,
        );
        let server = workspace.read(cx).server.clone();
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let id = thread.id.clone();
            let cleared = cx
                .background_spawn(async move { server.clear_thread(&id) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.editor.update(cx, |e, cx| e.clear(window, cx));
                this.attachments.clear();
                this.workspace
                    .update(cx, |w, cx| {
                        if let Err(e) = cleared {
                            w.error = Some(e);
                        }
                        w.reload(cx);
                    })
                    .ok();
            });
        })
        .detach();
    }
}
