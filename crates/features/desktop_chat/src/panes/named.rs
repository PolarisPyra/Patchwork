//! Layouts the user named: a layout's shape and shares kept under a name, to lay any project's
//! panes out as again.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use gpui::Context;
use serde_json::{Value, json};

use super::Node;
use super::saved::{node_from, node_to};

/// Where the named layouts are kept in the app state.
const NAMED_KEY: &str = "chat_pane_named_layouts";

/// The window's named methods, for any [`ChatWindow`].
pub trait NamedExt: ChatWindow {
    /// Reads the named layouts kept at the last launch.
    fn restore_named_layouts(&mut self) {
        let Some(saved) = self.core.state(NAMED_KEY) else {
            return;
        };
        let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&saved) else {
            log::warn!("could not read the named pane layouts");
            return;
        };
        self.named_layouts = entries
            .iter()
            .filter_map(|e| Some((e["name"].as_str()?.to_owned(), node_from(&e["root"])?)))
            .collect();
    }

    /// Keeps the shown layout's shape as `name`, in place of one already called that.
    fn save_named_layout(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let root = self.shown_layout(cx).root.clone();
        match self.named_layouts.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = root,
            None => self.named_layouts.push((name.to_owned(), root)),
        }
        self.keep_named_layouts();
        cx.notify();
    }

    /// Forgets the layout named `name`.
    fn remove_named_layout(&mut self, name: &str, cx: &mut Context<Self>) {
        self.named_layouts.retain(|(n, _)| n != name);
        self.keep_named_layouts();
        cx.notify();
    }

    /// Lays the shown project's panes out as the layout named `name`.
    fn apply_named_layout(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(shape) = self
            .named_layouts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, root)| root.clone())
        else {
            return;
        };
        let mut ids = Vec::new();
        shape.pane_ids(&mut ids);
        self.apply_shape(
            ids.len(),
            move |kept| {
                let mut root: Node = shape;
                root.renumber(&mut kept.iter().copied());
                root
            },
            cx,
        );
    }

    fn keep_named_layouts(&self) {
        let saved: Vec<Value> = self
            .named_layouts
            .iter()
            .map(|(name, root)| json!({ "name": name, "root": node_to(root) }))
            .collect();
        if let Err(e) = self
            .core
            .set_state(NAMED_KEY, &Value::Array(saved).to_string())
        {
            log::warn!(error:% = e; "could not keep the named pane layouts: {{error}}");
        }
    }
}

impl<W: ChatWindow> NamedExt for W {}
