//! A plan the agent proposed in plan mode. A long plan shows its
//! start until expanded; the menu copies or saves it as Markdown.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    ClipboardItem, Div, FontWeight, Role, SharedString, div, linear_color_stop, linear_gradient,
    prelude::*, px,
};

use crate::ui::markdown::markdown;
use crate::chat::plan::plan_title;
use crate::ui::theme::{FS_MD, FS_SM, RADIUS, RADIUS_SM, Theme};
use crate::ui::{BtnKind, badge, btn, button, icon, popup_shadow, slug_file_name};
use crate::workspace::Workspace;

/// How tall a collapsed plan is drawn.
const COLLAPSED_HEIGHT: f32 = 416.;

/// The plans expanded, the one whose menu is open, and the one just copied, by card.
#[derive(Default)]
pub struct ProposedPlans {
    expanded: HashSet<String>,
    menu: Option<String>,
    copied: Option<String>,
}

/// The plan without its title heading, and without a "Summary" heading right after it.
fn plan_body(markdown: &str) -> String {
    let is_heading = |line: &str| {
        let rest = line.trim_start_matches(' ');
        let hashes = rest.len() - rest.trim_start_matches('#').len();
        line.len() - rest.len() <= 3
            && (1..=6).contains(&hashes)
            && rest[hashes..].starts_with([' ', '\t'])
    };
    let mut lines: Vec<&str> = markdown.trim().lines().collect();
    if lines.first().is_some_and(|l| is_heading(l)) {
        lines.remove(0);
    }
    if let Some(next) = lines.iter().position(|l| !l.trim().is_empty())
        && is_heading(lines[next])
        && lines[next]
            .trim()
            .trim_start_matches('#')
            .trim()
            .eq_ignore_ascii_case("summary")
    {
        lines.remove(next);
    }
    lines.join("\n").trim().to_string()
}

impl Workspace {
    fn copy_plan(&mut self, key: String, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.proposed.copied = Some(key.clone());
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                // How long "Copied" shows before the button goes back to its icon.
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.proposed.copied.as_ref() == Some(&key) {
                    this.proposed.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Asks where to save the plan, and writes it there.
    fn download_plan(&mut self, title: &str, text: String, cx: &mut Context<Self>) {
        self.proposed.menu = None;
        let home = patchwork_server::system::fs::home_dir().unwrap_or_default();
        let picked = cx.prompt_for_new_path(&home, Some(&slug_file_name(title, "plan")));
        let server = Arc::clone(&self.server);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let path = path.to_string_lossy().into_owned();
            let saved = cx
                .background_spawn(async move { server.save_text_file(&path, &text) })
                .await;
            if let Err(e) = saved {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(e.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }
}

pub fn proposed_plan(
    w: &Workspace,
    key: String,
    source: &str,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let title = plan_title(source);
    let body = plan_body(source);
    let can_collapse = body.len() > 900 || body.split('\n').count() > 20;
    let expanded = w.proposed.expanded.contains(&key);
    let collapsed = can_collapse && !expanded;
    let preview = if collapsed {
        let mut lines: Vec<&str> = body
            .split('\n')
            .filter(|l| !l.trim().is_empty())
            .take(10)
            .collect();
        lines.extend(["", "..."]);
        lines.join("\n")
    } else {
        body
    };
    let menu_open = w.proposed.menu.as_ref() == Some(&key);
    let copied = w.proposed.copied.as_ref() == Some(&key);
    let full = format!("{}\n", source.trim_end());
    let (hover, fg) = (t.bg_hover, t.text);

    let item = |id: &str, label: &'static str| {
        div()
            .id(SharedString::from(format!("{id}-{key}")))
            .role(Role::MenuItem)
            .aria_label(label)
            .cursor_pointer()
            .flex()
            .w_full()
            .items_center()
            .rounded(px(RADIUS_SM))
            .px(px(8.))
            .py(px(5.))
            .whitespace_nowrap()
            .text_color(fg)
            .hover(move |s| s.bg(hover))
            .child(label)
    };
    let menu = menu_open.then(|| {
        let copy = {
            let (key, text) = (key.clone(), full.clone());
            item(
                "plan-copy",
                if copied {
                    "Copied!"
                } else {
                    "Copy to clipboard"
                },
            )
            .on_click(cx.listener(move |w, _, _, cx| w.copy_plan(key.clone(), text.clone(), cx)))
        };
        let download = {
            let (title, text) = (title.clone(), full.clone());
            item("plan-download", "Download as markdown")
                .on_click(cx.listener(move |w, _, _, cx| w.download_plan(&title, text.clone(), cx)))
        };
        div()
            .id(SharedString::from(format!("plan-menu-{key}")))
            .role(Role::Menu)
            .aria_label("Plan actions")
            .absolute()
            .top(px(28.))
            .right_0()
            .flex()
            .flex_col()
            .min_w(px(180.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .p(px(4.))
            .text_size(px(FS_SM))
            .shadow(popup_shadow(t))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| {
                w.proposed.menu = None;
                cx.notify();
            }))
            .child(copy)
            .child(download)
    });
    let actions = {
        let key = key.clone();
        button(
            SharedString::from(format!("plan-actions-{key}")),
            "Plan actions",
        )
        .aria_expanded(menu_open)
        .justify_center()
        .size(px(24.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(t.border)
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon("dots-three", 14.))
        .on_click(cx.listener(move |w, _, _, cx| {
            w.proposed.menu = if w.proposed.menu.as_ref() == Some(&key) {
                None
            } else {
                Some(key.clone())
            };
            cx.notify();
        }))
    };
    let panel = t.bg_panel;
    div()
        .rounded(px(16.))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .p(px(16.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(badge("Plan", t))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(FS_MD))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text)
                        .child(title),
                )
                .child(div().relative().flex_none().child(actions).children(menu)),
        )
        .child(
            div()
                .relative()
                .mt(px(16.))
                .when(collapsed, |d| {
                    d.max_h(px(COLLAPSED_HEIGHT)).overflow_hidden()
                })
                .child(markdown(&preview, format!("{key}-body"), cx))
                .when(collapsed, |d| {
                    d.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(96.))
                            .bg(linear_gradient(
                                0.,
                                linear_color_stop(panel, 0.),
                                linear_color_stop(panel.opacity(0.), 1.),
                            )),
                    )
                }),
        )
        .when(can_collapse, |d| {
            let label = if expanded {
                "Collapse plan"
            } else {
                "Expand plan"
            };
            d.child(
                div().mt(px(12.)).flex().justify_center().child(
                    btn(
                        SharedString::from(format!("plan-expand-{key}")),
                        label,
                        BtnKind::Plain,
                        false,
                        t,
                    )
                    .aria_expanded(expanded)
                    .child(label)
                    .on_click(cx.listener(move |w, _, _, cx| {
                        if !w.proposed.expanded.remove(&key) {
                            w.proposed.expanded.insert(key.clone());
                        }
                        cx.notify();
                    })),
                ),
            )
        })
}
