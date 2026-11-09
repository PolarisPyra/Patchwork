//! The form that adds one of Patchwork's MCP servers or changes it (`ServerForm.tsx`): a name, a
//! command line or URL, and its environment or headers. Pasted JSON imports every server in it.

use std::collections::BTreeMap;

use gpui::{AnyElement, Context, Focusable, FontWeight, Window, div, prelude::*, px};
use patchwork_server::provider::mcp::{self, McpServer};

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert};
use crate::ui::theme::{FS_SM, theme};
use crate::ui::{BtnKind, btn};

use super::{pairs_text, target};

/// Stands in for a stored value in the editor; saving it keeps the value.
pub const STORED: &str = "••••••";

/// The form's fields share this prefix, so closing it forgets them.
const PREFIX: &str = "mcp-form-";

/// A server being added (`previous` None) or changed, and the form's state.
#[derive(Default)]
pub struct Draft {
    previous: Option<String>,
    server: McpServer,
    name: String,
    line: String,
    pairs: String,
    busy: bool,
    error: Option<String>,
    confirm_remove: bool,
}

impl Draft {
    pub fn edit(previous: Option<String>, name: String, server: McpServer) -> Self {
        let line = if previous.is_some() {
            target(&server)
        } else {
            String::new()
        };
        let pairs = if server.url.is_some() {
            pairs_text(&server.headers, ": ")
        } else {
            pairs_text(&server.env, "=")
        };
        Self {
            previous,
            server,
            name,
            line,
            pairs,
            busy: false,
            error: None,
            confirm_remove: false,
        }
    }

    fn is_url(&self) -> bool {
        let line = self.line.trim().to_lowercase();
        line.starts_with("http://") || line.starts_with("https://")
    }

    fn is_json(&self) -> bool {
        self.line.trim().starts_with('{')
    }
}

/// Splits a command line into words, honoring quotes and backslashes.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word: Option<String> = None;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && quote != Some('\'') && chars.peek().is_some() {
            word.get_or_insert_default().extend(chars.next());
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.get_or_insert_default().push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            word.get_or_insert_default();
        } else if c.is_whitespace() {
            out.extend(word.take());
        } else {
            word.get_or_insert_default().push(c);
        }
    }
    out.extend(word);
    out
}

fn parse_pairs(text: &str, sep: char) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let at = line.find(sep).filter(|&at| at > 0)?;
            let value = line[at + 1..].trim();
            // An empty value keeps the stored one.
            let value = if value == STORED { "" } else { value };
            Some((line[..at].trim().to_string(), value.to_string()))
        })
        .collect()
}

/// The servers in pasted JSON: an `mcpServers` or `servers` map, a bare map of servers, or one
/// server, which takes `name`.
fn parse_servers(text: &str, name: &str) -> Result<Vec<(String, McpServer)>, String> {
    let json: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let obj = json.as_object().ok_or("That JSON is not an object")?;
    let map = obj
        .get("mcpServers")
        .or_else(|| obj.get("servers"))
        .unwrap_or(&json);
    let is_server = |v: &serde_json::Value| v.get("command").is_some() || v.get("url").is_some();
    let read = |v: &serde_json::Value| -> Result<McpServer, String> {
        serde_json::from_value(v.clone())
            .map(clean)
            .map_err(|e| e.to_string())
    };
    if is_server(map) {
        if name.trim().is_empty() {
            return Err("Give the server a name".into());
        }
        return Ok(vec![(name.trim().to_string(), read(map)?)]);
    }
    let entries: Vec<(String, McpServer)> = map
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| is_server(v))
        .map(|(n, v)| read(v).map(|s| (n.clone(), s)))
        .collect::<Result<_, _>>()?;
    if entries.is_empty() {
        return Err("No servers with a command or URL in that JSON".into());
    }
    Ok(entries)
}

