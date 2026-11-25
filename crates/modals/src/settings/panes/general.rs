//! The app icon picker of Settings → Interface, and the state the section list keeps.

// Rust guideline compliant 2026-10-09

use std::sync::OnceLock;

use gpui::{AnyElement, FocusHandle, FontWeight, Role, SharedString, div, img, prelude::*, px};

use crate::settings::SettingsDialog;
use crate::settings::controls::{anchored, radio};
use patchwork_ui::theme::{radius_sm, theme};
use patchwork_ui::tooltip;

/// The pane's own state.
#[derive(Default)]
pub struct GeneralState {
    /// The section list, so Up and Down move along it while it has focus.
    pub nav_focus: Option<FocusHandle>,
}

/// One app icon of `assets/logos/manifest.json`.
struct Logo {
    id: String,
    name: String,
}

/// The app icon picked in Settings, as an asset path.
pub(in crate::settings) fn app_icon(s: &patchwork_core::settings::Settings) -> SharedString {
    let id = s.logo.clone().unwrap_or_else(|| logos().1.clone());
    SharedString::from(format!("app-icons/{id}.svg"))
}

/// Puts the app icon picked in `s` in the desktop's launcher, title bar and taskbar, off the UI
/// thread.
pub fn show_app_icon(s: &patchwork_core::settings::Settings, cx: &gpui::App) {
    use gpui::AssetSource;
    match patchwork_ui::brand::assets::Assets.load(&app_icon(s)) {
        Ok(Some(svg)) => cx
            .background_spawn(async move { patchwork_core::system::set_launcher_icon(&svg) })
            .detach(),
        Ok(None) => log::warn!("the picked app icon is missing"),
        Err(e) => log::warn!(error:% = e; "the picked app icon cannot be read: {{error}}"),
    }
}

/// The app icons, and the one used when none is picked.
fn logos() -> &'static (Vec<Logo>, String) {
    static LOGOS: OnceLock<(Vec<Logo>, String)> = OnceLock::new();
    LOGOS.get_or_init(|| {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../assets/logos/manifest.json"))
                .unwrap_or_default();
        let text =
            |v: &serde_json::Value, key: &str| v[key].as_str().unwrap_or_default().to_string();
        let icons = manifest["icons"]
            .as_array()
            .map(|icons| {
                icons
                    .iter()
                    .map(|i| Logo {
                        id: text(i, "id"),
                        name: text(i, "name"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        (icons, text(&manifest, "default"))
    })
}

/// The App icon row: every app icon, with the picked one marked.
pub(in crate::settings) fn app_icon_row(
    d: &mut SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let (logos, default_logo) = logos();
    let logo = d
        .settings
        .logo
        .clone()
        .unwrap_or_else(|| default_logo.clone());

    let icons = div()
        .id("app-icons")
        .role(Role::RadioGroup)
        .aria_label("App icon")
        .mt(px(8.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .children(logos.chunks(8).map(|row| {
            div().flex().gap(px(4.)).children(
                row.iter()
                    .map(|l| {
                        let checked = l.id == logo;
                        let id = l.id.clone();
                        radio(
                            SharedString::from(format!("logo-{}", l.id)),
                            l.name.clone(),
                            checked,
                        )
                        // Eight to a row share its width, so the row ends where the pane does.
                        .flex_1()
                        .min_w_0()
                        .aspect_square()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(radius_sm()))
                        .border_1()
                        .map(|b| {
                            if checked {
                                b.border_color(t.accent).bg(t.bg_accent_soft)
                            } else {
                                b.border_color(gpui::transparent_black())
                                    .hover(move |s| s.bg(t.bg_hover))
                            }
                        })
                        .tooltip(tooltip(l.name.clone()))
                        .child(
                            img(SharedString::from(format!("app-icons/{}.svg", l.id)))
                                .size(px(64.)),
                        )
                        .on_click(cx.listener(move |d, _, _, cx| {
                            let id = id.clone();
                            d.update(|s| s.logo = Some(id), cx);
                            show_app_icon(&d.core.settings(), cx);
                        }))
                        .into_any_element()
                    })
                    .chain(
                        // A short last row keeps its icons the size of the others.
                        (row.len()..8).map(|_| div().flex_1().into_any_element()),
                    ),
            )
        }));

    anchored(div().id("row-app-icon"), "App icon")
        .border_b_1()
        .border_color(t.border)
        .py(px(patchwork_ui::theme::roomy(10.)))
        .child(div().font_weight(FontWeight::SEMIBOLD).child("App icon"))
        .child(icons)
        .into_any_element()
}
