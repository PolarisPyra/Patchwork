//! A layout's shape: panes side by side or stacked, splits inside splits, each part with its
//! share of the split's room.

// Rust guideline compliant 2026-11-27

use super::PaneId;

/// The smallest share of a split a part can be dragged to.
const MIN_SHARE: f32 = 0.1;

/// Which way a split lays its parts out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Side by side.
    Row,
    /// Stacked.
    Column,
}

/// A pane, or a split of parts laid out along an axis.
#[derive(Clone, Debug)]
pub enum Node {
    Pane(PaneId),
    Split(Split),
}

/// Parts laid out along an axis, each with its share of the room.
#[derive(Clone, Debug)]
pub struct Split {
    pub axis: Axis,
    pub parts: Vec<Node>,
    /// Each part's share, in the order of `parts`, adding up to 1.
    pub sizes: Vec<f32>,
}

impl Split {
    /// `parts` along `axis`, sharing the room equally.
    #[must_use]
    pub fn even(axis: Axis, parts: Vec<Node>) -> Self {
        let share = 1. / parts.len().max(1) as f32;
        Self {
            axis,
            sizes: vec![share; parts.len()],
            parts,
        }
    }

    /// Scales the shares to add up to 1, and makes one per part.
    pub(super) fn normalize(&mut self) {
        self.sizes.resize(self.parts.len(), MIN_SHARE);
        let total: f32 = self.sizes.iter().sum();
        if total > 0. {
            for size in &mut self.sizes {
                *size /= total;
            }
        }
    }
}

impl Node {
    /// The first pane, in reading order.
    pub fn first(&self) -> PaneId {
        match self {
            Self::Pane(id) => *id,
            Self::Split(s) => s.parts.first().map_or(0, Self::first),
        }
    }

    /// Trades the places of panes `a` and `b`, each taking the other's room.
    pub fn swap_panes(&mut self, a: PaneId, b: PaneId) {
        match self {
            Self::Pane(id) if *id == a => *id = b,
            Self::Pane(id) if *id == b => *id = a,
            Self::Pane(_) => {}
            Self::Split(s) => s.parts.iter_mut().for_each(|p| p.swap_panes(a, b)),
        }
    }

    /// Every pane, in reading order.
    pub fn pane_ids(&self, out: &mut Vec<PaneId>) {
        match self {
            Self::Pane(id) => out.push(*id),
            Self::Split(s) => s.parts.iter().for_each(|p| p.pane_ids(out)),
        }
    }

    /// The pane at the top right, where the layout button sits.
    pub fn top_right(&self) -> PaneId {
        match self {
            Self::Pane(id) => *id,
            Self::Split(s) => {
                let part = match s.axis {
                    Axis::Row => s.parts.last(),
                    Axis::Column => s.parts.first(),
                };
                part.map_or(0, Self::top_right)
            }
        }
    }

    /// Puts pane `new` beside pane `target` along `axis`, before it or after. A split already
    /// along `axis` takes it as one more part, with half of `target`'s share; otherwise `target`
    /// becomes a split of the two.
    pub fn split(&mut self, target: PaneId, new: PaneId, axis: Axis, before: bool) -> bool {
        match self {
            Self::Pane(id) if *id == target => {
                let parts = if before {
                    vec![Self::Pane(new), Self::Pane(target)]
                } else {
                    vec![Self::Pane(target), Self::Pane(new)]
                };
                *self = Self::Split(Split::even(axis, parts));
                true
            }
            Self::Pane(_) => false,
            Self::Split(s) => {
                if s.axis == axis
                    && let Some(at) = s
                        .parts
                        .iter()
                        .position(|p| matches!(p, Self::Pane(id) if *id == target))
                {
                    let half = s.sizes.get(at).copied().unwrap_or(MIN_SHARE) / 2.;
                    if let Some(size) = s.sizes.get_mut(at) {
                        *size = half;
                    }
                    let to = if before { at } else { at + 1 };
                    s.parts.insert(to, Self::Pane(new));
                    s.sizes.insert(to, half);
                    s.normalize();
                    return true;
                }
                s.parts
                    .iter_mut()
                    .any(|p| p.split(target, new, axis, before))
            }
        }
    }