/// Only the fields Patchwork reads; other clients add their own, such as `type`.
fn clean(s: McpServer) -> McpServer {
    if s.url.is_some() {
        McpServer {
            url: s.url,
            headers: s.headers,
            disabled: s.disabled,
            ..Default::default()
        }
    } else {
        McpServer {
            command: s.command,
            args: s.args,
            env: s.env,
            disabled: s.disabled,
            ..Default::default()
        }
    }
}

impl SettingsDialog {
    pub(super) fn open_mcp_form(
        &mut self,
        draft: Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus_name = draft.name.is_empty();
        self.drop_fields(PREFIX);
        self.mcp.editing = Some(draft);
        self.mcp.error = None;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        // The name takes focus while it is empty, as the web form's `autoFocus` does.
        let _ = render(self, window, cx);
        if focus_name && let Some(area) = self.field_area(&format!("{PREFIX}name")) {
            window.focus(&area.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn close_mcp_form(&mut self, cx: &mut Context<Self>) {
        self.mcp.editing = None;
        self.drop_fields(PREFIX);
        self.reload_mcp(cx);
        cx.notify();
    }

    fn save_mcp_form(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.mcp.editing.as_mut() else {
            return;
        };
        let (name, previous) = (draft.name.clone(), draft.previous.clone());
        let work: Result<Vec<(String, McpServer)>, String> = if draft.is_json() {
            parse_servers(&draft.line, &name)
        } else {
            let mut server = if draft.is_url() {
                McpServer {
                    url: Some(draft.line.trim().to_string()),
                    headers: parse_pairs(&draft.pairs, ':'),
                    ..Default::default()
                }
            } else {
                let mut words = words(&draft.line).into_iter();
                McpServer {
                    command: words.next(),
                    args: words.collect(),
                    env: parse_pairs(&draft.pairs, '='),
                    ..Default::default()
                }
            };
            server.disabled = draft.server.disabled;
            server.disabled_tools = draft.server.disabled_tools.clone();
            server.share = draft.server.share;
            Ok(vec![(name, server)])
        };
        let json = draft.is_json();
        draft.busy = true;
        draft.error = None;
        self.run(
            move || {
                for (i, (name, server)) in work?.into_iter().enumerate() {
                    // An import adds servers; only a single server renames the one it edits.
                    let previous = if json || i > 0 {
                        None
                    } else {
                        previous.clone()
                    };
                    mcp::save(name, previous, server)?;
                }
                Ok::<(), String>(())
            },
            |d, out, cx| match out {
                Ok(()) => d.close_mcp_form(cx),
                Err(e) => {
                    if let Some(draft) = &mut d.mcp.editing {
                        draft.busy = false;
                        draft.error = Some(e);
                    }
                }
            },
            cx,
        );
    }

    fn remove_mcp_server(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.mcp.editing.as_mut() else {
            return;
        };
        let name = draft.previous.clone().unwrap_or_else(|| draft.name.clone());
        draft.busy = true;
        self.run(
            move || mcp::remove(&name),
            |d, out, cx| match out {
                Ok(()) => d.close_mcp_form(cx),
                Err(e) => {
                    if let Some(draft) = &mut d.mcp.editing {
                        draft.busy = false;
                        draft.error = Some(e);
                    }
                }
            },
            cx,
        );
    }
}

/// A field with its label above it.
fn labeled(label: &'static str, field: gpui::Div, muted: gpui::Hsla) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(FS_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(label),
        )
        .child(field)
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let Some(draft) = d.mcp.editing.as_ref() else {
        return div().into_any_element();
    };
    let (name, line, pairs) = (draft.name.clone(), draft.line.clone(), draft.pairs.clone());
    let (is_url, is_json, busy, editing) = (
        draft.is_url(),
        draft.is_json(),
        draft.busy,
        draft.previous.is_some(),
    );
    let (error, confirm) = (draft.error.clone(), draft.confirm_remove);
    let live = FieldKind {
        live: true,
        ..Default::default()
    };
    let set = |f: fn(&mut Draft, String)| {
        move |d: &mut SettingsDialog, text: String, cx: &mut Context<SettingsDialog>| {
            if let Some(draft) = &mut d.mcp.editing {
                f(draft, text);
            }
            cx.notify();
        }
    };

    let name_field = d.field(
        format!("{PREFIX}name"),
        "Name",
        &name,
        "",
        live,
        set(|draft, text| draft.name = text),
        window,
        cx,
    );
    // Pasted JSON gets a taller field, which takes over the text and the focus.
    let (line_key, other_key) = if is_json {
        (format!("{PREFIX}json"), format!("{PREFIX}line"))
    } else {
        (format!("{PREFIX}line"), format!("{PREFIX}json"))
    };
    let switched = d
        .field_area(&other_key)
        .is_some_and(|a| a.read(cx).focus_handle(cx).is_focused(window));
    if switched {
        d.drop_fields(&line_key);
    }
    let line_field = d.field(
        line_key.clone(),
        if is_json { "JSON" } else { "Command or URL" },
        &line,
        if is_json {
            ""
        } else {
            r#"npx -y @playwright/mcp@latest, https://…, or {"mcpServers": …}"#
        },
        FieldKind {
            live: true,
            mono: true,
            lines: is_json,
            ..Default::default()
        },
        set(|draft, text| draft.line = text),
        window,
        cx,
    );
    if switched {
        d.drop_fields(&other_key);
        if let Some(area) = d.field_area(&line_key) {
            window.focus(&area.read(cx).focus_handle(cx), cx);
        }
    }
    let pairs_field = (!is_json).then(|| {
        d.field(
            format!("{PREFIX}pairs"),
            if is_url { "Headers" } else { "Environment" },
            &pairs,
            if is_url {
                "Authorization: Bearer …"
            } else {
                "API_KEY=…"
            },
            FieldKind {
                live: true,
                mono: true,
                lines: true,
                ..Default::default()
            },
            set(|draft, text| draft.pairs = text),
            window,
            cx,
        )
    });

    let can_save = !busy && !line.trim().is_empty() && (is_json || !name.trim().is_empty());
    let muted = t.text_muted;
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .py(px(10.))
        .child(labeled("Name", name_field, muted))
        .child(labeled(
            if is_json { "JSON" } else { "Command or URL" },
            if is_json {
                line_field.min_h(px(160.))
            } else {
                line_field
            },
            muted,
        ))
        .children(pairs_field.map(|f| {
            labeled(
                if is_url { "Headers" } else { "Environment" },
                f.min_h(px(80.)),
                muted,
            )
        }))
        .children(error.map(|e| alert("mcp-form-error", e, t.danger).pt_0()))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .border_t_1()
                .border_color(t.border)
                .pt(px(12.))
                .child(
                    btn("mcp-form-save", "Save", BtnKind::Primary, !can_save, &t)
                        .child(if busy {
                            "Saving…"
                        } else if is_json {
                            "Import"
                        } else {
                            "Save"
                        })
                        .when(can_save, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.save_mcp_form(cx)))
                        }),
                )
                .child(
                    btn("mcp-form-cancel", "Cancel", BtnKind::Plain, busy, &t)
                        .child("Cancel")
                        .when(!busy, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.close_mcp_form(cx)))
                        }),
                )
                .when(editing, |row| {
                    row.child(
                        btn(
                            "mcp-form-remove",
                            if confirm { "Remove server" } else { "Remove" },
                            BtnKind::Plain,
                            busy,
                            &t,
                        )
                        .ml_auto()
                        .when(confirm, |b| b.text_color(t.danger))
                        .child(if confirm { "Remove server" } else { "Remove" })
                        .when(!busy, |b| {
                            b.on_click(cx.listener(move |d, _, _, cx| {
                                if confirm {
                                    d.remove_mcp_server(cx);
                                } else if let Some(draft) = &mut d.mcp.editing {
                                    draft.confirm_remove = true;
                                    cx.notify();
                                }
                            }))
                        }),
                    )
                }),
        )
        .into_any_element()
}
