//! A bot's home: the bot as last saved, its fields, what is open, and what the page does.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;
use gpui::{Entity, FocusHandle, Focusable, PathPromptOptions, WeakEntity, Window, prelude::*};
use patchwork_core::Core;
use patchwork_core::bots::{BlockedTools, Bot, BotUsage, KnowledgeFile, STARTING_POINTS};
use patchwork_core::permissions::PermissionKind;
use crate::ui::text_area::{Submitted, TextArea};
use crate::workspace::Workspace;
use super::sections::Drill;

/// The tabs of the details beside the conversation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Details,
    Library,
    Computer,
}

impl Tab {
    pub const ALL: [(Tab, &'static str); 3] = [
        (Tab::Details, "Details"),
        (Tab::Library, "Library"),
        (Tab::Computer, "Computer"),
    ];
}

/// How much of the page is drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The whole page, in place of a chat.
    Page,
    /// Only the details, beside the bot's chat, which shows or hides them.
    BesideChat,
}

pub struct BotPage {
    pub(super) workspace: WeakEntity<Workspace>,
    pub(super) core: Arc<Core>,
    pub bot: Bot,
    pub(super) tab: Tab,
    pub(super) name: Entity<TextArea>,
    /// The linked project picked for the next run; None until one is picked.
    pub(super) run_in: Option<String>,
    pub(super) knowledge: Vec<KnowledgeFile>,
    /// The bot's runs, listed as its conversations; None until read.
    pub(super) usage: Option<BotUsage>,
    /// A run is being started.
    pub starting: bool,
    /// The details are shown beside the conversation.
    pub(super) details_open: bool,
    /// Whether the page is drawn whole, or only its details beside the bot's chat.
    pub layout: Layout,
    /// The page open in place of the tab, if any.
    pub(super) drill: Option<Drill>,
    /// What the bot is told, read when the Inspector opens; None until read.
    pub(super) brief: Option<Result<String, String>>,
    /// The starting points were closed without picking one.
    pub(super) start_closed: bool,
    /// The MCP servers the bot could block, read when the Computer tab opens.
    pub(super) servers: Vec<String>,
    pub(super) focus: FocusHandle,
}

impl Focusable for BotPage {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BotPage {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        core: Arc<Core>,
        bot: Bot,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = |text: &str, label: &str, single: bool, cx: &mut Context<Self>| {
            let (text, label) = (text.to_string(), label.to_string());
            cx.new(|cx| {
                let mut area = TextArea::new(String::new(), label, cx);
                area.single_line = single;
                area.placeholder_color = Some(crate::ui::theme::input_placeholder());
                area.set_text(&text, cx);
                area
            })
        };
        let name = field(&bot.name, "Name", true, cx);
        // Moving the cursor out saves the name.
        cx.subscribe_in(&name, window, |this, _, _: &Submitted, window, cx| {
            window.focus(&this.focus, cx);
        })
        .detach();
        let focus = name.read(cx).focus_handle(cx);
        cx.on_focus_out(&focus, window, |this, _, _, cx| this.save_name(cx))
            .detach();
        let mut page = Self {
            workspace,
            core,
            bot,
            tab: Tab::Details,
            name,
            run_in: None,
            knowledge: Vec::new(),
            usage: None,
            starting: false,
            details_open: false,
            layout,
            drill: None,
            brief: None,
            start_closed: false,
            servers: Vec::new(),
            focus: cx.focus_handle(),
        };
        page.load_knowledge(cx);
        page.load_usage(cx);
        // Made while the window's workspace is being drawn, which the composer belongs to.
        if layout == Layout::Page {
            cx.defer_in(window, |this, window, cx| this.focus_composer(window, cx));
        }
        page
    }

