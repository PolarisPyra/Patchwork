//! A CLI's Details (`providers/CliPane.tsx`): its version and path, its flags or settings, its
//! models, and its own read-only diagnostics. It takes the pane while it is open.

use gpui::{AnyElement, Context, FontWeight, Role, SharedString, Window, div, prelude::*, px};
use patchwork_server::provider::cli_info::InfoCommand;
use patchwork_server::provider::{AgentKind, ModelOption, antigravity, claude, codex, opencode};

use super::rows::{tildify};
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert};
use crate::ui::theme::{FONT_MONO, FS_LG, FS_SM, FS_XS, RADIUS, RADIUS_SM, theme};
use crate::ui::{BtnKind, agent_label, agent_logo, btn, icon, icon_button};

/// The field the rows are searched with.
const SEARCH_FIELD: &str = "cli-search";

/// One flag or setting of a CLI. `kept` is whether the filter chip keeps it; `source` is the file
/// that sets it.
struct Feature {
    name: String,
    detail: String,
    enabled: Option<bool>,
    kept: bool,
    source: Option<String>,
}

struct Model {
    id: String,
    name: String,
    context_window: Option<u64>,
    efforts: Vec<String>,
    tag: Option<String>,
}

/// Each CLI's reading in one shape, so one view draws them all.
pub(super) struct Info {
    path: String,
    version: String,
    features: Vec<Feature>,
    models: Vec<Model>,
}

/// What a CLI's rows are.
#[derive(Clone, Copy, PartialEq)]
enum Rows {
    Flags,
    Settings,
}

/// How each CLI is read and what its page offers (`CLIS`).
struct Cli {
    /// The binary's name, which titles a command's output.
    name: &'static str,
    /// Each command: its button, its output's title, and what it runs.
    commands: &'static [(&'static str, &'static str, InfoCommand)],
    rows: Option<Rows>,
    filter: Option<&'static str>,
    empty: &'static str,
}

/// The commands Claude Code and Codex share.
const SHARED: &[(&str, &str, InfoCommand)] = &[
    ("Doctor", "doctor", InfoCommand::Doctor),
    ("MCP servers", "mcp list", InfoCommand::McpList),
    ("Plugins", "plugin list", InfoCommand::PluginList),
];

fn cli(kind: AgentKind) -> Option<Cli> {
    Some(match kind {
        AgentKind::Claude => Cli {
            name: "claude",
            commands: SHARED,
            rows: Some(Rows::Flags),
            filter: Some("On only"),
            empty: "Claude Code has not cached its feature gates yet. Start it once, then read again.",
        },
        AgentKind::Codex => Cli {
            name: "codex",
            commands: SHARED,
            rows: Some(Rows::Flags),
            filter: Some("Stable only"),
            empty: "Nothing matches.",
        },
        AgentKind::OpenCode => Cli {
            name: "opencode",
            commands: &[
                ("Agents", "debug agents", InfoCommand::Agents),
                ("Paths", "debug paths", InfoCommand::Paths),
                ("MCP servers", "mcp list", InfoCommand::McpList),
                ("Plugins", "plugin list", InfoCommand::PluginList),
            ],
            rows: Some(Rows::Settings),
            filter: None,
            empty: "No config file sets anything yet.",
        },
        AgentKind::Antigravity => Cli {
            name: "agy",
            commands: &[
                ("Changelog", "changelog", InfoCommand::Changelog),
                ("MCP servers", "mcp list", InfoCommand::McpList),
                ("Plugins", "plugin list", InfoCommand::PluginList),
            ],
            rows: None,
            filter: None,
            empty: "Nothing matches.",
        },
        AgentKind::Api => return None,
    })
}

/// Whether Patchwork can read `kind`'s details, so its row offers them.
pub fn has_details(kind: AgentKind) -> bool {
    cli(kind).is_some()
}

/// OpenCode's and Antigravity's models, which carry no tag.
fn cli_models(models: Vec<ModelOption>) -> Vec<Model> {
    models
        .into_iter()
        .map(|m| Model {
            id: m.id.unwrap_or_else(|| m.label.clone()),
            name: m.label,
            context_window: m.context_window,
            efforts: m.efforts.unwrap_or_default(),
            tag: None,
        })
        .collect()
}

