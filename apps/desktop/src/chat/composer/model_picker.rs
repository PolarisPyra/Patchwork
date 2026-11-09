//! The model picker (`modelPicker/PickerPopup.tsx`): the providers down the left, a search field
//! over the list of models, and the Legacy models panel beside it. Starred models are kept across
//! restarts.

use gpui::{
    AnyElement, Context, Entity, Focusable, FontWeight, KeyDownEvent, MouseDownEvent, Role,
    SharedString, Window, div, prelude::*, px,
};
use patchwork_server::Server;
use patchwork_server::provider::{AgentKind, ModelOption};

use super::{Composer};
use super::menus::Pick;
use crate::ui::AGENT_KINDS;
use crate::chat::text_area::TextArea;
use crate::ui::theme::{FS_SM, Theme};
use crate::ui::{
    agent_label, agent_logo, api_model_place, api_provider_name, button, icon, popup_shadow,
    spinner,
};

/// Where the starred models are kept, as `agent:model` keys.
const FAVORITES_KEY: &str = "patchwork-model-favorites";

/// The picker's height, and its panels' widths.
const HEIGHT: f32 = 346.;
const WIDTH: f32 = 360.;
const LEGACY_WIDTH: f32 = 300.;

pub struct ModelPicker {
    pub search: Entity<TextArea>,
    /// The provider whose models show, or None for the favorites.
    pub provider: Option<AgentKind>,
    /// The provider whose Legacy models panel is open.
    pub legacy_open: Option<AgentKind>,
    pub favorites: Vec<String>,
    /// "Use a local model" is looking for one, or why it couldn't set one up.
    pub setting_up: bool,
    pub set_up_error: Option<String>,
}

/// A model in the list: its agent, the option, and its `agent:model` key.
#[derive(Clone)]
pub struct ModelRow {
    pub kind: AgentKind,
    pub item: ModelOption,
    pub key: String,
}

pub fn row_key(kind: AgentKind, item: &ModelOption) -> String {
    format!(
        "{}:{}",
        agent_label(kind).to_lowercase(),
        item.id.as_deref().unwrap_or("default")
    )
}

