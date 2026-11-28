//! Patchwork's theme, mirrored into gpui-component's, so the kit's buttons, checkboxes,
//! tooltips and lists draw in the window's palette, fonts and corners.

// Rust guideline compliant 2026-11-27

use gpui::component::theme::{Theme as KitTheme, ThemeMode};
use gpui::{App, Hsla, px};

use crate::palette::{Theme, border_focus};
use crate::sizes::{font_mono, font_ui, fs_md, radius, radius_lg};

/// Copies `theme` into gpui-component's global theme. Does nothing before `gpui::init`.
pub(crate) fn mirror(theme: &Theme, cx: &mut App) {
    if !cx.has_global::<KitTheme>() {
        return;
    }
    let mode = if theme.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    // A mode change reloads the kit's own palette, so it goes first and the colours over it.
    KitTheme::update(cx, |kit| kit.mode = mode);
    let t = *theme;
    KitTheme::update(cx, |kit| {
        kit.font_family = font_ui();
        kit.mono_font_family = font_mono();
        kit.font_size = px(fs_md());
        kit.radius = px(radius());
        kit.radius_lg = px(radius_lg());
        kit.shadow = false;

        let c = &mut kit.colors;
        c.background = t.bg_app;
        c.foreground = t.text;
        c.border = t.border;
        c.input = t.border_strong;
        c.ring = border_focus();
        c.caret = t.text;
        c.selection = t.bg_selected;
        c.overlay = t.shadow;
        c.window_border = t.border;
        c.link = t.accent_text;
        c.link_hover = t.accent_text;
        c.link_active = t.accent_text;

        c.primary = t.accent;
        c.primary_hover = t.accent_hover;
        c.primary_active = t.accent_hover;
        c.primary_foreground = t.text_on_accent;
        c.secondary = t.bg_editor;
        c.secondary_hover = t.bg_hover;
        c.secondary_active = t.bg_selected;
        c.secondary_foreground = t.text;
        c.muted = t.bg_panel;
        c.muted_foreground = t.text_muted;
        c.accent = t.bg_hover;
        c.accent_foreground = t.text;
        c.popover = t.bg_elevated;
        c.popover_foreground = t.text;

        c.success = t.success;
        c.success_hover = t.success;
        c.success_active = t.success;
        c.success_foreground = t.text_on_accent;
        c.danger = t.danger;
        c.danger_hover = t.danger;
        c.danger_active = t.danger;
        c.danger_foreground = t.text_on_accent;
        c.warning = t.warning;
        c.warning_hover = t.warning;
        c.warning_active = t.warning;
        c.warning_foreground = t.text_on_accent;

        c.button = t.bg_editor;
        c.button_hover = t.bg_hover;
        c.button_active = t.bg_selected;
        c.button_foreground = t.text;
        c.button_primary = t.accent;
        c.button_primary_hover = t.accent_hover;
        c.button_primary_active = t.accent_hover;
        c.button_primary_foreground = t.text_on_accent;
        c.button_secondary = t.bg_editor;
        c.button_secondary_hover = t.bg_hover;
        c.button_secondary_active = t.bg_selected;
        c.button_secondary_foreground = t.text;
        c.button_success = t.success;
        // `filter: brightness(0.92)`, as Patchwork's own success button hovers.
        c.button_success_hover = Hsla {
            l: t.success.l * 0.92,
            ..t.success
        };
        c.button_success_active = t.success;
        c.button_success_foreground = t.text_on_accent;
        c.button_danger = t.danger;
        c.button_danger_hover = t.danger;
        c.button_danger_active = t.danger;
        c.button_danger_foreground = t.text_on_accent;

        c.list = t.bg_panel;
        c.list_hover = t.bg_hover;
        c.list_active = t.bg_selected;
        c.list_active_border = t.bg_selected;
        c.list_even = t.bg_panel;
        c.list_head = t.bg_panel;
        c.sidebar = t.bg_panel;
        c.sidebar_foreground = t.text;
        c.sidebar_border = t.border;
        c.sidebar_accent = t.bg_hover;
        c.sidebar_accent_foreground = t.text;
        c.sidebar_primary = t.accent;
        c.sidebar_primary_foreground = t.text_on_accent;
        c.title_bar = t.bg_panel;
        c.title_bar_border = t.border;
        c.status_bar = t.bg_panel;
        c.status_bar_border = t.border;
        c.tab_bar = t.bg_panel;
        c.tab = t.bg_panel;
        c.tab_active = t.bg_editor;
        c.tab_foreground = t.text_muted;
        c.tab_active_foreground = t.text;
        c.switch = t.border_strong;
        c.switch_thumb = t.bg_editor;
        c.slider_bar = t.accent;
        c.slider_thumb = t.bg_editor;
        c.progress_bar = t.accent;
        c.skeleton = t.bg_hover;
        c.drop_target = t.bg_accent_soft;
        c.drag_border = t.accent;
    });
}
