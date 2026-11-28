//! A CLI's Details: its version and path, its flags or settings, its
//! models, and its own read-only diagnostics. It takes the pane while it is open.

// Rust guideline compliant 2026-10-09

use patchwork_client::local::core::provider::{AgentKind, ProviderKind};
use patchwork_client::local::core::provider::cli_info::{
    CliPage, InfoCommand, CliFeature as Feature, CliRows as Rows, CliReading as Info,
};
use gpui::{prelude::*, px, AnyElement, FontWeight, Role, SharedString, Window, div};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{font_mono, fs_sm, fs_xs, radius, radius_sm, theme};
use patchwork_design_components::{BtnKind, btn};
use patchwork_design_icons::icon;

use crate::settings::SettingsDialog;
use patchwork_views_logos::{agent_label, agent_logo};
use crate::settings::controls::{tildify, FieldKind, alert, detail_header};

/// The field the rows are searched with.
const SEARCH_FIELD: &str = "cli-search";

/// What `kind`'s Details page offers, when Patchwork can read its CLI.
fn cli(kind: AgentKind) -> Option<CliPage> {
    kind.provider().cli_page()
}

/// Whether Patchwork can read `kind`'s details, so its row offers them.
pub fn has_details(kind: AgentKind) -> bool {
    cli(kind).is_some()
}

/// Reads `kind`'s CLI.
fn read(kind: AgentKind) -> Result<Info, patchwork_client::local::core::Error> {
    kind.provider().read_cli()
}

/// Runs one of `kind`'s read-only diagnostics commands.
fn run_command(
    kind: AgentKind,
    command: InfoCommand,
) -> Result<String, patchwork_client::local::core::Error> {
    kind.provider().run_cli_command(command)
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
    pub(in crate::settings::providers) fn open_details(
        &mut self,
        kind: AgentKind,
        cx: &mut Context<Self>,
    ) {
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
                        Err(e) => details.error = Some(e.to_string()),
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
                            details.output = Some((title, "Nothing to report.".into()));
                        }
                        Ok(text) => details.output = Some((title, text)),
                        Err(e) => details.error = Some(e.to_string()),
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
    let back = |d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>| d.close_details(cx);
    let logo = agent_logo(kind, None, 24.);
    let header = detail_header("details", logo, agent_label(kind), back, cx)
        .pt(px(12.))
        .pb(px(16.));
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
                .text_size(px(fs_sm()))
                .text_color(t.text_subtle)
                .child(tildify(&info.path)),
        )
        .children(spec.rows.map(|rows| {
            div()
                .ml_auto()
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(match rows {
                    Rows::Flags => format!("{enabled} of {} flags on", all.len()),
                    Rows::Settings => format!("{} settings", all.len()),
                })
        }));

    let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);
    let commands: Vec<_> = spec
        .commands
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
                b.child(icon(IconName::Stethoscope, 13.))
            })
            .child(label)
            .when(!busy, |b| {
                b.on_click(cx.listener(move |d, _, _, cx| {
                    d.run_details_command(title.clone(), command, cx);
                }))
            })
        })
        .collect();
    let tools = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.))
        .pb(px(12.))
        .children(spec.filter.map(|label| {
            patchwork_design_components::button("details-filter", label)
                .aria_toggled(filtered.into())
                .h(px(28.))
                .px(px(10.))
                .rounded(px(radius_sm()))
                .border_1()
                .text_size(px(fs_sm()))
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
                        .child(icon(IconName::MagnifyingGlass, 13.)),
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
            .rounded(px(radius()))
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
                    .text_size(px(fs_xs()))
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
                    .font_family(font_mono())
                    .text_size(px(patchwork_design_theme::fs(12.)))
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
                        .font_family(font_mono())
                        .text_size(px(patchwork_design_theme::fs(12.)))
                        .child(f.name.clone()),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("feature-detail-{}", f.name)))
                        .max_w(px(260.))
                        .truncate()
                        .font_family(font_mono())
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .tooltip(patchwork_design_components::tooltip(tip))
                        .child(f.detail.clone()),
                )
                .child(
                    div()
                        .w(px(32.))
                        .flex()
                        .justify_end()
                        .text_size(px(fs_sm()))
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
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .text_size(px(fs_sm()))
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
            .rounded(px(radius()))
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
                            .text_size(px(fs_xs()))
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
                            .text_size(px(fs_xs()))
                            .text_color(t.text_muted)
                            .child(m.efforts.join(" · ")),
                    )
                    .children(m.tag.clone().map(|tag| {
                        div()
                            .flex_none()
                            .rounded(px(radius_sm()))
                            .bg(t.bg_selected_muted)
                            .px(px(6.))
                            .text_size(px(fs_xs()))
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
