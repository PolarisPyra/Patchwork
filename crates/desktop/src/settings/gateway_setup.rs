//! The steps that set up each chat app, with links to the pages they happen on.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, ClickEvent, ClipboardItem, Role, SharedString, Window, div, prelude::*, px};
use patchwork_core::gateway::Platform;

use crate::settings::SettingsDialog;
use crate::ui::theme::{font_mono, fs_sm, radius_sm, theme};
use crate::ui::{button, icon_button};

/// One step: what to do, a command to run, and the page to do it on.
struct Step {
    text: &'static str,
    command: Option<&'static str>,
    link: Option<(&'static str, &'static str)>,
}

const fn step(text: &'static str) -> Step {
    Step {
        text,
        command: None,
        link: None,
    }
}

const fn linked(text: &'static str, label: &'static str, url: &'static str) -> Step {
    Step {
        text,
        command: None,
        link: Some((label, url)),
    }
}

const fn run(text: &'static str, command: &'static str) -> Step {
    Step {
        text,
        command: Some(command),
        link: None,
    }
}

/// The step every chat app ends with.
const PAIR: Step = step("Turn it on below, then send the bot the pairing code to start chatting.");

fn steps(platform: Platform) -> &'static [Step] {
    match platform {
        Platform::Telegram => {
            const {
                &[
                    linked(
                        "Open BotFather in Telegram and send /newbot.",
                        "Open BotFather",
                        "https://t.me/BotFather",
                    ),
                    step("Pick a name and a username ending in \"bot\"."),
                    step("Copy the token BotFather sends and paste it into Bot token."),
                    PAIR,
                ]
            }
        }
        Platform::Discord => {
            const {
                &[
                    linked(
                        "Create an application in the Discord Developer Portal.",
                        "Open the Developer Portal",
                        "https://discord.com/developers/applications",
                    ),
                    step(
                        "Under Bot, turn on Message Content Intent, then press Reset Token and copy it.",
                    ),
                    step("Paste the token into Bot token."),
                    step(
                        "Under OAuth2, pick the bot scope with Send Messages and Read Message History, \
                 and open the link it makes to add the bot to a server. You can also message \
                 the bot directly.",
                    ),
                    PAIR,
                ]
            }
        }
        Platform::Slack => {
            const {
                &[
                    linked(
                        "Create an app from scratch on Slack's app page.",
                        "Open Slack apps",
                        "https://api.slack.com/apps",
                    ),
                    step(
                        "Under Socket Mode, turn it on and make an app token with connections:write. \
                 Paste it into App token.",
                    ),
                    step(
                        "Under OAuth & Permissions, add the bot scopes chat:write, im:history and \
                 files:read.",
                    ),
                    step("Under Event Subscriptions, subscribe to the bot event message.im."),
                    step("Under App Home, allow people to send messages from the Messages tab."),
                    step("Install the app, then paste its Bot User OAuth Token into Bot token."),
                    PAIR,
                ]
            }
        }
        Platform::Matrix => {
            const {
                &[
                    linked(
                        "Make an account for the bot, for example in Element.",
                        "Open Element",
                        "https://app.element.io",
                    ),
                    step(
                        "In Element, open Settings, then Help & About, and copy the access token. Paste \
                 it into Access token.",
                    ),
                    step("Enter the bot's homeserver, such as https://matrix.org."),
                    step(
                        "Start a chat with the bot with encryption turned off: encrypted rooms are not \
                 read.",
                    ),
                    PAIR,
                ]
            }
        }
        Platform::Mattermost => {
            const {
                &[
                    linked(
                        "Turn on bot accounts in the System Console, then add a bot under Integrations.",
                        "Read about bot accounts",
                        "https://developers.mattermost.com/integrate/reference/bot-accounts/",
                    ),
                    step("Copy the bot's token and paste it into Access token."),
                    step("Enter your Mattermost address, such as https://chat.example.com."),
                    PAIR,
                ]
            }
        }
        Platform::Signal => {
            const {
                &[
                    linked(
                        "Install signal-cli on this computer.",
                        "Download signal-cli",
                        "https://github.com/AsamK/signal-cli/releases",
                    ),
                    Step {
                        text: "Link it to the Signal account the bot answers as, and scan the code with \
                       that phone.",
                        command: Some("signal-cli link -n Patchwork"),
                        link: Some((
                            "Read about linking",
                            "https://github.com/AsamK/signal-cli/wiki/Linking-other-devices-%28Provisioning%29",
                        )),
                    },
                    run(
                        "Start its daemon and keep it running.",
                        "signal-cli -a +15551234567 daemon --http 127.0.0.1:8080",
                    ),
                    step("Enter the account's phone number below."),
                    PAIR,
                ]
            }
        }
        Platform::Qq => {
            const {
                &[
                    linked(
                        "Create a bot on the QQ bot platform.",
                        "Open the QQ bot platform",
                        "https://q.qq.com/",
                    ),
                    step(
                        "Under its development settings, add this computer's IP address to the allowlist.",
                    ),
                    step("Copy the AppID and AppSecret into the fields below."),
                    PAIR,
                ]
            }
        }
    }
}

/// `platform`'s setup steps as a numbered list.
pub(super) fn setup(platform: Platform, cx: &mut gpui::Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let id = platform.id();
    let items = steps(platform).iter().enumerate().map(|(i, step)| {
        let number = i + 1;
        let command = step.command.map(|command| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(radius_sm()))
                        .bg(t.bg_panel)
                        .font_family(font_mono())
                        .text_size(px(fs_sm()))
                        .child(command),
                )
                .child(
                    icon_button(
                        SharedString::from(format!("gw-{id}-copy-{number}")),
                        format!("Copy {command}"),
                        "copy",
                        14.,
                        cx,
                    )
                    .on_click(move |_: &ClickEvent, _: &mut Window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(command.to_string()));
                    }),
                )
        });
        let link = step.link.map(|(label, url)| {
            button(SharedString::from(format!("gw-{id}-link-{number}")), label)
                .role(Role::Link)
                .text_color(t.accent_text)
                .hover(gpui::Styled::underline)
                .child(label)
                .on_click(move |_, _, cx| cx.open_url(url))
        });
        div()
            .id(SharedString::from(format!("gw-{id}-step-{number}")))
            .role(Role::ListItem)
            .flex()
            .gap(px(10.))
            .py(px(6.))
            .child(
                div()
                    .flex_none()
                    .size(px(20.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(t.bg_selected)
                    .text_size(px(fs_sm()))
                    .child(number.to_string()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(6.))
                    .child(step.text)
                    .children(command)
                    .children(link),
            )
    });
    div()
        .id(SharedString::from(format!("gw-{id}-steps")))
        .role(Role::List)
        .aria_label(format!("Set up {}", platform.label()))
        .flex()
        .flex_col()
        .py(px(6.))
        .children(items)
        .into_any_element()
}
