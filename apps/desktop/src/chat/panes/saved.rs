//! Layouts kept across launches: each project's splits, their shares, each pane's tabs, and
//! which pane had the focus.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::tree::Split;
use super::{Axis, Layout, Node, Pane, PaneId};
use crate::workspace::Workspace;

/// Where the layouts are kept in the app state.
const LAYOUTS_KEY: &str = "chat_pane_layouts";

impl Workspace {
    /// Reads the layouts kept at the last launch, before any chat opens.
    pub(crate) fn restore_layouts(&mut self) {
        self.restore_named_layouts();
        let Some(saved) = self.core.state(LAYOUTS_KEY) else {
            return;
        };
        let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&saved) else {
            log::warn!("could not read the kept pane layouts");
            return;
        };
        for entry in &entries {
            let project = entry["project"].as_str().map(str::to_owned);
            if let Some(layout) = layout_from(entry) {
                let mut ids = Vec::new();
                layout.root.pane_ids(&mut ids);
                self.pane_seq = ids.into_iter().fold(self.pane_seq, PaneId::max);
                self.layouts.insert(project, layout);
            }
        }
        self.saved_layouts = saved;
    }

    /// Keeps the layouts when they changed since they were last kept, unless a line between
    /// panes is still being dragged.
    pub(crate) fn keep_layouts(&mut self, dragging: bool) {
        if dragging {
            return;
        }
        let mut entries: Vec<_> = self.layouts.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let saved: Vec<Value> = entries
            .into_iter()
            .map(|(project, layout)| layout_to(project.as_deref(), layout))
            .collect();
        let saved = Value::Array(saved).to_string();
        if saved == self.saved_layouts {
            return;
        }
        if let Err(e) = self.core.set_state(LAYOUTS_KEY, &saved) {
            log::warn!(error:% = e; "could not keep the pane layouts: {{error}}");
        }
        self.saved_layouts = saved;
    }
}

/// `layout` of `project`, as kept.
fn layout_to(project: Option<&str>, layout: &Layout) -> Value {
    let panes: Vec<Value> = layout
        .panes
        .iter()
        .map(|p| json!({ "id": p.id, "tabs": p.tabs, "shown": p.shown }))
        .collect();
    json!({
        "project": project,
        "root": node_to(&layout.root),
        "panes": panes,
        "focused": layout.focused,
    })
}

/// `node`, as kept.
pub(super) fn node_to(node: &Node) -> Value {
    match node {
        Node::Pane(id) => json!({ "pane": id }),
        Node::Split(s) => json!({
            "axis": if s.axis == Axis::Row { "row" } else { "column" },
            "parts": s.parts.iter().map(node_to).collect::<Vec<_>>(),
            "sizes": s.sizes,
        }),
    }
}

/// The layout kept in `entry`, when it reads whole and its panes match its splits.
fn layout_from(entry: &Value) -> Option<Layout> {
    let root = node_from(&entry["root"])?;
    let panes: Vec<Pane> = entry["panes"]
        .as_array()?
        .iter()
        .filter_map(|p| {
            Some(Pane {
                id: p["id"].as_u64()?,
                tabs: p["tabs"]
                    .as_array()?
                    .iter()
                    .filter_map(|t| t.as_str().map(str::to_owned))
                    .collect(),
                shown: p["shown"].as_str().map(str::to_owned),
                view: None,
            })
        })
        .collect();
    let mut ids = Vec::new();
    root.pane_ids(&mut ids);
    let matched =
        ids.len() == panes.len() && ids.iter().all(|id| panes.iter().any(|p| p.id == *id));
    if !matched {
        return None;
    }
    let focused = entry["focused"]
        .as_u64()
        .filter(|f| ids.contains(f))
        .unwrap_or_else(|| root.first());
    Some(Layout {
        root,
        panes,
        focused,
        maximized: None,
    })
}

/// The node kept in `value`, when it reads whole.
pub(super) fn node_from(value: &Value) -> Option<Node> {
    if let Some(id) = value["pane"].as_u64() {
        return Some(Node::Pane(id));
    }
    let axis = match value["axis"].as_str()? {
        "row" => Axis::Row,
        "column" => Axis::Column,
        _ => return None,
    };
    let parts: Vec<Node> = value["parts"]
        .as_array()?
        .iter()
        .map(node_from)
        .collect::<Option<_>>()?;
    if parts.len() < 2 {
        return None;
    }
    let sizes: Vec<f32> = value["sizes"]
        .as_array()
        .map(|s| {
            s.iter()
                .filter_map(Value::as_f64)
                .map(|s| s as f32)
                .collect()
        })
        .unwrap_or_default();
    let mut split = Split::even(axis, parts);
    if sizes.len() == split.parts.len() && sizes.iter().all(|s| *s > 0.) {
        split.sizes = sizes;
        split.normalize();
    }
    Some(Node::Split(split))
}
