//! Settings → Terminal: how the integrated terminal starts, looks, and
//! behaves.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, FontWeight, SharedString, Window, div, prelude::*, px};
use patchwork_server::settings::{CursorStyle, DetectLocale, RightClickBehavior, TerminalSettings};

use crate::settings::SettingsDialog;
use crate::settings::controls::{section_heading, segmented, switch, token_row};
use crate::ui::theme::{fs_sm, theme};
use crate::ui::{BtnKind, btn, icon_button, tooltip};

/// Width of the settings' text fields (`TextField`'s default).
const FIELD_WIDTH: f32 = 220.;

impl SettingsDialog {
    /// Saves the terminal settings as `change` leaves them.
    fn set_terminal(&mut self, change: impl FnOnce(&mut TerminalSettings), cx: &mut Context<Self>) {
        self.update(|s| change(&mut s.terminal), cx);
    }
}

/// The terminal's environment variables (`EnvEditor`). A removed variable is kept as `None`,
/// which takes it out of the shell's environment.
fn env_editor(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let entries: Vec<(String, Option<String>)> = d
        .settings
        .terminal
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let rows: Vec<_> = entries
        .iter()
        .enumerate()
        .map(|(i, (name, value))| {
            let name_field = d.text_field(
                format!("term-env-name-{i}"),
                "Variable name",
                name,
                "",
                Some(160.),
                {
                    let (old, value) = (name.clone(), value.clone());
                    move |d, next, cx| {
                        let next = next.trim().to_string();
                        let (old, value) = (old.clone(), value.clone());
                        d.set_terminal(
                            |s| {
                                s.env.remove(&old);
                                if !next.is_empty() {
                                    s.env.insert(next, value);
                                }
                            },
                            cx,
                        );
                    }
                },
                window,
                cx,
            );
            let value_field = d.text_field(
                format!("term-env-value-{i}"),
                format!("{name} value"),
                value.as_deref().unwrap_or(""),
                if value.is_none() {
                    "Removed from the shell"
                } else {
                    ""
                },
                None,
                {
                    let name = name.clone();
                    move |d, next, cx| {
                        let name = name.clone();
                        d.set_terminal(
                            |s| {
                                s.env.insert(name, Some(next));
                            },
                            cx,
                        );
                    }
                },
                window,
                cx,
            );
            let removed = value.is_none();
            let (toggle, delete) = (name.clone(), name.clone());
            div()
                .mt(px(6.))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(name_field)
                .child(div().min_w_0().flex_1().child(value_field))
                .child(
                    btn(
                        SharedString::from(format!("term-env-remove-{i}")),
                        "Remove from shell",
                        BtnKind::Plain,
                        false,
                        &t,
                    )
                    .aria_toggled(if removed {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
                    .when(removed, |b| b.bg(t.bg_selected_muted))
                    .tooltip(tooltip("Take this variable out of the shell's environment"))
                    .child("Remove from shell")
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let name = toggle.clone();
                        d.set_terminal(
                            |s| {
                                s.env.insert(name, removed.then(String::new));
                            },
                            cx,
                        );
                    })),
                )
                .child(
                    icon_button(
                        SharedString::from(format!("term-env-delete-{i}")),
                        format!("Delete {name}"),
                        "x",
                        14.,
                        cx,
                    )
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let name = delete.clone();
                        d.set_terminal(
                            |s| {
                                s.env.remove(&name);
                            },
                            cx,
                        );
                    })),
                )
        })
        .collect();
    div()
        .border_b_1()
        .border_color(t.border)
        .py(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Environment variables"),
                        )
                        .child(
                            div()
                                .text_size(px(fs_sm()))
                                .text_color(t.text_muted)
                                .child("Values can use ${env:NAME} and ${workspaceFolder}"),
                        ),
                )
                .child(
                    btn("term-env-add", "Add", BtnKind::Plain, false, &t)
                        .child("Add")
                        .on_click(cx.listener(|d, _, _, cx| {
                            d.set_terminal(
                                |s| {
                                    let mut name = "NAME".to_string();
                                    let mut n = 2;
                                    while s.env.contains_key(&name) {
                                        name = format!("NAME_{n}");
                                        n += 1;
                                    }
                                    s.env.insert(name, Some(String::new()));
                                },
                                cx,
                            );
                        })),
                ),
        )
        .children(rows)
        .into_any_element()
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let term = d.settings.terminal.clone();

    let switch_row = |title: &'static str,
                      desc: Option<&str>,
                      on: bool,
                      set: fn(&mut TerminalSettings, bool),
                      cx: &mut Context<SettingsDialog>| {
        token_row(title, desc, &t).child(
            switch(SharedString::from(format!("term-{title}")), title, on, &t)
                .on_click(cx.listener(move |d, _, _, cx| d.set_terminal(|s| set(s, !on), cx))),
        )
    };
    macro_rules! number_row {
        ($title:expr, $desc:expr, $value:expr, $step:expr, $min:expr, $max:expr, $set:expr) => {{
            let set: fn(&mut TerminalSettings, f64) = $set;
            let field = d.number_field(
                concat!("term-", $title),
                $title,
                // Through its text, so an f32 such as 1.2 is not shown as 1.2000000476837158.
                format!("{}", $value).parse::<f64>().unwrap_or_default(),
                $step,
                $min,
                $max,
                move |d, n, cx| d.set_terminal(|s| set(s, n), cx),
                window,
                cx,
            );
            token_row($title, $desc, &t).child(field)
        }};
    }
    let text_row = |d: &mut SettingsDialog,
                    title: &'static str,
                    value: &str,
                    placeholder: &str,
                    width: f32,
                    set: fn(&mut TerminalSettings, String),
                    window: &mut Window,
                    cx: &mut Context<SettingsDialog>| {
        let field = d.text_field(
            format!("term-{title}"),
            title,
            value,
            placeholder,
            Some(width),
            move |d, text, cx| d.set_terminal(|s| set(s, text), cx),
            window,
            cx,
        );
        token_row(title, None, &t).child(field)
    };

    let cwd = text_row(
        d,
        "Starting folder",
        term.cwd.as_deref().unwrap_or(""),
        "Project folder",
        FIELD_WIDTH,
        |s, text| s.cwd = Some(text.trim().to_string()).filter(|c| !c.is_empty()),
        window,
        cx,
    );
    let locale = token_row(
        "UTF-8 locale",
        Some("Sets LANG; Auto only when it is not UTF-8"),
        &t,
    )
    .child(segmented(
        "term-locale",
        "UTF-8 locale",
        term.detect_locale,
        &[
            (DetectLocale::Auto, "Auto"),
            (DetectLocale::On, "On"),
            (DetectLocale::Off, "Off"),
        ],
        &t,
        |d, value, cx| d.set_terminal(|s| s.detect_locale = value, cx),
        cx,
    ));
    let env = env_editor(d, window, cx);
    let font = text_row(
        d,
        "Font",
        term.font_family.as_deref().unwrap_or(""),
        "App monospace font",
        FIELD_WIDTH,
        |s, text| s.font_family = Some(text.trim().to_string()).filter(|f| !f.is_empty()),
        window,
        cx,
    );
    let font_size = number_row!(
        "Font size",
        None,
        term.font_size,
        1.,
        Some(6.),
        Some(100.),
        |s, n| s.font_size = n as f32
    );
    let line_height = number_row!(
        "Line height",
        None,
        term.line_height,
        0.1,
        Some(1.),
        None,
        |s, n| s.line_height = n as f32
    );
    let letter_spacing = number_row!(
        "Letter spacing",
        None,
        term.letter_spacing,
        0.5,
        None,
        None,
        |s, n| s.letter_spacing = n as f32
    );
    let weight = text_row(
        d,
        "Font weight",
        &term.font_weight,
        "",
        80.,
        |s, text| {
            if !text.trim().is_empty() {
                s.font_weight = text.trim().to_string();
            }
        },
        window,
        cx,
    );
    let weight_bold = text_row(
        d,
        "Bold font weight",
        &term.font_weight_bold,
        "",
        80.,
        |s, text| {
            if !text.trim().is_empty() {
                s.font_weight_bold = text.trim().to_string();
            }
        },
        window,
        cx,
    );
    let cursor = token_row("Cursor", None, &t).child(segmented(
        "term-cursor",
        "Cursor style",
        term.cursor_style,
        &[
            (CursorStyle::Block, "Block"),
            (CursorStyle::Line, "Line"),
            (CursorStyle::Underline, "Underline"),
        ],
        &t,
        |d, value, cx| d.set_terminal(|s| s.cursor_style = value, cx),
        cx,
    ));
    let cursor_width = (term.cursor_style == CursorStyle::Line).then(|| {
        number_row!(
            "Cursor width",
            None,
            term.cursor_width,
            1.,
            Some(1.),
            None,
            |s, n| s.cursor_width = n as u32
        )
    });
    let contrast = number_row!(
        "Minimum contrast ratio",
        None,
        term.minimum_contrast_ratio,
        0.5,
        Some(1.),
        Some(21.),
        |s, n| s.minimum_contrast_ratio = n as f32
    );
    let scrollback = number_row!(
        "Scrollback",
        Some("Lines kept above the screen"),
        term.scrollback,
        1.,
        Some(0.),
        None,
        |s, n| s.scrollback = n as u32
    );
    let right_click = token_row("Right click", None, &t).child(segmented(
        "term-right-click",
        "Right click",
        term.right_click_behavior,
        &[
            (RightClickBehavior::Default, "Menu"),
            (RightClickBehavior::CopyPaste, "Copy or paste"),
            (RightClickBehavior::Paste, "Paste"),
            (RightClickBehavior::SelectWord, "Select word"),
            (RightClickBehavior::Nothing, "Nothing"),
        ],
        &t,
        |d, value, cx| d.set_terminal(|s| s.right_click_behavior = value, cx),
        cx,
    ));
    let scroll_speed = number_row!(
        "Scroll speed",
        None,
        term.mouse_wheel_scroll_sensitivity,
        0.5,
        Some(0.1),
        None,
        |s, n| s.mouse_wheel_scroll_sensitivity = n as f32
    );
    let fast_scroll = number_row!(
        "Fast scroll speed",
        Some("While Alt is held"),
        term.fast_scroll_sensitivity,
        0.5,
        Some(0.1),
        None,
        |s, n| s.fast_scroll_sensitivity = n as f32
    );

    div()
        .flex()
        .flex_col()
        .child(section_heading("Shell", true, &t))
        .child(cwd)
        .child(switch_row(
            "Inherit environment",
            Some("Start from Patchwork's environment"),
            term.inherit_env,
            |s, on| s.inherit_env = on,
            cx,
        ))
        .child(locale)
        .child(env)
        .child(section_heading("Appearance", false, &t))
        .child(font)
        .child(font_size)
        .child(line_height)
        .child(letter_spacing)
        .child(weight)
        .child(weight_bold)
        .child(cursor)
        .children(cursor_width)
        .child(switch_row(
            "Cursor blinking",
            None,
            term.cursor_blinking,
            |s, on| s.cursor_blinking = on,
            cx,
        ))
        .child(contrast)
        .child(switch_row(
            "Bold text in bright colors",
            None,
            term.draw_bold_text_in_bright_colors,
            |s, on| s.draw_bold_text_in_bright_colors = on,
            cx,
        ))
        .child(section_heading("Behavior", false, &t))
        .child(scrollback)
        .child(switch_row(
            "Copy on selection",
            None,
            term.copy_on_selection,
            |s, on| s.copy_on_selection = on,
            cx,
        ))
        .child(right_click)
        .child(switch_row(
            "Alt-click moves the cursor",
            None,
            term.alt_click_moves_cursor,
            |s, on| s.alt_click_moves_cursor = on,
            cx,
        ))
        .when(cfg!(target_os = "macos"), |col| {
            col.child(switch_row(
                "Option is Meta",
                None,
                term.mac_option_is_meta,
                |s, on| s.mac_option_is_meta = on,
                cx,
            ))
        })
        .child(switch_row(
            "Smooth scrolling",
            None,
            term.smooth_scrolling,
            |s, on| s.smooth_scrolling = on,
            cx,
        ))
        .child(scroll_speed)
        .child(fast_scroll)
        .child(
            div().mt(px(16.)).flex().child(
                btn("term-reset", "Reset to defaults", BtnKind::Plain, false, &t)
                    .child("Reset to defaults")
                    .on_click(cx.listener(|d, _, _, cx| {
                        d.set_terminal(|s| *s = TerminalSettings::default(), cx);
                    })),
            ),
        )
        .into_any_element()
}
