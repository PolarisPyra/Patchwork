//! Extensions → Hooks: bundles of hooks that run on turn events,
//! and the apps they ask for. A bundle is a folder with a `hooks.json`, or a `plugin.json` when it
//! carries more.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, FontWeight, SharedString, div, prelude::*, px};
use patchwork_server::hooks::{self, HookEvent, Bundle, Hook, HookHandler};

use crate::settings::SettingsDialog;
use crate::settings::controls::{switch, tildify, token_row};
use crate::ui::theme::{font_mono, fs_sm, fs_xs, radius, radius_sm, Theme, theme};
use crate::ui::{AGENT_KINDS, BtnKind, agent_label, btn, icon};

use super::{empty, example, list_box};

/// How a bundle is laid out: a folder with a `hooks.json`, whose commands get the event as JSON on
/// stdin. A tool hook refuses the call with exit code 2 and its reason on stderr.
const HOOKS_EXAMPLE: &str = "hooks/
└── guard/
    └── hooks.json

guard/hooks.json
{
  \"hooks\": {
    \"preToolUse\": [{
      \"matcher\": \"Bash\",
      \"hooks\": [{
        \"type\": \"command\",
        \"command\": \"grep -qv 'tail -f' || { echo 'tail -f never ends' >&2; exit 2; }\"
      }]
    }],
    \"stop\": [{
      \"hooks\": [{ \"type\": \"command\", \"command\": \"notify-send \\\"Turn $PATCHWORK_STATUS\\\"\" }]
    }]
  }
}";

/// The shell tool as each agent names it: Claude Code and Codex `Bash`, Patchwork's own agent
/// `bash`, and Antigravity the rest.
const SHELL_TOOLS: [&str; 5] = ["Bash", "bash", "run_command", "execute_command", "shell"];

/// Whether a matcher names only shell tools.
fn is_shell(matcher: Option<&str>) -> bool {
    matcher.is_some_and(|m| m.split('|').all(|tool| SHELL_TOOLS.contains(&tool)))
}

/// When a hook runs, in words.
fn when_text(event: HookEvent, shell: bool) -> String {
    let calls = if shell {
        "shell commands"
    } else {
        "tool calls"
    };
    match event {
        HookEvent::UserPromptSubmit => "When a prompt is sent".into(),
        HookEvent::SessionStart => "When a session starts".into(),
        HookEvent::PreToolUse => format!("Before {calls}"),
        HookEvent::PermissionRequest if shell => "When a shell command asks permission".into(),
        HookEvent::PermissionRequest => "When a tool asks permission".into(),
        HookEvent::PostToolUse => format!("After {calls}"),
        HookEvent::PreCompact => "Before compacting".into(),
        HookEvent::PostCompact => "After compacting".into(),
        HookEvent::SubagentStart => "When a subagent starts".into(),
        HookEvent::SubagentStop => "When a subagent stops".into(),
        HookEvent::Stop => "After each turn".into(),
        HookEvent::Interrupt => "When a turn is interrupted".into(),
        HookEvent::SessionEnd => "When a session ends".into(),
    }
}

/// The event as a manifest names it, such as `preToolUse`.
fn event_name(event: HookEvent) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// What a hook that runs no command does, in words.
fn handler_text(handler: &HookHandler) -> String {
    match handler {
        HookHandler::Command { command, .. } => command.clone(),
        HookHandler::McpTool { server, tool } => format!("{server} → {tool}"),
        HookHandler::Prompt => "A prompt hook".into(),
        HookHandler::Agent => "An agent hook".into(),
        HookHandler::Unsupported { kind } => format!("A {kind} hook"),
    }
}

impl SettingsDialog {
    fn list_bundles(&mut self, cx: &mut Context<Self>) {
        let settings = self.server.store.settings();
        self.run(
            move || hooks::list(&settings),
            |d, list, _| d.extensions.bundles = Some(list),
            cx,
        );
    }

    fn toggle_bundle(&mut self, id: String, on: bool, cx: &mut Context<Self>) {
        if let Some(bundle) = self
            .extensions
            .bundles
            .iter_mut()
            .flatten()
            .find(|p| p.id == id)
        {
            bundle.enabled = on;
        }
        self.update(
            |s| {
                s.disabled_hooks.retain(|p| *p != id);
                if !on {
                    s.disabled_hooks.push(id);
                }
            },
            cx,
        );
        self.list_bundles(cx);
    }
}