    /// Takes pane `id` out, its share going to the part before it, or after it when it was
    /// first. A split left with one part becomes that part, and one left with parts along its
    /// own axis takes them in with their shares.
    pub fn remove(&mut self, id: PaneId) {
        let Self::Split(s) = self else {
            return;
        };
        if let Some(at) = s
            .parts
            .iter()
            .position(|p| matches!(p, Self::Pane(p) if *p == id))
        {
            s.parts.remove(at);
            let share = if at < s.sizes.len() {
                s.sizes.remove(at)
            } else {
                0.
            };
            let heir = at.saturating_sub(1);
            if let Some(size) = s.sizes.get_mut(heir) {
                *size += share;
            }
        }
        for part in &mut s.parts {
            part.remove(id);
        }
        let axis = s.axis;
        let mut parts = Vec::with_capacity(s.parts.len());
        let mut sizes = Vec::with_capacity(s.parts.len());
        for (part, size) in s.parts.drain(..).zip(s.sizes.drain(..)) {
            match part {
                Self::Split(inner) if inner.axis == axis => {
                    parts.extend(inner.parts);
                    sizes.extend(inner.sizes.iter().map(|s| s * size));
                }
                other => {
                    parts.push(other);
                    sizes.push(size);
                }
            }
        }
        s.parts = parts;
        s.sizes = sizes;
        s.normalize();
        if s.parts.len() == 1 {
            *self = s.parts.remove(0);
        }
    }

    /// Where each pane lies in the layout, in shares of its width and height: `[left, top,
    /// right, bottom]`, the whole layout being `[0, 0, 1, 1]`.
    pub fn rects(&self, out: &mut Vec<(PaneId, [f32; 4])>) {
        self.rects_in([0., 0., 1., 1.], out);
    }

    fn rects_in(&self, area: [f32; 4], out: &mut Vec<(PaneId, [f32; 4])>) {
        let s = match self {
            Self::Pane(id) => return out.push((*id, area)),
            Self::Split(s) => s,
        };
        let [left, top, right, bottom] = area;
        let mut start = 0.;
        for (part, size) in s.parts.iter().zip(&s.sizes) {
            let end = start + size;
            let inner = match s.axis {
                Axis::Row => {
                    let w = right - left;
                    [left + w * start, top, left + w * end, bottom]
                }
                Axis::Column => {
                    let h = bottom - top;
                    [left, top + h * start, right, top + h * end]
                }
            };
            part.rects_in(inner, out);
            start = end;
        }
    }

    /// Gives its panes the numbers `ids` hands out, in reading order.
    pub fn renumber(&mut self, ids: &mut impl Iterator<Item = PaneId>) {
        match self {
            Self::Pane(id) => {
                if let Some(next) = ids.next() {
                    *id = next;
                }
            }
            Self::Split(s) => s.parts.iter_mut().for_each(|p| p.renumber(ids)),
        }
    }

    /// The split reached by taking part `path[0]`, then `path[1]`, and so on.
    pub fn split_at(&mut self, path: &[usize]) -> Option<&mut Split> {
        let Self::Split(s) = self else {
            return None;
        };
        match path.split_first() {
            None => Some(s),
            Some((at, rest)) => s.parts.get_mut(*at)?.split_at(rest),
        }
    }
}

impl Split {
    /// Moves the line after part `at` to `fraction` of the split's length, keeping each of the
    /// two parts beside it at least `min` of the length.
    pub fn drag_line(&mut self, at: usize, fraction: f32, min: f32) {
        if at + 1 >= self.sizes.len() {
            return;
        }
        let start: f32 = self.sizes[..at].iter().sum();
        let pair = self.sizes[at] + self.sizes[at + 1];
        let min = min.max(0.).min(pair / 2.);
        let first = (fraction - start).clamp(min, pair - min);
        self.sizes[at] = first;
        self.sizes[at + 1] = pair - first;
    }

    /// Moves the line after part `at` by `step` of the split's length, keeping each of the two
    /// parts beside it at least the smallest share.
    pub fn nudge_line(&mut self, at: usize, step: f32) {
        let end: f32 = self.sizes.iter().take(at + 1).sum();
        self.drag_line(at, end + step, MIN_SHARE);
    }

    /// Gives every part an equal share.
    pub fn equalize(&mut self) {
        *self = Self::even(self.axis, std::mem::take(&mut self.parts));
    }
}
