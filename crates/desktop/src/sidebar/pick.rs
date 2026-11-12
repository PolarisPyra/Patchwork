//! Picking several rows of the task tree: Ctrl+click adds or drops a row, Shift+click takes every
//! row from the last one clicked, and the right-click menu then acts on all of them.

// Rust guideline compliant 2026-10-09

use gpui::{Context, Modifiers};

use super::menu::Target;
use crate::workspace::Workspace;

#[derive(Default)]
pub struct Picking {
    /// The rows picked with Ctrl or Shift, in tree order for a Shift range.
    pub rows: Vec<Target>,
    /// The row last clicked, where a Shift range starts.
    cursor: Option<Target>,
    /// The tree's rows as last drawn, top to bottom, for a Shift range to run along.
    pub tree: Vec<Target>,
    /// The row last plainly clicked, which the Delete key acts on. Any other press lets it go.
    pub clicked: Option<Target>,
}

impl Workspace {
    /// Ctrl+click adds or drops `target` from the picked rows, and Shift+click picks the rows from
    /// the last one clicked; both return true. A plain click clears them and returns false, so the
    /// row acts as usual.
    pub fn pick_row(&mut self, target: &Target, m: Modifiers, cx: &mut Context<Self>) -> bool {
        let picking = &mut self.picking;
        let anchor = picking
            .cursor
            .as_ref()
            .and_then(|c| picking.tree.iter().position(|r| r == c));
        let at = picking.tree.iter().position(|r| r == target);
        picking.cursor = Some(target.clone());
        if m.shift
            && let (Some(anchor), Some(at)) = (anchor, at)
        {
            let (from, to) = if anchor < at {
                (anchor, at)
            } else {
                (at, anchor)
            };
            picking.rows = picking.tree[from..=to].to_vec();
            cx.notify();
            return true;
        }
        if !(m.control || m.platform) {
            picking.clicked = Some(target.clone());
            if !picking.rows.is_empty() {
                picking.rows.clear();
                cx.notify();
            }
            return false;
        }
        match picking.rows.iter().position(|r| r == target) {
            Some(i) => {
                picking.rows.remove(i);
            }
            None => picking.rows.push(target.clone()),
        }
        cx.notify();
        true
    }

    /// What a right-click on `target` acts on: every picked row when it is one of them, otherwise
    /// it alone, and the others are let go.
    pub fn menu_target(&mut self, target: Target) -> Target {
        if self.picking.rows.len() > 1 && self.picking.rows.contains(&target) {
            return Target::Many(self.picking.rows.clone());
        }
        self.picking.rows.clear();
        target
    }

    pub fn is_picked(&self, target: &Target) -> bool {
        self.picking.rows.contains(target)
    }
}
