//! Settings → Notifications: which events notify on the desktop and on the chat apps, when the
//! chat apps hear about them, and the chats and projects muted on the chat apps.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Role, div, prelude::*, px};
use patchwork_core::notifications::{ChatAppsWhen, Kind, Notices};

use crate::settings::SettingsDialog;
use crate::settings::controls::{section_heading, segmented, switch, token_row};
use crate::ui::theme::{fs_sm, theme};
use crate::ui::{BtnKind, btn};

/// The events, each with its row's title.
const EVENTS: [(Kind, &str); 5] = [
    (Kind::Asks, "Asks something"),
    (Kind::Finished, "Finished"),
    (Kind::Goal, "Goal met"),
    (Kind::Failed, "Failed"),
    (Kind::Usage, "Usage runs out"),
];

/// When the chat apps are sent notifications.
const WHEN: [(ChatAppsWhen, &str); 2] = [
    (ChatAppsWhen::Away, "When I'm away"),
    (ChatAppsWhen::Always, "Always"),
];

/// How long Patchwork must be out of focus before you count as away, in seconds.
const AWAY_AFTER: [(u32, &str); 4] = [
    (60, "1 min"),
    (300, "5 min"),
    (900, "15 min"),
    (1800, "30 min"),
];

/// The width of each place's column of switches.
const COLUMN: f32 = 112.;

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let n = d.settings.notifications.clone();
    let column = |text: &'static str| {
        div()
            .w(px(COLUMN))
            .flex()
            .justify_center()
            .text_size(px(fs_sm()))
            .text_color(t.text_subtle)
            .child(text)
    };
    let header = div()
        .flex()
        .justify_end()
        // The rows' gap between their switches, so each label sits over its column.
        .gap(px(12.))
        .pb(px(4.))
        .border_b_1()
        .border_color(t.border)
        .child(column("Desktop"))
        .child(column("Chat apps"));
    let rows = EVENTS.iter().map(|&(kind, title)| {
        let desktop = n.desktop.allows(kind);
        let chat_apps = n.chat_apps.allows(kind);
        let cell = |el| div().w(px(COLUMN)).flex().justify_center().child(el);
        token_row(title, None, &t)
            .child(cell(
                switch(
                    format!("notify-desktop-{title}"),
                    format!("{title} on the desktop"),
                    desktop,
                    &t,
                )
                .on_click(cx.listener(move |d, _, _, cx| {
                    d.update(
                        |s| s.notifications.desktop = s.notifications.desktop.with(kind, !desktop),
                        cx,
                    );
                })),
            ))
            .child(cell(
                switch(
                    format!("notify-chat-apps-{title}"),
                    format!("{title} on chat apps"),
                    chat_apps,
                    &t,
                )
                .on_click(cx.listener(move |d, _, _, cx| {
                    d.update(
                        |s| {
                            s.notifications.chat_apps =
                                s.notifications.chat_apps.with(kind, !chat_apps);
                        },
                        cx,
                    );
                })),
            ))
            .into_any_element()
    });
    let events = div()
        .id("notify-events")
        .role(Role::Group)
        .aria_label("Notifications")
        .child(header)
        .children(rows);

    let when = token_row("Send to chat apps", None, &t).child(segmented(
        "notify-when",
        "Send to chat apps",
        n.chat_apps_when,
        &WHEN,
        &t,
        |d, when, cx| d.update(|s| s.notifications.chat_apps_when = when, cx),
        cx,
    ));
    let away = (n.chat_apps_when == ChatAppsWhen::Away).then(|| {
        token_row("Away after", None, &t).child(segmented(
            "notify-away",
            "Away after",
            n.away_after_secs,
            &AWAY_AFTER,
            &t,
            |d, secs, cx| d.update(|s| s.notifications.away_after_secs = secs, cx),
            cx,
        ))
    });
    let muted = n
        .threads
        .values()
        .filter(|own| **own == Notices::Off)
        .count()
        + n.muted_projects.len();
    let unmute = (muted > 0).then(|| {
        let label = format!("Unmute all ({muted})");
        token_row("Muted", None, &t).child(
            btn("notify-unmute", label.clone(), BtnKind::Plain, false, &t)
                .child(label)
                .on_click(cx.listener(|d, _, _, cx| {
                    d.update(
                        |s| {
                            let n = &mut s.notifications;
                            n.threads.retain(|_, own| *own != Notices::Off);
                            n.muted_projects.clear();
                        },
                        cx,
                    );
                })),
        )
    });
    div()
        .flex()
        .flex_col()
        .child(section_heading("Events", true, &t))
        .child(events)
        .child(section_heading("Chat apps", false, &t))
        .child(when)
        .children(away)
        .children(unmute)
        .into_any_element()
}
