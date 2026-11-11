//! Opening and closing a CLI's details and running its commands.

// Rust guideline compliant 2026-10-09

use super::read::run_command;
use super::read::read;
use super::read::SEARCH_FIELD;
use super::read::Details;
use gpui::{prelude::*, px};
use patchwork_server::provider::cli_info::InfoCommand;
use patchwork_server::provider::AgentKind;
use crate::settings::SettingsDialog;

impl SettingsDialog {
    pub(in crate::settings::providers) fn open_details(
        &mut self,
        kind: AgentKind,
        cx: &mut Context<Self>,
    ) {
        self.providers.details = Some(Details {
            kind,
            info: None,
            error: None,
            query: String::new(),
            filtered: false,
            output: None,
            busy: false,
        });
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.read_details(cx);
    }

    pub(super) fn close_details(&mut self, cx: &mut Context<Self>) {
        self.providers.details = None;
        self.drop_fields(SEARCH_FIELD);
        cx.notify();
    }

    pub(super) fn read_details(&mut self, cx: &mut Context<Self>) {
        let Some(details) = &mut self.providers.details else {
            return;
        };
        details.busy = true;
        details.error = None;
        let kind = details.kind;
        self.run(
            move || read(kind),
            move |d, info, _| {
                if let Some(details) = d.providers.details.as_mut().filter(|x| x.kind == kind) {
                    details.busy = false;
                    match info {
                        Ok(info) => details.info = Some(info),
                        Err(e) => details.error = Some(e.to_string()),
                    }
                }
            },
            cx,
        );
        cx.notify();
    }

    pub(super) fn run_details_command(
        &mut self,
        title: String,
        command: InfoCommand,
        cx: &mut Context<Self>,
    ) {
        let Some(details) = &mut self.providers.details else {
            return;
        };
        details.busy = true;
        details.error = None;
        details.output = None;
        let kind = details.kind;
        self.run(
            move || run_command(kind, command),
            move |d, out, _| {
                if let Some(details) = d.providers.details.as_mut().filter(|x| x.kind == kind) {
                    details.busy = false;
                    match out {
                        Ok(text) if text.trim().is_empty() => {
                            details.output = Some((title, "Nothing to report.".into()));
                        }
                        Ok(text) => details.output = Some((title, text)),
                        Err(e) => details.error = Some(e.to_string()),
                    }
                }
            },
            cx,
        );
        cx.notify();
    }
}
