//! Above the composer when the chat's local model cannot answer as asked (`ModelNotice.tsx`): its
//! server is not answering, the server no longer serves the model, or the message has images the
//! model does not read. Checked when the model changes, when a reply ends, and after Find server.

use gpui::{AnyElement, Context, Div, Role, Stateful, div, prelude::*, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::provider::api::{config, servers};
use patchwork_server::threads::AttachmentKind;

use super::{Composer};
use super::menus::{Menu, Pick};
use crate::ui::theme::{FS_SM, RADIUS, Theme};
use crate::ui::{BtnKind, btn, icon, spinner};

/// The most other models offered when the server stopped serving the chat's.
const MAX_OFFERED: usize = 3;

/// What the last check found for a model: its server's health, or none for a model that is not
/// on a local server.
#[derive(Default)]
pub(super) struct ModelHealth {
    checked: Option<(String, Option<config::ProviderHealth>)>,
    finding: bool,
    error: Option<String>,
}

impl Composer {
    /// Checks the chat's API model off the main thread, once per model until a reply ends.
    pub(super) fn check_model(
        &mut self,
        agent: AgentKind,
        model: Option<&str>,
        working: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(model) = model.filter(|_| agent == AgentKind::Api && !working) else {
            return;
        };
        if self
            .health
            .checked
            .as_ref()
            .is_some_and(|(checked, _)| checked == model)
        {
            return;
        }
        let model = model.to_string();
        self.health.checked = Some((model.clone(), None));
        let provider = model.split('/').next().unwrap_or_default().to_string();
        let task = cx.background_spawn(async move {
            config::provider(&provider)
                .filter(|p| p.local)
                .and_then(|_| config::probe(&provider).ok())
        });
        cx.spawn(async move |this, cx| {
            let health = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some((checked, found)) = this.health.checked.as_mut()
                    && *checked == model
                {
                    *found = health;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Checks again at the next draw, as when a reply ends.
    pub(super) fn recheck_model(&mut self) {
        self.health.checked = None;
    }

    /// Looks for where the model's server moved, and points its provider there.
    fn find_server(&mut self, provider: String, cx: &mut Context<Self>) {
        self.health.finding = true;
        self.health.error = None;
        let task = cx.background_spawn(async move { servers::relocate(&provider) });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                this.health.finding = false;
                match found {
                    Ok(_) => this.recheck_model(),
                    Err(e) => this.health.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn model_notice(
        &self,
        agent: AgentKind,
        model: Option<&str>,
        vision_set: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let model = model.filter(|_| agent == AgentKind::Api)?;
        let (checked, health) = self.health.checked.as_ref()?;
        let health = health.as_ref().filter(|_| checked == model)?;
        let provider = model.split('/').next().unwrap_or_default().to_string();
        let model_id = model[provider.len()..].trim_start_matches('/').to_string();
        let name = config::provider(&provider).map_or(provider.clone(), |p| p.name);
        let images = self
            .attachments
            .iter()
            .any(|a| a.kind == AttachmentKind::Image);
        let (text, actions): (String, Vec<AnyElement>) = if !health.reachable {
            let finding = self.health.finding;
            let find = btn("find-server", "Find server", BtnKind::Plain, finding, t)
                .map(|b| {
                    if finding {
                        b.child(spinner(13., t.text_muted))
                    } else {
                        b.child(icon("magnifying-glass", 13.))
                    }
                })
                .child("Find server")
                .when(!finding, |b| {
                    b.on_click(
                        cx.listener(move |this, _, _, cx| this.find_server(provider.clone(), cx)),
                    )
                });
            (
                format!("{name} isn't answering"),
                vec![find.into_any_element()],
            )
        } else if !health.served_models.contains(&model_id) {
            let offered = health
                .served_models
                .iter()
                .filter(|m| !m.to_lowercase().contains("embed"))
                .take(MAX_OFFERED)
                .map(|m| {
                    let pick = format!("{provider}/{m}");
                    btn(
                        format!("use-model-{m}"),
                        format!("Use {m}"),
                        BtnKind::Plain,
                        false,
                        t,
                    )
                    .child(format!("Use {m}"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.apply(Pick::Model(AgentKind::Api, Some(pick.clone())), window, cx)
                    }))
                    .into_any_element()
                })
                .collect();
            (format!("{name} no longer serves {model_id}"), offered)
        } else if images && health.images.get(&model_id) == Some(&false) && !vision_set {
            (format!("{model_id} doesn't read images"), Vec::new())
        } else {
            return None;
        };
        Some(
            div()
                .id("model-notice")
                .role(Role::Status)
                .mb(px(6.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .rounded(px(RADIUS))
                .border_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .px(px(10.))
                .py(px(6.))
                .text_size(px(FS_SM))
                .child(icon("warning", 14.).flex_none().text_color(t.warning))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(t.text)
                        .child(text),
                )
                .children(actions)
                .child(
                    btn("choose-model", "Choose model", BtnKind::Plain, false, t)
                        .child("Choose model")
                        .on_click(cx.listener(|this, e, window, cx| {
                            this.toggle_menu(Menu::Model, e, window, cx)
                        })),
                )
                .children(self.health.error.clone().map(|e| {
                    div()
                        .id("model-notice-error")
                        .role(Role::Alert)
                        .w_full()
                        .text_color(t.danger)
                        .child(e)
                })),
        )
    }
}