/// Reads the CLI. Claude Code's gates and catalog come from its caches, Codex's from the binary,
/// OpenCode's rows are the settings in its config files, and Antigravity shows none.
fn read(kind: AgentKind) -> Result<Info, String> {
    match kind {
        AgentKind::Claude => claude::cli::info().map(|i| Info {
            path: i.path,
            version: i.version,
            features: i
                .features
                .into_iter()
                .map(|f| Feature {
                    kept: f.enabled == Some(true),
                    name: f.name,
                    detail: f.value,
                    enabled: f.enabled,
                    source: None,
                })
                .collect(),
            models: i
                .models
                .into_iter()
                .map(|m| Model {
                    id: m.id,
                    name: m.name,
                    context_window: m.context_window,
                    efforts: m.efforts,
                    tag: m.fast_mode.then(|| "fast".to_string()),
                })
                .collect(),
        }),
        AgentKind::Codex => codex::cli::info().map(|i| Info {
            path: i.path,
            version: i.version,
            features: i
                .features
                .into_iter()
                .map(|f| Feature {
                    kept: f.stage == "stable",
                    name: f.name,
                    detail: f.stage,
                    enabled: Some(f.enabled),
                    source: None,
                })
                .collect(),
            models: i
                .models
                .into_iter()
                .map(|m| Model {
                    id: m.slug,
                    name: m.display_name,
                    context_window: m.context_window,
                    efforts: m.efforts,
                    tag: m.multi_agent,
                })
                .collect(),
        }),
        AgentKind::OpenCode => opencode::cli::info().map(|i| Info {
            path: i.path,
            version: i.version,
            features: i
                .settings
                .into_iter()
                .map(|s| Feature {
                    name: s.name,
                    detail: s.value,
                    enabled: None,
                    kept: true,
                    source: Some(s.source),
                })
                .collect(),
            models: cli_models(i.models),
        }),
        AgentKind::Antigravity => antigravity::info().map(|i| Info {
            path: i.path,
            version: i.version,
            features: Vec::new(),
            models: cli_models(i.models),
        }),
        AgentKind::Api => Err("API providers have no CLI".into()),
    }
}

fn run_command(kind: AgentKind, command: InfoCommand) -> Result<String, String> {
    match kind {
        AgentKind::Claude => claude::cli::command(command),
        AgentKind::Codex => codex::cli::command(command),
        AgentKind::OpenCode => opencode::cli::command(command),
        AgentKind::Antigravity => antigravity::command(command),
        AgentKind::Api => Err("API providers have no CLI".into()),
    }
}

/// The open Details page.
pub struct Details {
    kind: AgentKind,
    info: Option<Info>,
    error: Option<String>,
    query: String,
    filtered: bool,
    /// The last command's title and output.
    output: Option<(String, String)>,
    busy: bool,
}

impl SettingsDialog {
    pub(super) fn open_details(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.providers.details = Some(Details {
            kind,
            info: None,
            error: None,
            query: String::new(),
            filtered: false,
            output: None,
            busy: false,
        });
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.read_details(cx);
    }

    fn close_details(&mut self, cx: &mut Context<Self>) {
        self.providers.details = None;
        self.drop_fields(SEARCH_FIELD);
        cx.notify();
    }

    fn read_details(&mut self, cx: &mut Context<Self>) {
        let Some(details) = &mut self.providers.details else {
            return;
        };
        details.busy = true;
        details.error = None;
        let kind = details.kind;
        self.run(
            move || read(kind),
            move |d, info, _| {
                if let Some(details) = d.providers.details.as_mut().filter(|x| x.kind == kind) {
                    details.busy = false;
                    match info {
                        Ok(info) => details.info = Some(info),
                        Err(e) => details.error = Some(e),
                    }
                }
            },
            cx,
        );
        cx.notify();
    }