/// One line of a hook's details: a name, and what it is.
fn detail(name: &'static str, value: impl IntoElement, t: &Theme) -> Div {
    div()
        .flex()
        .gap(px(12.))
        .child(
            div()
                .flex_none()
                .w(px(64.))
                .text_color(t.text_subtle)
                .child(name),
        )
        .child(div().min_w_0().flex_1().child(value))
}

/// One hook: when it runs and what it does, opening to the event, matcher, and handler as written.
fn hook_row(
    bundle: &str,
    at: usize,
    hook: &Hook,
    open: bool,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let shell = is_shell(hook.matcher.as_deref());
    let skipped: Vec<&str> = AGENT_KINDS
        .into_iter()
        .filter(|kind| !hook.run_by.contains(kind))
        .map(agent_label)
        .collect();
    let when = when_text(hook.event, shell);
    let key = (bundle.to_string(), at);
    let hover = t.bg_selected_muted;
    let command = matches!(hook.handler, HookHandler::Command { .. });
    let summary = crate::ui::button(
        SharedString::from(format!("hook-{bundle}-{at}")),
        when.clone(),
    )
    .aria_expanded(open)
    .w_full()
    .flex_col()
    .items_start()
    .gap(px(4.))
    .px(px(10.))
    .py(px(8.))
    .hover(move |s| s.bg(hover))
    .child(
        div()
            .flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text)
                    .child(when),
            )
            .children(hook.matcher.clone().filter(|_| !shell).map(|m| {
                div()
                    .truncate()
                    .rounded(px(radius_sm()))
                    .bg(t.bg_selected_muted)
                    .px(px(6.))
                    .py(px(2.))
                    .font_family(font_mono())
                    .text_color(t.text_muted)
                    .child(m)
            }))
            .when(!skipped.is_empty(), |row| {
                row.child(div().ml_auto().flex_none().text_color(t.text_subtle).child(
                    if hook.run_by.is_empty() {
                        "Not run".to_string()
                    } else {
                        format!("Not run by {}", skipped.join(", "))
                    },
                ))
            })
            .child(
                div()
                    .flex_none()
                    .when(skipped.is_empty(), gpui::Styled::ml_auto)
                    .text_color(t.text_subtle)
                    .child(icon(if open { "caret-down" } else { "caret-right" }, 12.)),
            ),
    )
    .child(
        div()
            .w_full()
            .truncate()
            .text_color(t.text_muted)
            .when(command, |d| d.font_family(font_mono()))
            .child(handler_text(&hook.handler)),
    )
    .on_click(cx.listener(move |d, _, _, cx| {
        if !d.extensions.open_hooks.remove(&key) {
            d.extensions.open_hooks.insert(key.clone());
        }
        cx.notify();
    }));

    let details = open.then(|| {
        let mono = |text: String| div().font_family(font_mono()).child(text);
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(10.))
            .pb(px(10.))
            .text_color(t.text_muted)
            .child(detail("Event", mono(event_name(hook.event)), t))
            .children(hook.matcher.clone().map(|m| detail("Tools", mono(m), t)))
            .map(|d| match &hook.handler {
                HookHandler::Command {
                    command,
                    timeout_secs,
                } => d
                    .child(detail(
                        "Command",
                        div()
                            .rounded(px(radius_sm()))
                            .bg(t.bg_selected_muted)
                            .px(px(8.))
                            .py(px(6.))
                            .font_family(font_mono())
                            .text_color(t.text)
                            .child(command.clone()),
                        t,
                    ))
                    .child(detail("Timeout", format!("{timeout_secs}s"), t)),
                _ => d,
            })
    });
    div()
        .text_size(px(fs_xs()))
        .child(summary)
        .children(details)
        .into_any_element()
}

