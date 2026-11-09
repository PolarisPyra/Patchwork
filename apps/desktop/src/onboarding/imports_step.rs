//! Setup's second step (`ImportsStep.tsx`): the conversations each agent saved on this computer,
//! and importing the ones picked.

use std::sync::Arc;

use gpui::{AnyElement, Context, Role, Window, div, prelude::*, px};
use patchwork_server::import::{ImportScan, ImportSource};

use super::{
    IMPORTS_STEP, ImportProgress, ImportResult, Onboarding, OnboardingEvent, choice_row, heading,
    key_id, row_end, tick,
};
use crate::settings::projects::{import_sources, source_agent, source_label};
use crate::theme::{FS_SM, Theme};
use crate::ui::{agent_logo, badge, spinner};

impl Onboarding {
    pub(super) fn sources(&self) -> Vec<ImportSource> {
        import_sources(self.agents.as_deref().unwrap_or_default())
    }

    pub(super) fn scanning(&self) -> bool {
        self.sources().iter().any(|k| self.scan_of(*k).is_none())
    }

    /// Scans each source's saved conversations again.
    pub(super) fn scan(&mut self, cx: &mut Context<Self>) {
        self.scan_run += 1;
        let run = self.scan_run;
        self.scans.clear();
        for kind in self.sources() {
            let server = Arc::clone(&self.server);
            let task = cx.background_spawn(async move { server.scan_imports(kind) });
            cx.spawn(async move |this, cx| {
                let scan = task.await;
                let _ = this.update(cx, |this, cx| {
                    if this.scan_run != run {
                        return;
                    }
                    this.scans.push((kind, Some(scan)));
                    if this.step == IMPORTS_STEP && !this.scanning() && this.import_kinds.is_none()
                    {
                        let kinds = this
                            .sources()
                            .into_iter()
                            .filter(|k| this.new_in(*k) > 0)
                            .collect();
                        this.import_kinds = Some(kinds);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// `kind`'s scan, once it answered.
    pub(super) fn scan_of(&self, kind: ImportSource) -> Option<&Option<ImportScan>> {
        self.scans
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, scan)| scan)
    }

    /// How many of `kind`'s conversations are not imported yet.
    pub(super) fn new_in(&self, kind: ImportSource) -> u32 {
        self.scan_of(kind)
            .and_then(|s| s.as_ref())
            .map_or(0, |s| s.new)
    }

    pub(super) fn toggle_import(&mut self, kind: ImportSource, cx: &mut Context<Self>) {
        let set = self.import_kinds.get_or_insert_default();
        if set.contains(&kind) {
            set.retain(|k| *k != kind);
        } else {
            set.push(kind);
        }
        cx.notify();
    }

    /// Imports the chosen providers' conversations, one provider at a time so the step can say
    /// which one is running, then moves on. A conversation two providers both hold is still
    /// imported once, since each run skips what the last one saved.
    pub(super) fn import_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kinds: Vec<ImportSource> = self
            .import_kinds
            .iter()
            .flatten()
            .copied()
            .filter(|k| self.new_in(*k) > 0)
            .collect();
        if kinds.is_empty() {
            return self.go_to(self.step + 1, window, cx);
        }
        self.saving = true;
        self.import_error = None;
        let projects_before = self.settings.projects.len();
        self.tally.get_or_insert((0, projects_before));
        let server = Arc::clone(&self.server);
        cx.spawn_in(window, async move |this, cx| {
            let total = kinds.len();
            for (index, kind) in kinds.into_iter().enumerate() {
                let _ = this.update(cx, |this, cx| {
                    this.importing = Some(ImportProgress { kind, index, total });
                    cx.notify();
                });
                let server = Arc::clone(&server);
                let out = cx
                    .background_spawn(async move { server.import_threads(&[kind]) })
                    .await;
                let failed = this.update(cx, |this, cx| match out {
                    Ok(count) => {
                        if let Some(tally) = &mut this.tally {
                            tally.0 += count;
                        }
                        false
                    }
                    Err(e) => {
                        this.import_error = Some(e);
                        this.importing = None;
                        this.saving = false;
                        cx.notify();
                        true
                    }
                });
                if failed.unwrap_or(true) {
                    return;
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                // Importing adds projects and threads on the server.
                this.settings = this.server.store.settings();
                cx.emit(OnboardingEvent::Changed);
                if let Some((threads, before)) = this.tally {
                    this.imported = Some(ImportResult {
                        threads,
                        projects: this.settings.projects.len().saturating_sub(before),
                    });
                }
                this.import_kinds = None;
                this.importing = None;
                this.saving = false;
                this.go_to(this.step + 1, window, cx);
            });
        })
        .detach();
        cx.notify();
    }
}

fn describe_scan(scan: Option<&Option<ImportScan>>) -> String {
    match scan {
        None => "Looking for saved conversations…".into(),
        Some(None) => "Couldn't read saved conversations".into(),
        Some(Some(s)) if s.found == 0 => "No saved conversations".into(),
        Some(Some(s)) if s.new == 0 => {
            format!("All {} conversations are already imported", s.found)
        }
        Some(Some(s)) => {
            let plural = if s.new == 1 {
                "conversation"
            } else {
                "conversations"
            };
            format!(
                "{} {plural} to import, grouped into projects by folder",
                s.new
            )
        }
    }
}

fn describe_progress(p: ImportProgress, scan: Option<&ImportScan>) -> String {
    let count = scan
        .filter(|s| s.new > 0)
        .map_or(String::new(), |s| format!("{} ", s.new));
    let of = if p.total > 1 {
        format!(" ({} of {})", p.index + 1, p.total)
    } else {
        String::new()
    };
    format!(
        "Importing {count}{} conversations{of}…",
        source_label(p.kind)
    )
}

pub fn imports_step(this: &Onboarding, t: &Theme, cx: &mut Context<Onboarding>) -> AnyElement {
    let selected = this.import_kinds.clone().unwrap_or_default();
    let rows = this.sources().into_iter().map(|kind| {
        let scan = this.scan_of(kind);
        let importable = this.new_in(kind) > 0;
        let checked = importable && selected.contains(&kind);
        let enabled = importable && this.importing.is_none();
        let mut end = Vec::new();
        if scan.is_none() {
            end.push(
                badge("", t)
                    .gap(px(4.))
                    .child(spinner(12., t.text_muted))
                    .child("Scanning…")
                    .into_any_element(),
            );
        }
        if importable {
            end.push(tick(checked, t));
        }
        let logo = match source_agent(kind) {
            Some(agent) => agent_logo(agent, None, 26.),
            None => div().into_any_element(),
        };
        let row = choice_row(
            key_id("setup-import", kind),
            source_label(kind),
            describe_scan(scan),
            logo,
            checked,
            enabled,
            t,
        )
        .child(row_end(end));
        if enabled {
            row.on_click(cx.listener(move |this, _, _, cx| this.toggle_import(kind, cx)))
        } else {
            row
        }
    });
    let progress = this.importing.map(|p| {
        let scan = this.scan_of(p.kind).and_then(|s| s.as_ref());
        (spinner(12., t.text_muted), describe_progress(p, scan))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(heading("Conversations found on this computer"))
        .child(
            div()
                .id("setup-sources")
                .role(Role::Group)
                .aria_label("Providers to import from")
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(rows),
        )
        // Always present, so screen readers announce each provider as it starts.
        .child(
            div()
                .id("setup-import-status")
                .role(Role::Status)
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .when_some(progress, |d, (spin, text)| d.child(spin).child(text)),
        )
        .when_some(this.import_error.clone(), |d, e| {
            d.child(
                div()
                    .id("setup-import-error")
                    .role(Role::Alert)
                    .text_size(px(FS_SM))
                    .text_color(t.danger)
                    .child(e),
            )
        })
        .into_any_element()
}