    fn run_details_command(&mut self, title: String, command: InfoCommand, cx: &mut Context<Self>) {
        let Some(details) = &mut self.providers.details else {
            return;
        };
        details.busy = true;
        details.error = None;
        details.output = None;
        let kind = details.kind;
        self.run(
            move || run_command(kind, command),
            move |d, out, _| {
                if let Some(details) = d.providers.details.as_mut().filter(|x| x.kind == kind) {
                    details.busy = false;
                    match out {
                        Ok(text) if text.trim().is_empty() => {
                            details.output = Some((title, "Nothing to report.".into()))
                        }
                        Ok(text) => details.output = Some((title, text)),
                        Err(e) => details.error = Some(e),
                    }
                }
            },
            cx,
        );
        cx.notify();
    }
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let Some(kind) = d.providers.details.as_ref().map(|x| x.kind) else {
        return div().into_any_element();
    };
    let Some(spec) = cli(kind) else {
        return div().into_any_element();
    };
    let query = d
        .providers
        .details
        .as_ref()
        .map(|x| x.query.clone())
        .unwrap_or_default();
    let search = spec.rows.map(|rows| {
        let what = if rows == Rows::Flags {
            "flags"
        } else {
            "settings"
        };
        d.field(
            SEARCH_FIELD,
            format!("Search {what}"),
            &query,
            &format!("Search {what}"),
            FieldKind {
                live: true,
                ..Default::default()
            },
            |d, text, cx| {
                if let Some(x) = &mut d.providers.details {
                    x.query = text;
                }
                cx.notify();
            },
            window,
            cx,
        )
        .w(px(208.))
        .flex_none()
        .gap(px(6.))
    });
    let Some(details) = &d.providers.details else {
        return div().into_any_element();
    };
    let busy = details.busy;
    let header = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .pb(px(16.))
        .child(
            icon_button("details-back", "Back", "caret-left", 16., cx)
                .tooltip(crate::ui::tooltip("Back"))
                .on_click(cx.listener(|d, _, _, cx| d.close_details(cx))),
        )
        .child(agent_logo(kind, None, 24.))
        .child(
            div()
                .id("details-title")
                .role(Role::Heading)
                .text_size(px(FS_LG))
                .font_weight(FontWeight::SEMIBOLD)
                .child(agent_label(kind)),
        );
    let read_again = |id: &'static str| {
        btn(id, "Read again", BtnKind::Plain, busy, &t)
            .child("Read again")
            .when(!busy, |b| {
                b.on_click(cx.listener(|d, _, _, cx| d.read_details(cx)))
            })
    };

    let Some(info) = &details.info else {
        return div()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_color(t.text_muted)
                    .child(details.error.clone().unwrap_or_else(|| "Reading…".into()))
                    .when(details.error.is_some(), |r| {
                        r.child(read_again("details-retry"))
                    }),
            )
            .into_any_element();
    };

    let q = details.query.trim().to_lowercase();
    let all = &info.features;
    let features: Vec<&Feature> = all
        .iter()
        .filter(|f| {
            (!details.filtered || f.kept)
                && (q.is_empty()
                    || format!("{} {}", f.name, f.detail)
                        .to_lowercase()
                        .contains(&q))
        })
        .collect();
    let enabled = all.iter().filter(|f| f.enabled == Some(true)).count();
    let filtered = details.filtered;

    let summary = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.))
        .pb(px(12.))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(info.version.clone()),
        )
        .child(
            div()
                .text_size(px(FS_SM))
                .text_color(t.text_subtle)
                .child(tildify(&info.path)),
        )
        .children(spec.rows.map(|rows| {
            div()
                .ml_auto()
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(match rows {
                    Rows::Flags => format!("{enabled} of {} flags on", all.len()),
                    Rows::Settings => format!("{} settings", all.len()),
                })
        }));

    let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);
    let commands: Vec<_> =
        spec.commands
            .iter()
            .map(|&(label, title, command)| {
                let title = format!("{} {title}", spec.name);
                btn(
                    SharedString::from(format!("cmd-{label}")),
                    label,
                    BtnKind::Plain,
                    busy,
                    &t,
                )
                .when(command == InfoCommand::Doctor, |b| {
                    b.child(icon("stethoscope", 13.))
                })
                .child(label)
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |d, _, _, cx| {
                        d.run_details_command(title.clone(), command, cx)
                    }))
                })
            })
            .collect();
    let tools = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.))
        .pb(px(8.))
        .children(spec.filter.map(|label| {
            crate::ui::button("details-filter", label)
                .aria_toggled(if filtered {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
                .h(px(28.))
                .px(px(10.))
                .rounded(px(RADIUS_SM))
                .border_1()
                .text_size(px(FS_SM))
                .map(|b| {
                    if filtered {
                        b.border_color(t.accent)
                            .bg(t.bg_accent_soft)
                            .text_color(t.accent_text)
                    } else {
                        b.border_color(t.border_strong)
                            .text_color(muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                    }
                })
                .child(label)
                .on_click(cx.listener(|d, _, _, cx| {
                    if let Some(x) = &mut d.providers.details {
                        x.filtered = !x.filtered;
                    }
                    cx.notify();
                }))
        }))
        .children(search.map(|s| {
            div()
                .flex()
                .items_center()
                .relative()
                .child(s.pl(px(26.)))
                .child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .text_color(muted)
                        .child(icon("magnifying-glass", 13.)),
                )
        }))
        .child(
            div()
                .ml_auto()
                .flex()
                .gap(px(6.))
                .children(commands)
                .child(read_again("details-read")),
        );

    let output = details.output.clone().map(|(title, text)| {
        div()
            .mb(px(12.))
            .overflow_hidden()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.border)
                    .py(px(4.))
                    .pr(px(4.))
                    .pl(px(12.))
                    .text_size(px(FS_XS))
                    .text_color(t.text_subtle)
                    .child(title)
                    .child(
                        btn("output-close", "Close", BtnKind::Plain, false, &t)
                            .child("Close")
                            .on_click(cx.listener(|d, _, _, cx| {
                                if let Some(x) = &mut d.providers.details {
                                    x.output = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("command-output")
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .px(px(12.))
                    .py(px(8.))
                    .font_family(FONT_MONO)
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .child(text),
            )
    });

    let rows: Vec<_> = features
        .iter()
        .map(|f| {
            let tip = match &f.source {
                Some(source) => format!("{}\n{}", f.detail, tildify(source)),
                None => f.detail.clone(),
            };
            div()
                .id(SharedString::from(format!("feature-{}", f.name)))
                .role(Role::Row)
                .flex()
                .items_center()
                .gap(px(8.))
                .border_b_1()
                .border_color(t.border)
                .px(px(8.))
                .py(px(4.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(FONT_MONO)
                        .text_size(px(12.))
                        .child(f.name.clone()),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("feature-detail-{}", f.name)))
                        .max_w(px(260.))
                        .truncate()
                        .font_family(FONT_MONO)
                        .text_size(px(FS_XS))
                        .text_color(t.text_muted)
                        .tooltip(crate::ui::tooltip(tip))
                        .child(f.detail.clone()),
                )
                .child(
                    div()
                        .w(px(32.))
                        .flex()
                        .justify_end()
                        .text_size(px(FS_SM))
                        .map(|c| match f.enabled {
                            Some(true) => c.text_color(t.accent_text).child("on"),
                            Some(false) => c.text_color(t.text_subtle).child("off"),
                            None => c,
                        }),
                )
        })
        .collect();
    let table = spec.rows.map(|kind| {
        let what = if kind == Rows::Flags {
            "Flags"
        } else {
            "Settings"
        };
        div()
            .id("cli-features")
            .role(Role::Table)
            .aria_label(what)
            .max_h(px(300.))
            .overflow_y_scroll()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .text_size(px(FS_SM))
            .children(rows)
            .when(features.is_empty(), |b| {
                b.child(
                    div()
                        .px(px(12.))
                        .py(px(16.))
                        .flex()
                        .justify_center()
                        .text_color(t.text_muted)
                        .child(if all.is_empty() {
                            spec.empty
                        } else {
                            "Nothing matches."
                        }),
                )
            })
    });

    let models = (!info.models.is_empty()).then(|| {
        div()
            .id("cli-models")
            .role(Role::List)
            .aria_label("Models")
            .mt(px(12.))
            .overflow_hidden()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .children(info.models.iter().enumerate().map(|(i, m)| {
                div()
                    .id(SharedString::from(format!("cli-model-{}", m.id)))
                    .role(Role::ListItem)
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(6.))
                    .when(i > 0, |r| r.border_t_1().border_color(t.border))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(m.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(FS_XS))
                            .text_color(t.text_subtle)
                            .child(
                                m.context_window
                                    .map(|w| format!("{}k", (w as f64 / 1000.).round()))
                                    .unwrap_or_default(),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(FS_XS))
                            .text_color(t.text_muted)
                            .child(m.efforts.join(" · ")),
                    )
                    .children(m.tag.clone().map(|tag| {
                        div()
                            .flex_none()
                            .rounded(px(RADIUS_SM))
                            .bg(t.bg_selected_muted)
                            .px(px(6.))
                            .text_size(px(FS_XS))
                            .text_color(t.text_muted)
                            .child(tag)
                    }))
            }))
    });

    div()
        .flex()
        .flex_col()
        .child(header)
        .child(summary)
        .child(tools)
        .children(output)
        .children(table)
        .children(models)
        .children(
            details
                .error
                .clone()
                .map(|e| alert("details-error", e, t.danger)),
        )
        .into_any_element()
}