fn bundle_card(
    bundle: &Bundle,
    d: &SettingsDialog,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let hooks: Vec<AnyElement> = bundle
        .hooks
        .iter()
        .enumerate()
        .map(|(i, hook)| {
            let open = d.extensions.open_hooks.contains(&(bundle.id.clone(), i));
            hook_row(&bundle.id, i, hook, open, t, cx)
        })
        .collect();
    let (id, on) = (bundle.id.clone(), bundle.enabled);
    div()
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border)
        .p(px(12.))
        .when(!on, |d| d.opacity(0.6))
        .child(
            div()
                .flex()
                .min_w_0()
                .items_start()
                .gap(px(10.))
                .child(
                    div()
                        .mt(px(2.))
                        .flex_none()
                        .text_color(t.text_muted)
                        .child(icon("puzzle-piece", 18.)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .truncate()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(bundle.name.clone()),
                                )
                                .children(bundle.version.clone().map(|v| {
                                    div()
                                        .text_size(px(fs_xs()))
                                        .text_color(t.text_subtle)
                                        .child(v)
                                })),
                        )
                        .children(bundle.description.clone().map(|desc| {
                            div()
                                .mt(px(2.))
                                .text_size(px(fs_sm()))
                                .text_color(t.text_muted)
                                .child(desc)
                        }))
                        .when(!hooks.is_empty(), |d| {
                            d.child(
                                div()
                                    .mt(px(8.))
                                    .child(list_box(hooks, t).rounded(px(radius_sm()))),
                            )
                        })
                        .when(!bundle.apps.is_empty(), |d| {
                            d.child(div().mt(px(6.)).flex().flex_wrap().gap(px(4.)).children(
                                bundle.apps.iter().map(|app| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .rounded(px(radius_sm()))
                                        .bg(t.bg_selected_muted)
                                        .px(px(6.))
                                        .py(px(2.))
                                        .text_size(px(fs_xs()))
                                        .text_color(t.text_muted)
                                        .child(app.name.clone())
                                        .child(div().text_color(t.text_subtle).child(
                                            if app.required { "required" } else { "optional" },
                                        ))
                                }),
                            ))
                        }),
                )
                .child(
                    switch(
                        SharedString::from(format!("hooks-on-{}", bundle.id)),
                        format!("{} is on", bundle.name),
                        on,
                        t,
                    )
                    .on_click(cx.listener(move |d, _, _, cx| d.toggle_bundle(id.clone(), !on, cx))),
                ),
        )
        .into_any_element()
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    if !std::mem::replace(&mut d.extensions.asked_bundles, true) {
        d.list_bundles(cx);
    }
    let t = *theme(cx);
    let list = d.extensions.bundles.clone();
    let on: Vec<&Bundle> = list.iter().flatten().filter(|p| p.enabled).collect();
    let hooks: usize = on.iter().map(|p| p.hooks.len()).sum();
    let count = match &list {
        None => "Reading hooks…".to_string(),
        Some(_) => format!(
            "{} on · {hooks} {}",
            on.len(),
            if hooks == 1 { "hook" } else { "hooks" }
        ),
    };
    let dir = hooks::dir(&d.settings);
    let folder = tildify(&dir.to_string_lossy());
    let (open_top, open_row) = (dir.clone(), dir.clone());
    let tools = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .pb(px(12.))
        .child(
            div()
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(count),
        )
        .child(
            btn("hooks-refresh", "Refresh", BtnKind::Plain, false, &t)
                .ml_auto()
                .child("Refresh")
                .on_click(cx.listener(|d, _, _, cx| {
                    d.extensions.error = None;
                    d.list_bundles(cx);
                })),
        )
        .child(
            btn("hooks-open", "Open folder", BtnKind::Plain, false, &t)
                .px(px(10.))
                .child(icon("folder-simple", 13.))
                .child("Open folder")
                .on_click(cx.listener(move |d, _, _, cx| d.open_folder(open_top.clone(), cx))),
        );
    let body = match &list {
        Some(list) if !list.is_empty() => {
            let cards: Vec<AnyElement> = list.iter().map(|p| bundle_card(p, d, &t, cx)).collect();
            div().flex().flex_col().gap(px(8.)).children(cards)
        }
        Some(_) => empty(
            "No hooks yet. Drop a folder with a hooks.json in the hooks folder.",
            &t,
        ),
        None => div(),
    };
    div()
        .flex()
        .flex_col()
        .child(tools)
        .child(body)
        .child(
            div().mt(px(12.)).child(
                token_row("Folder", Some(&folder), &t).child(
                    btn("hooks-open-row", "Open", BtnKind::Plain, false, &t)
                        .child("Open")
                        .on_click(
                            cx.listener(move |d, _, _, cx| d.open_folder(open_row.clone(), cx)),
                        ),
                ),
            ),
        )
        .child(example(HOOKS_EXAMPLE, &t))
        .into_any_element()
}
