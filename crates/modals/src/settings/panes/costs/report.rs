//! What the Costs pane holds, and loading and exporting its report.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::Context;
use patchwork_core::costs::{CostFilter, CostRange, CostReport};

use crate::settings::SettingsDialog;

/// What the overview and the charts measure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum CostMetric {
    #[default]
    Cost,
    Tokens,
}

/// The latest report, the range it covers, and the chart bar picked.
#[derive(Default)]
pub struct CostsState {
    pub(super) report: Option<CostReport>,
    /// Set once a report has been asked for since the pane opened or the replies changed.
    pub(crate) asked: bool,
    pub(super) range: CostRange,
    pub(super) metric: CostMetric,
    /// The provider, model or project the page is narrowed to.
    pub(super) filter: Option<CostFilter>,
    /// The bar under the pointer or picked with the arrow keys, whose figures the chart shows.
    pub(super) bar: Option<usize>,
}

impl SettingsDialog {
    /// Makes a new report off the UI thread when none was asked for since the last change. The
    /// last one stays up until it is ready.
    pub(super) fn load_costs(&mut self, cx: &mut Context<Self>) {
        if self.costs.asked {
            return;
        }
        self.costs.asked = true;
        let core = Arc::clone(&self.core);
        let range = self.costs.range;
        let filter = self.costs.filter.clone();
        self.run(
            move || core.cost_report(range, filter),
            |d, report, _| {
                // A report for a range or filter since left is dropped; the newer one is on its
                // way.
                if report.range == d.costs.range && report.filter == d.costs.filter {
                    d.costs.report = Some(report);
                }
            },
            cx,
        );
    }

    pub(super) fn set_cost_range(&mut self, range: CostRange, cx: &mut Context<Self>) {
        if self.costs.range == range {
            return;
        }
        self.costs.range = range;
        self.costs.bar = None;
        self.costs.asked = false;
        cx.notify();
    }

    pub(super) fn set_cost_metric(&mut self, metric: CostMetric, cx: &mut Context<Self>) {
        self.costs.metric = metric;
        cx.notify();
    }

    /// Narrows the page to `filter`, or back to every reply when it is the one already set.
    pub(super) fn toggle_cost_filter(
        &mut self,
        filter: Option<CostFilter>,
        cx: &mut Context<Self>,
    ) {
        self.costs.filter = if self.costs.filter == filter {
            None
        } else {
            filter
        };
        self.costs.bar = None;
        self.costs.asked = false;
        cx.notify();
    }

    /// Adds the costs up again, as a reply finished.
    pub fn refresh_costs(&mut self, cx: &mut Context<Self>) {
        self.costs.asked = false;
        if self.section == "costs" {
            cx.notify();
        }
    }

    /// Asks where to save the report as CSV, and writes it there.
    pub(super) fn export_costs(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.costs.report.as_ref() else {
            return;
        };
        let text = report.csv();
        let name = format!(
            "patchwork-costs-{}.csv",
            report.range.label().to_ascii_lowercase().replace(' ', "-")
        );
        let home = patchwork_core::system::files::paths::home_dir().unwrap_or_default();
        let picked = cx.prompt_for_new_path(&home, Some(&name));
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let path = path.to_string_lossy().into_owned();
            let saved = cx
                .background_executor()
                .spawn(async move { core.save_text_file(&path, &text) })
                .await;
            if let Err(e) = saved {
                let _ = this.update(cx, |d, cx| {
                    d.error = Some(e.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