pub fn read_favorites(server: &Server) -> Vec<String> {
    server
        .store
        .state(FAVORITES_KEY)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save_favorites(server: &Server, favorites: &[String]) {
    let saved = serde_json::to_string(favorites)
        .map_err(|e| e.to_string())
        .and_then(|json| server.store.set_state(FAVORITES_KEY, &json));
    if let Err(e) = saved {
        log::warn!("could not save the favorite models: {e}");
    }
}

/// The name a model's second line gives where it comes from.
fn source_of(kind: AgentKind, item: &ModelOption) -> String {
    match (&item.id, kind) {
        (None, _) => item
            .default_model
            .clone()
            .unwrap_or_else(|| "CLI default".into()),
        (Some(id), AgentKind::Api) => [
            Some(api_provider_name(id).unwrap_or_else(|| "API".to_string())),
            api_model_place(id),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · "),
        _ => agent_label(kind).into(),
    }
}

impl Composer {
    /// The rows the list shows: a search's matches, the favorites, or one provider's current
    /// models; favorites first, then current models, then legacy ones.
    pub(super) fn model_rows(&self, agents: &[AgentKind], cx: &gpui::App) -> Vec<ModelRow> {
        let Some(picker) = &self.picker else {
            return Vec::new();
        };
        let query = picker.search.read(cx).content.trim().to_lowercase();
        let favorites = &picker.favorites;
        let legacy =
            |item: &ModelOption, key: &str| item.is_legacy && !favorites.iter().any(|f| f == key);
        let rank = |row: &ModelRow| {
            if favorites.contains(&row.key) {
                2
            } else if row.item.is_legacy {
                0
            } else {
                1
            }
        };
        let mut rows: Vec<ModelRow> = agents
            .iter()
            .flat_map(|&kind| {
                self.models_of(kind)
                    .unwrap_or_default()
                    .iter()
                    .map(move |item| ModelRow {
                        kind,
                        key: row_key(kind, item),
                        item: item.clone(),
                    })
            })
            .filter(|row| {
                if !query.is_empty() {
                    let id = row.item.id.as_deref();
                    format!(
                        "{} {} {} {} {}",
                        row.item.label,
                        row.item.default_model.as_deref().unwrap_or(""),
                        agent_label(row.kind),
                        id.and_then(api_provider_name).unwrap_or_default(),
                        id.and_then(api_model_place).unwrap_or_default(),
                    )
                    .to_lowercase()
                    .contains(&query)
                } else {
                    match picker.provider {
                        None => favorites.contains(&row.key),
                        Some(kind) => row.kind == kind && !legacy(&row.item, &row.key),
                    }
                }
            })
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(rank(row)));
        rows
    }

    /// The provider's legacy models, shown in their own panel.
    fn legacy_rows(&self, kind: AgentKind) -> Vec<ModelRow> {
        let favorites = self
            .picker
            .as_ref()
            .map(|p| p.favorites.clone())
            .unwrap_or_default();
        self.models_of(kind)
            .unwrap_or_default()
            .iter()
            .map(|item| ModelRow {
                kind,
                key: row_key(kind, item),
                item: item.clone(),
            })
            .filter(|row| row.item.is_legacy && !favorites.contains(&row.key))
            .collect()
    }

    fn toggle_favorite(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        if let Some(i) = picker.favorites.iter().position(|f| f == key) {
            picker.favorites.remove(i);
        } else {
            picker.favorites.push(key.to_string());
        }
        if let Some(workspace) = self.workspace.upgrade() {
            save_favorites(&workspace.read(cx).server, &picker.favorites);
        }
        cx.notify();
    }

    /// Sets up a model server running on this machine and picks its model.
    fn set_up_local_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(picker) = &mut self.picker else {
            return;
        };
        picker.setting_up = true;
        picker.set_up_error = None;
        let set_up = workspace.update(cx, |w, cx| w.set_up_local_model(cx));
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let set_up = set_up.await;
            let _ = this.update_in(cx, |this, window, cx| match set_up {
                Ok(model) => this.apply(Pick::Model(AgentKind::Api, Some(model)), window, cx),
                Err(e) => {
                    if let Some(picker) = &mut this.picker {
                        picker.setting_up = false;
                        picker.set_up_error = Some(e);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The picker over the window, its rows already in `self.menu_picks` for the keyboard.
    pub(super) fn model_picker(
        &mut self,
        agents: &[AgentKind],
        current: (AgentKind, Option<&str>),
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = self.model_rows(agents, cx);
        self.menu_picks = rows
            .iter()
            .map(|r| Pick::Model(r.kind, r.item.id.clone()))
            .collect();
        let Some(picker) = &self.picker else {
            return div().into_any_element();
        };
        let searching = !picker.search.read(cx).content.trim().is_empty();
        let provider = picker.provider;
        let favorites = picker.favorites.clone();
        let legacy_toggle = (!searching).then_some(provider).flatten();
        let legacy = legacy_toggle
            .map(|kind| self.legacy_rows(kind))
            .unwrap_or_default();
        let legacy_open = legacy_toggle.filter(|k| picker.legacy_open == Some(*k));
        let search_focused = picker.search.read(cx).focus_handle(cx).is_focused(window);
        let search = picker.search.clone();
        let highlight = self.highlight;

        let model_row = |row: &ModelRow, index: Option<usize>, cx: &mut Context<Self>| {
            let selected = row.kind == current.0 && row.item.id.as_deref() == current.1;
            let favorite = favorites.contains(&row.key);
            let pick = Pick::Model(row.kind, row.item.id.clone());
            let key = row.key.clone();
            let label = row.item.label.clone();
            let hover = t.bg_hover;
            let shortcut = index.filter(|&i| i < 9).map(|i| {
                div()
                    .flex_none()
                    .rounded(px(4.))
                    .bg(t.text_tint_6)
                    .px(px(4.))
                    .py(px(2.))
                    .text_size(px(FS_SM))
                    .line_height(px(16.))
                    .text_color(t.text_muted)
                    .child(format!("Ctrl+{}", i + 1))
            });
            let (star_fg, star_hover) = if favorite {
                (t.accent_text, t.accent_text)
            } else {
                (t.text_subtle, t.text)
            };
            div()
                .id(SharedString::from(format!("model-{}", row.key)))
                .role(Role::ListBoxOption)
                .aria_label(label.clone())
                .aria_selected(selected)
                .when(index.is_none(), |r| r.tab_index(0))
                .mb(px(2.))
                .min_h(px(44.))
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(4.))
                .px(px(8.))
                .py(px(4.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .when(index.is_some_and(|i| i == highlight), |r| r.bg(hover))
                .when_some(index, |r, i| {
                    r.on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.highlight != i {
                            this.highlight = i;
                            cx.notify();
                        }
                    }))
                })
                .on_click(
                    cx.listener(move |this, _, window, cx| this.apply(pick.clone(), window, cx)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(px(4.))
                        .text_size(px(FS_SM))
                        .line_height(px(16.5))
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(label.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_color(t.text_subtle)
                                .child(agent_logo(
                                    row.kind,
                                    row.item.id.as_deref().or(row.item.default_model.as_deref()),
                                    12.,
                                ))
                                .child(div().truncate().child(source_of(row.kind, &row.item))),
                        ),
                )
                .children(shortcut)
                .child(
                    button(
                        SharedString::from(format!("star-{}", row.key)),
                        if favorite {
                            format!("Remove {label} from favorites")
                        } else {
                            format!("Add {label} to favorites")
                        },
                    )
                    .size(px(24.))
                    .flex_none()
                    .justify_center()
                    .rounded(px(4.))
                    .mr(px(-4.))
                    .text_color(star_fg)
                    .hover(move |s| s.bg(hover).text_color(star_hover))
                    .child(icon(if favorite { "star-fill" } else { "star" }, 12.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_favorite(&key, cx);
                    })),
                )
        };

        let rail = (!searching).then(|| {
            let rail_button = |id: SharedString,
                               label: String,
                               pressed: bool,
                               logo: AnyElement,
                               target: Option<AgentKind>,
                               cx: &mut Context<Self>| {
                let hover = t.bg_hover;
                button(id, label)
                    .relative()
                    .size(px(36.))
                    .flex_none()
                    .justify_center()
                    .rounded(px(6.))
                    .text_color(t.text)
                    .hover(move |s| s.bg(hover))
                    .aria_toggled(if pressed {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
                    .child(logo)
                    .when(pressed, |b| {
                        b.child(
                            div()
                                .absolute()
                                .top(px(8.))
                                .right(px(-4.))
                                .w(px(3.))
                                .h(px(20.))
                                .rounded_l(px(3.))
                                .bg(t.accent),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(picker) = &mut this.picker {
                            picker.provider = target;
                            let focus = picker.search.read(cx).focus_handle(cx);
                            window.focus(&focus, cx);
                        }
                        cx.notify();
                    }))
            };
            let mut nav = div()
                .id("providers")
                .role(Role::Navigation)
                .aria_label("Providers")
                .flex()
                .flex_col()
                .flex_none()
                .w(px(44.))
                .gap(px(4.))
                .p(px(4.))
                .bg(t.bg_app.opacity(0.3))
                .child(rail_button(
                    "rail-favorites".into(),
                    "Favorites".into(),
                    provider.is_none(),
                    icon("star-fill", 20.).into_any_element(),
                    None,
                    cx,
                ))
                .child(div().h(px(1.)).flex_none().bg(t.border));
            for &kind in agents {
                nav = nav.child(rail_button(
                    SharedString::from(format!("rail-{}", agent_label(kind))),
                    agent_label(kind).into(),
                    provider == Some(kind),
                    agent_logo(kind, None, 20.),
                    Some(kind),
                    cx,
                ));
            }
            nav
        });

        let list_rows: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| model_row(row, Some(i), cx))
            .collect();
        let legacy_toggle_row = legacy_toggle.filter(|_| !legacy.is_empty()).map(|kind| {
            let hover = t.bg_hover;
            button("legacy-toggle", "Legacy models")
                .aria_expanded(legacy_open.is_some())
                .mb(px(2.))
                .min_h(px(44.))
                .w_full()
                .gap(px(6.))
                .rounded(px(4.))
                .px(px(8.))
                .py(px(4.))
                .hover(move |s| s.bg(hover))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap(px(4.))
                        .text_size(px(FS_SM))
                        .line_height(px(16.5))
                        .child(div().font_weight(FontWeight::MEDIUM).child("Legacy models"))
                        .child(
                            div()
                                .text_color(t.text_subtle)
                                .child(legacy.len().to_string()),
                        ),
                )
                .child(icon("caret-right", 14.).text_color(t.text_subtle))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(picker) = &mut this.picker {
                        picker.legacy_open = if picker.legacy_open == Some(kind) {
                            None
                        } else {
                            Some(kind)
                        };
                        let focus = picker.search.read(cx).focus_handle(cx);
                        window.focus(&focus, cx);
                    }
                    cx.notify();
                }))
        });
        // With no local model yet, the list ends with a way to set one up.
        let has_local = self
            .models_of(AgentKind::Api)
            .unwrap_or_default()
            .iter()
            .any(|m| m.id.as_deref().and_then(api_model_place).is_some());
        let setting_up = picker.setting_up;
        let set_up_error = picker.set_up_error.clone();
        let set_up_row = (!has_local).then(|| {
            let hover = t.bg_hover;
            button("use-local-model", "Use a local model")
                .mb(px(2.))
                .min_h(px(44.))
                .w_full()
                .min_w_0()
                .gap(px(6.))
                .rounded(px(4.))
                .px(px(8.))
                .py(px(4.))
                .text_size(px(FS_SM))
                .line_height(px(16.5))
                .font_weight(FontWeight::MEDIUM)
                .when(!setting_up, |b| b.hover(move |s| s.bg(hover)))
                .child(if setting_up {
                    spinner(14., t.text).into_any_element()
                } else {
                    icon("desktop", 14.).flex_none().into_any_element()
                })
                .child("Use a local model")
                .when(!setting_up, |b| {
                    b.on_click(
                        cx.listener(|this, _, window, cx| this.set_up_local_model(window, cx)),
                    )
                })
        });
        let set_up_alert = set_up_error.map(|e| {
            div()
                .id("use-local-model-error")
                .role(Role::Alert)
                .px(px(8.))
                .pb(px(4.))
                .text_size(px(FS_SM))
                .line_height(px(16.5))
                .text_color(t.danger)
                .child(e)
        });
        let empty = (rows.is_empty() && legacy_toggle_row.is_none()).then(|| {
            div()
                .p(px(8.))
                .text_center()
                .text_size(px(14.))
                .text_color(t.text_subtle)
                .child("No models found")
        });
        let flyout = legacy_open.map(|_| {
            let legacy_rows: Vec<_> = legacy.iter().map(|row| model_row(row, None, cx)).collect();
            div()
                .id("legacy-models")
                .role(Role::ListBox)
                .aria_label("Legacy models")
                .flex()
                .flex_col()
                .w(px(LEGACY_WIDTH))
                .h_full()
                .overflow_hidden()
                .rounded(px(8.))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.bg_elevated)
                .shadow(popup_shadow(t))
                .child(
                    div()
                        .px(px(12.))
                        .pt(px(12.))
                        .pb(px(6.))
                        .text_size(px(FS_SM))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text_subtle)
                        .child("Legacy models"),
                )
                .child(
                    div()
                        .id("legacy-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px(px(8.))
                        .pb(px(6.))
                        .children(legacy_rows),
                )
        });

        div()
            .id("model-picker")
            .occlude()
            .flex()
            .gap(px(6.))
            .h(px(HEIGHT))
            .on_mouse_down_out(cx.listener(|this, e: &MouseDownEvent, _, cx| {
                this.dismissed = this.menu.take().map(|m| (m, e.position));
                this.picker = None;
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::picker_key))
            .child(
                div()
                    .id("model-panel")
                    .role(Role::Dialog)
                    .aria_label("Choose model")
                    .flex()
                    .w(px(WIDTH))
                    .h_full()
                    .overflow_hidden()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .text_color(t.text)
                    .shadow(popup_shadow(t))
                    .children(rail)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .when(!searching, |c| c.border_l_1().border_color(t.border))
                            .child(
                                div()
                                    .mx(px(12.))
                                    .mt(px(9.))
                                    .h(px(34.))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .border_b_1()
                                    .border_color(if search_focused { t.accent } else { t.border })
                                    .pb(px(6.))
                                    .text_color(t.text_subtle)
                                    .child(icon("magnifying-glass", 16.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(14.))
                                            .line_height(px(20.))
                                            .child(search),
                                    ),
                            )
                            .child(
                                div()
                                    .id("model-options")
                                    .role(Role::ListBox)
                                    .aria_label("Models")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .py(px(6.))
                                    .px(px(8.))
                                    .children(list_rows)
                                    .children(legacy_toggle_row)
                                    .children(set_up_row)
                                    .children(set_up_alert)
                                    .children(empty),
                            ),
                    ),
            )
            .children(flyout)
            .into_any_element()
    }

    /// Escape closes the picker, and Ctrl+1 to Ctrl+9 pick from the top of the list. Up, Down
    /// and Enter come from the search field.
    fn picker_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = &event.keystroke;
        if keys.key == "escape" {
            self.close_menu(window, cx);
        } else if keys.modifiers.control
            && let Some(n) = keys
                .key
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=9).contains(n))
        {
            if let Some(pick) = self.menu_picks.get(n - 1).cloned() {
                self.apply(pick, window, cx);
            }
        } else {
            return;
        }
        cx.stop_propagation();
    }

    /// Opens the picker on the favorites if one is still listed, else on the chat's agent; and on
    /// its Legacy models panel when the chat's model is a legacy one.
    pub(super) fn open_model_picker(
        &mut self,
        agents: &[AgentKind],
        current: (AgentKind, Option<String>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let favorites = read_favorites(&workspace.read(cx).server);
        let has_favorite = agents.iter().any(|&kind| {
            self.models_of(kind)
                .unwrap_or_default()
                .iter()
                .any(|m| favorites.contains(&row_key(kind, m)))
        });
        let (agent, model) = current;
        let active_legacy = self
            .models_of(agent)
            .unwrap_or_default()
            .iter()
            .find(|m| m.id == model)
            .is_some_and(|m| m.is_legacy && !favorites.contains(&row_key(agent, m)));
        let t = *crate::ui::theme::theme(cx);
        let search = cx.new(|cx| {
            let mut field = TextArea::new("Search models...", "Search models", cx);
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = t.text_subtle;
            field.selection_color = t.accent.opacity(0.3);
            field
        });
        cx.subscribe_in(
            &search,
            window,
            |this, _, _: &crate::chat::text_area::Changed, _, cx| {
                this.highlight = 0;
                cx.notify();
            },
        )
        .detach();
        cx.subscribe_in(
            &search,
            window,
            |this, _, arrow: &crate::chat::text_area::Arrow, _, cx| {
                let count = this.menu_picks.len();
                if count > 0 {
                    this.highlight =
                        (this.highlight as isize + arrow.0).rem_euclid(count as isize) as usize;
                }
                cx.notify();
            },
        )
        .detach();
        cx.subscribe_in(
            &search,
            window,
            |this, _, _: &crate::chat::text_area::Submitted, window, cx| {
                if let Some(pick) = this.menu_picks.get(this.highlight).cloned() {
                    this.apply(pick, window, cx);
                }
            },
        )
        .detach();
        let focus = search.read(cx).focus_handle(cx);
        self.picker = Some(ModelPicker {
            search,
            provider: if agents.len() > 1 && has_favorite {
                None
            } else {
                Some(agent)
            },
            legacy_open: active_legacy.then_some(agent),
            favorites,
            setting_up: false,
            set_up_error: None,
        });
        // The chat's model starts highlighted.
        let rows = self.model_rows(agents, cx);
        self.highlight = rows
            .iter()
            .position(|r| r.kind == agent && r.item.id == model)
            .unwrap_or(0);
        window.focus(&focus, cx);
    }
}

/// The agents a model can come from, in the pickers' order.
pub fn ordered(agents: &[AgentKind]) -> Vec<AgentKind> {
    AGENT_KINDS
        .into_iter()
        .filter(|k| agents.contains(k))
        .collect()
}