    /// Shows the error in the window's error banner.
    fn fail(&self, error: &impl ToString, cx: &mut Context<Self>) {
        let error = error.to_string();
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |w, cx| {
                w.error = Some(error);
                cx.notify();
            });
        }
    }

    pub(super) fn open_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.drill = None;
        match tab {
            Tab::Details => self.load_usage(cx),
            Tab::Library => self.load_knowledge(cx),
            Tab::Computer => self.load_servers(cx),
        }
        cx.notify();
    }

    /// Saves `bot` off the UI thread, and shows it as saved, or the error and the bot as it was.
    fn save(&mut self, bot: Bot, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let task = cx.background_spawn(async move { core.save_bot(bot) });
        cx.spawn(async move |this, cx| {
            let saved = task.await;
            let _ = this.update(cx, |this, cx| {
                match saved {
                    Ok(bot) => this.show_saved(bot, cx),
                    Err(e) => {
                        this.fail(&e, cx);
                        // A name turned down goes back to the saved one.
                        this.show_saved(this.bot.clone(), cx);
                    }
                }
                this.refresh_sidebar(cx);
            });
        })
        .detach();
    }

    /// Shows `bot` as saved: its name in the name field, and a project it no longer works in
    /// no longer picked.
    pub fn show_saved(&mut self, bot: Bot, cx: &mut Context<Self>) {
        if bot.id != self.bot.id {
            return;
        }
        self.name.update(cx, |a, cx| {
            if a.content != bot.name {
                a.set_text(&bot.name, cx);
            }
        });
        if let Some(run_in) = &self.run_in
            && !bot.projects.contains(run_in)
        {
            self.run_in = None;
        }
        self.bot = bot;
        cx.notify();
    }

    /// Saves the name when it changed.
    fn save_name(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).content.clone();
        if name == self.bot.name {
            return;
        }
        let bot = Bot {
            name,
            ..self.bot.clone()
        };
        self.save(bot, cx);
    }

    /// Shows the name as a field, with the cursor in it.
    pub(super) fn edit_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.name.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Drops the typed name and shows the saved one.
    pub(super) fn cancel_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.bot.name.clone();
        self.name.update(cx, |a, cx| a.set_text(&name, cx));
        window.focus(&self.focus, cx);
    }

    pub(super) fn toggle_project(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut bot = self.bot.clone();
        if bot.projects.iter().any(|p| p == id) {
            bot.projects.retain(|p| p != id);
        } else {
            bot.projects.push(id.to_string());
        }
        self.save(bot, cx);
    }

    /// Blocks the bot's runs from a kind of call, or lets them make it again.
    pub(super) fn toggle_kind(&mut self, kind: PermissionKind, cx: &mut Context<Self>) {
        let mut blocked = self.bot.blocked.clone();
        if blocked.kinds.contains(&kind) {
            blocked.kinds.retain(|k| *k != kind);
        } else {
            blocked.kinds.push(kind);
        }
        self.save_blocked(blocked, cx);
    }

    /// Blocks the bot's runs from MCP server `name`, or lets them use it again.
    pub(super) fn toggle_server(&mut self, name: &str, cx: &mut Context<Self>) {
        let mut blocked = self.bot.blocked.clone();
        if blocked.servers.iter().any(|s| s == name) {
            blocked.servers.retain(|s| s != name);
        } else {
            blocked.servers.push(name.to_string());
        }
        self.save_blocked(blocked, cx);
    }

    /// Saves the blocked tools off the UI thread, and shows the bot as saved.
    fn save_blocked(&mut self, blocked: BlockedTools, cx: &mut Context<Self>) {
        let (core, id) = (Arc::clone(&self.core), self.bot.id.clone());
        let task = cx.background_spawn(async move { core.set_bot_blocked(&id, blocked) });
        cx.spawn(async move |this, cx| {
            let saved = task.await;
            let _ = this.update(cx, |this, cx| match saved {
                Ok(bot) => this.show_saved(bot, cx),
                Err(e) => this.fail(&e, cx),
            });
        })
        .detach();
    }

    /// Reads the MCP servers the bot could block, off the UI thread.
    fn load_servers(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let task = cx.background_spawn(async move { core.mcp_server_names() });
        cx.spawn(async move |this, cx| {
            let names = task.await;
            let _ = this.update(cx, |this, cx| {
                this.servers = names;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn set_run_in(&mut self, project: Option<String>, cx: &mut Context<Self>) {
        self.run_in = project;
        cx.notify();
    }

    /// The bot's linked projects that still exist, as id and name.
    pub(super) fn linked_projects(&self) -> Vec<(String, String)> {
        let projects = self.core.settings().projects;
        self.bot
            .projects
            .iter()
            .filter_map(|id| projects.iter().find(|p| &p.id == id))
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect()
    }

    /// Where the next run works: the project picked, else the first linked one. None when no
    /// project is linked, and the run works in the bot's folder.
    pub fn run_place(&self) -> Option<String> {
        let linked = self.linked_projects();
        self.run_in
            .clone()
            .filter(|id| linked.iter().any(|(l, _)| l == id))
            .or_else(|| linked.into_iter().next().map(|(id, _)| id))
    }

    /// Shows whether a run is being started.
    pub fn set_starting(&mut self, starting: bool, cx: &mut Context<Self>) {
        self.starting = starting;
        cx.notify();
    }

    /// Asks for files and gives the bot a copy of each.
    pub(super) fn add_files(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        let core = Arc::clone(&self.core);
        let id = self.bot.id.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let added = cx
                .background_spawn(async move {
                    let errors: Vec<String> = paths
                        .iter()
                        .filter_map(|path: &PathBuf| core.add_knowledge(&id, path).err())
                        .map(|e| e.to_string())
                        .collect();
                    (errors, core.knowledge_files(&id))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let (errors, files) = added;
                this.knowledge = files;
                if !errors.is_empty() {
                    this.fail(&errors.join("\n"), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn remove_file(&mut self, name: &str, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let (id, name) = (self.bot.id.clone(), name.to_string());
        let task = cx.background_spawn(async move {
            let removed = core.remove_knowledge(&id, &name);
            (removed, core.knowledge_files(&id))
        });
        cx.spawn(async move |this, cx| {
            let (removed, files) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.knowledge = files;
                if let Err(e) = removed {
                    this.fail(&e, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_knowledge(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let id = self.bot.id.clone();
        let task = cx.background_spawn(async move { core.knowledge_files(&id) });
        cx.spawn(async move |this, cx| {
            let files = task.await;
            let _ = this.update(cx, |this, cx| {
                this.knowledge = files;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn load_usage(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let id = self.bot.id.clone();
        let task = cx.background_spawn(async move { core.bot_usage(&id) });
        cx.spawn(async move |this, cx| {
            let usage = task.await;
            let _ = this.update(cx, |this, cx| {
                this.usage = Some(usage);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_run(&self, thread_id: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            let id = thread_id.to_string();
            workspace.update(cx, |w, cx| w.open_thread(&id, cx));
        }
    }

    /// Puts the cursor in the message field.
    pub(super) fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            let editor = workspace.read(cx).composer.read(cx).editor.clone();
            window.focus(&editor.read(cx).focus_handle(cx), cx);
        }
    }

    /// Shows or hides the details beside the conversation.
    pub(super) fn toggle_details(&mut self, cx: &mut Context<Self>) {
        if self.layout == Layout::BesideChat {
            let _ = self.workspace.update(cx, Workspace::toggle_bot_details);
            return;
        }
        self.details_open = !self.details_open;
        cx.notify();
    }

    /// Whether the starting points are offered: while the bot has no instructions and they
    /// were not closed.
    pub(super) fn offers_start(&self) -> bool {
        !self.start_closed && self.bot.instructions.trim().is_empty()
    }

    /// Starts the bot in starting point `index`: its instructions, then the message field. The
    /// last one, Something else, goes straight to the message field.
    pub(super) fn pick_start(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(point) = STARTING_POINTS.get(index) else {
            return;
        };
        if point.instructions.is_empty() {
            self.close_start(window, cx);
            return;
        }
        self.start_closed = true;
        let bot = Bot {
            instructions: point.instructions.to_string(),
            ..self.bot.clone()
        };
        self.save(bot, cx);
        self.focus_composer(window, cx);
    }

    /// Closes the starting points without picking one.
    pub(super) fn close_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_closed = true;
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Has the sidebar show the bot's name as saved.
    fn refresh_sidebar(&self, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, Workspace::reload_bots);
        }
    }
}
