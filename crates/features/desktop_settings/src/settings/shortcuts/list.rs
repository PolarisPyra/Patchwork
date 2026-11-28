//! The shortcuts pane: its state, and the pane drawn, with the search, the filters, the directory
//! of every command and key, and the inspector beside it.
//!
//! The inspector shows one thing at a time, the spot: a command, a fixed key, or a chord. Clicking
//! a row picks it; clicking one of a command's keys shows everything else on that key.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::Disableable as _;
use gpui::component::Sizable as _;
use std::collections::HashMap;

use gpui::{
    AnyElement, Entity, Focusable, FontWeight, Role, ScrollHandle, SharedString, Subscription,
    Window, div, prelude::*, px,
};
use patchwork_views_chat::sigils::SigilKind;
use patchwork_design_editor::{Arrow, Changed, Submitted, TextArea};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs_md, fs_sm, fs_xs, radius, radius_lg, radius_sm, theme};
use patchwork_design_components::focus_ring;
use patchwork_design_icons::icon;

use super::bindings::{
    COMMANDS, Command, FIXED_KEYS, Overrides, PAGES, STATE_KEY, bind_keys, clashes, command,
    format_key, keys_for, load, store_keys,
};
use super::chord::Chord;
use crate::settings::SettingsDialog;
use crate::settings::controls::{key_cap, tint};
use crate::settings::sigils::{self, SIGILS_PAGE};

/// The filter over the directory.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Chip {
    /// Every command and key.
    #[default]
    All,
    /// One page's.
    Page(&'static str),
    /// The sigils typed in the composer.
    Sigils,
    /// Commands whose keys the user changed.
    Changed,
    /// Commands with no key.
    Unassigned,
    /// Commands sharing a key with another.
    Conflicts,
}

/// What the inspector shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Spot {
    /// Nothing picked: the pane's tips, or what a search names.
    #[default]
    None,
    /// A command, by id.
    Command(&'static str),
    /// A fixed key, by its place in [`FIXED_KEYS`].
    Fixed(usize),
    /// One key, with everything on it.
    Chord(Chord),
}

/// The pane's state.
pub struct ShortcutsState {
    pub(super) chip: Chip,
    search: Option<Entity<TextArea>>,
    /// The search the spot was last reset for.
    last_query: String,
    pub(super) overrides: Option<Overrides>,
    /// The command whose next key pressed is taken as one of its keys.
    pub recording: Option<&'static str>,
    /// Which of its keys the key recorded replaces, or None to add one.
    pub(super) slot: Option<usize>,
    pub recording_sigil: Option<SigilKind>,
    /// Why the last key or sigil recorded was refused.
    pub refused: Option<String>,
    pub(super) spot: Spot,
    /// Which of the spotted command's keys is marked.
    pub(super) shown_binding: usize,
    list_scroll: ScrollHandle,
    /// The rows listed on the last draw, in order, for Up and Down in the search field.
    listed: Vec<Item>,
    /// Where each listed row sits among the list's children, past the page headings.
    listed_at: Vec<usize>,
    /// The watch on keys pressed, for recording, for as long as the dialog is open.
    pub(super) watches: Vec<Subscription>,
}

impl Default for ShortcutsState {
    fn default() -> Self {
        Self {
            chip: Chip::All,
            search: None,
            last_query: String::new(),
            overrides: None,
            recording: None,
            slot: None,
            recording_sigil: None,
            refused: None,
            spot: Spot::None,
            shown_binding: 0,
            list_scroll: ScrollHandle::new(),
            listed: Vec::new(),
            listed_at: Vec::new(),
            watches: Vec::new(),
        }
    }
}

/// A row of the directory: a command, or a key the editor or the mouse owns.
#[derive(Clone, Copy, Debug)]
pub(super) enum Item {
    Command(&'static Command),
    Fixed(usize),
}

impl Item {
    fn page(self) -> &'static str {
        match self {
            Self::Command(c) => c.page,
            Self::Fixed(at) => FIXED_KEYS[at].0,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Command(c) => c.title,
            Self::Fixed(at) => FIXED_KEYS[at].2,
        }
    }

    fn spot(self) -> Spot {
        match self {
            Self::Command(c) => Spot::Command(c.id),
            Self::Fixed(at) => Spot::Fixed(at),
        }
    }
}

/// Every command's keys, read once a frame: each key as stored and as a chord, and the commands
/// on each chord.
pub(super) struct Index {
    pub(super) overrides: Overrides,
    keys: HashMap<&'static str, Vec<(String, Option<Chord>)>>,
    on: HashMap<Chord, Vec<&'static Command>>,
}

impl Index {
    pub(super) fn new(overrides: Overrides) -> Self {
        let mut keys = HashMap::new();
        let mut on: HashMap<Chord, Vec<&'static Command>> = HashMap::new();
        for c in COMMANDS {
            let parsed: Vec<(String, Option<Chord>)> = keys_for(&overrides, c.id)
                .into_iter()
                .map(|k| {
                    let chord = Chord::from_stored(&k);
                    (k, chord)
                })
                .collect();
            for chord in parsed.iter().filter_map(|(_, chord)| *chord) {
                on.entry(chord).or_default().push(c);
            }
            keys.insert(c.id, parsed);
        }
        Self {
            overrides,
            keys,
            on,
        }
    }

    /// Command `id`'s keys, as stored, with their chords.
    pub(super) fn keys(&self, id: &str) -> &[(String, Option<Chord>)] {
        self.keys.get(id).map_or(&[], Vec::as_slice)
    }

    /// The commands on `chord`.
    pub(super) fn on(&self, chord: Chord) -> &[&'static Command] {
        self.on.get(&chord).map_or(&[], Vec::as_slice)
    }

    /// The other commands on command `id`'s keys, each with the key they share.
    pub(super) fn conflicts(&self, id: &str) -> Vec<(String, &'static Command)> {
        let mut found = Vec::new();
        for (key, chord) in self.keys(id) {
            let Some(chord) = chord else { continue };
            for other in self.on(*chord) {
                if clashes(id, other, key) {
                    found.push((key.clone(), *other));
                }
            }
        }
        found
    }

    /// A row's keys, each as the parts to show.
    pub(super) fn parts(&self, item: Item) -> Vec<Vec<String>> {
        match item {
            Item::Command(c) => self
                .keys(c.id)
                .iter()
                .map(|(key, chord)| chord.map_or_else(|| format_key(key), Chord::parts))
                .collect(),
            Item::Fixed(at) => vec![FIXED_KEYS[at].1.iter().map(|p| (*p).to_owned()).collect()],
        }
    }

    /// A row's keys as chords, for searches that name keys.
    pub(super) fn chords(&self, item: Item) -> Vec<Chord> {
        match item {
            Item::Command(c) => self.keys(c.id).iter().filter_map(|(_, c)| *c).collect(),
            Item::Fixed(at) => Chord::from_names(FIXED_KEYS[at].1),
        }
    }

    /// Whether `item` is on the chord a search names; a chord of modifiers alone takes in every
    /// key held with at least them.
    fn has_chord(&self, item: Item, query: Chord) -> bool {
        self.chords(item).iter().any(|c| match query.key {
            None => c.mods & query.mods == query.mods,
            Some(_) => *c == query,
        })
    }

    /// Whether the chip lets `item` through.
    fn passes(&self, item: Item, chip: Chip) -> bool {
        match (chip, item) {
            (Chip::All, _) => true,
            (Chip::Page(p), item) => item.page() == p,
            (Chip::Sigils, _) | (_, Item::Fixed(_)) => false,
            (Chip::Changed, Item::Command(c)) => self.overrides.contains_key(c.id),
            (Chip::Unassigned, Item::Command(c)) => !c.fixed && self.keys(c.id).is_empty(),
            (Chip::Conflicts, Item::Command(c)) => !self.conflicts(c.id).is_empty(),
        }
    }

    /// Every command whose keys can change, in the pages' order. Locked keys are left out, as
    /// there is nothing to do with them here.
    fn items() -> impl Iterator<Item = Item> {
        PAGES.iter().flat_map(|p| {
            COMMANDS
                .iter()
                .filter(move |c| c.page == *p && !c.fixed)
                .map(Item::Command)
        })
    }

    /// How many commands each filter holds: changed, unassigned, conflicting.
    fn counts(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for c in COMMANDS {
            let item = Item::Command(c);
            counts.0 += usize::from(self.passes(item, Chip::Changed));
            counts.1 += usize::from(self.passes(item, Chip::Unassigned));
            counts.2 += usize::from(self.passes(item, Chip::Conflicts));
        }
        counts
    }
}

impl SettingsDialog {
    pub(super) fn save_keys(&mut self, overrides: Overrides, cx: &mut Context<Self>) {
        let json = serde_json::to_string(&overrides).unwrap_or_default();
        if let Err(e) = self.core.set_state(STATE_KEY, &json) {
            self.error = Some(e.to_string());
        }
        self.shortcuts.overrides = Some(overrides);
        bind_keys(&self.core, cx);
        cx.notify();
    }

    /// Gives command `id` the keys `keys`.
    pub(super) fn set_keys(&mut self, id: &str, keys: &[String], cx: &mut Context<Self>) {
        let mut overrides = self.shortcuts.overrides.clone().unwrap_or_default();
        store_keys(&mut overrides, id, keys);
        self.save_keys(overrides, cx);
    }

    /// Shows command `id`, the way settings search opens it: the pane's own
    /// search and filter are cleared so its row is listed.
    pub(in crate::settings) fn show_command(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if let Some(search) = &self.shortcuts.search {
            search.update(cx, |area, cx| area.set_text("", cx));
        }
        self.shortcuts.last_query.clear();
        self.shortcuts.chip = Chip::All;
        self.show_spot(Spot::Command(id), cx);
    }

    /// Moves the pick `step` rows along the list, from the search field, and scrolls it into view.
    fn step_row(&mut self, step: isize, cx: &mut Context<Self>) {
        let s = &self.shortcuts;
        if s.listed.is_empty() {
            return;
        }
        let last = s.listed.len() as isize - 1;
        let to = match s.listed.iter().position(|i| i.spot() == s.spot) {
            Some(at) => (at as isize + step).clamp(0, last),
            None if step < 0 => last,
            None => 0,
        } as usize;
        let spot = s.listed[to].spot();
        if let Some(child) = s.listed_at.get(to) {
            s.list_scroll.scroll_to_item(*child);
        }
        self.show_spot(spot, cx);
    }

    /// Shows `spot` in the inspector, and stops any recording.
    pub(super) fn show_spot(&mut self, spot: Spot, cx: &mut Context<Self>) {
        let s = &mut self.shortcuts;
        if s.spot != spot {
            s.shown_binding = 0;
        }
        s.spot = spot;
        s.recording = None;
        s.recording_sigil = None;
        s.refused = None;
        cx.notify();
    }

    /// Starts recording a key for command `id`, to replace its key `slot`, or to add one.
    pub(super) fn record(
        &mut self,
        id: &'static str,
        slot: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = &mut self.shortcuts;
        s.spot = Spot::Command(id);
        s.recording = Some(id);
        s.slot = slot;
        s.recording_sigil = None;
        s.refused = None;
        // Keys pressed go nowhere else while recording, the search field included.
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

/// A key's parts as small key caps: "Ctrl" "Shift" "K".
pub fn kbd(parts: &[String], t: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(3.))
        .children(parts.iter().map(|p| key_cap(p.clone(), t)))
}

/// A command's keys side by side, an "or" between each, so a second key reads as another
/// way to the same command rather than one long chord.
fn keys_or(keys: &[Vec<String>], t: &Theme) -> Vec<AnyElement> {
    let mut out = Vec::with_capacity(keys.len() * 2);
    for (at, parts) in keys.iter().enumerate() {
        if at > 0 {
            out.push(
                div()
                    .text_size(px(fs_xs()))
                    .text_color(t.text_subtle)
                    .child("or")
                    .into_any_element(),
            );
        }
        out.push(kbd(parts, t).into_any_element());
    }
    out
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if d.shortcuts.overrides.is_none() {
        d.shortcuts.overrides = Some(load(&d.core));
    }
    let search = search_field(d, window, cx);
    let raw_query = search.read(cx).content.trim().to_owned();
    let query = raw_query.to_lowercase();
    if query != d.shortcuts.last_query {
        d.shortcuts.last_query.clone_from(&query);
        d.shortcuts.spot = Spot::None;
        d.shortcuts.shown_binding = 0;
    }
    let search_focused = search
        .read(cx)
        .focus_handle(cx)
        .contains_focused(window, cx);
    let index = Index::new(d.shortcuts.overrides.clone().unwrap_or_default());
    let chip = d.shortcuts.chip;
    let query_chord = Chord::from_query(&raw_query);
    let items: Vec<Item> = Index::items()
        .filter(|i| index.passes(*i, chip))
        .filter(|i| {
            query.is_empty()
                || query_chord.is_some_and(|q| index.has_chord(*i, q))
                || matches_text(&index, *i, &query)
        })
        .collect();

    let none_changed = index.overrides.is_empty() && !d.sigils(cx).changed();
    let counts = index.counts();
    let chips = chips(chip, counts, &t, cx);
    let directory = if chip == Chip::Sigils {
        d.shortcuts.listed.clear();
        d.shortcuts.listed_at.clear();
        sigils::rows(d, SIGILS_PAGE, &query, &t, cx)
    } else {
        let sigil_rows = if chip == Chip::All && !query.is_empty() {
            sigils::rows(d, "", &query, &t, cx)
        } else {
            Vec::new()
        };
        directory(d, &items, &index, sigil_rows, &t, cx)
    };
    let inspector = super::inspector::render(d, &index, &t, cx);
    let reset_all = Button::new("reset-all-shortcuts")
        .small()
        .disabled(none_changed)
        .accessibility_label("Reset all shortcuts")
        .h(px(FIELD_H))
        .child("Reset all")
        .when(!none_changed, |b| {
            b.on_click(cx.listener(|d, _, _, cx| {
                d.show_spot(Spot::None, cx);
                d.save_keys(Overrides::new(), cx);
                d.save_sigils(patchwork_views_chat::sigils::Sigils::default(), cx);
            }))
        });

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(FIELD_H))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(radius()))
                        .border_1()
                        .border_color(if search_focused {
                            t.border_strong
                        } else {
                            t.border
                        })
                        .bg(t.bg_input)
                        .px(px(10.))
                        .text_size(px(fs_md()))
                        .child(
                            div()
                                .flex_none()
                                .text_color(t.text_subtle)
                                .child(icon(IconName::MagnifyingGlass, 13.)),
                        )
                        .child(div().flex_1().min_w_0().child(search)),
                )
                .child(reset_all),
        )
        .child(chips)
        .child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .gap(px(12.))
                .child(
                    div()
                        .id("shortcut-directory")
                        .role(Role::List)
                        .aria_label("Shortcuts")
                        .flex_1()
                        .min_w_0()
                        .overflow_y_scroll()
                        .track_scroll(&d.shortcuts.list_scroll)
                        .pr(px(4.))
                        .children(directory),
                )
                .when_some(inspector, |row, inspector| {
                    row.child(
                        div()
                            .id("shortcut-inspector")
                            .flex_none()
                            .w(px(300.))
                            .overflow_y_scroll()
                            .rounded(px(radius()))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.bg_panel)
                            .p(px(12.))
                            .child(inspector),
                    )
                }),
        )
        .into_any_element()
}

/// The search field, made on the pane's first draw.
fn search_field(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Entity<TextArea> {
    if let Some(search) = &d.shortcuts.search {
        return search.clone();
    }
    let search = cx.new(|cx| {
        let mut area = TextArea::new("Search", "Search shortcuts", cx);
        area.single_line = true;
        area.placeholder_color = Some(patchwork_design_theme::input_placeholder());
        area
    });
    cx.subscribe(&search, |_, _, _: &Changed, cx| cx.notify())
        .detach();
    cx.subscribe(&search, |d, _, step: &Arrow, cx| d.step_row(step.0, cx))
        .detach();
    cx.subscribe_in(&search, window, |d, _, _: &Submitted, window, cx| {
        // Enter adds a key to the command picked.
        if let Spot::Command(id) = d.shortcuts.spot
            && command(id).is_some_and(|c| !c.fixed)
        {
            d.record(id, None, window, cx);
        }
    })
    .detach();
    d.shortcuts.search = Some(search.clone());
    search
}

/// Whether a row's words take in `query`, already lowercase.
fn matches_text(index: &Index, item: Item, query: &str) -> bool {
    let desc = match item {
        Item::Command(c) => c.desc.unwrap_or_default(),
        Item::Fixed(_) => "",
    };
    let keys = index
        .parts(item)
        .iter()
        .map(|p| p.join(" "))
        .collect::<Vec<_>>()
        .join(" ");
    [item.title(), desc, item.page(), &keys]
        .iter()
        .any(|x| x.to_lowercase().contains(query))
}

/// The filter chips, with how many each holds where it helps.
fn chips(
    chip: Chip,
    (changed, unassigned, conflicts): (usize, usize, usize),
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);
    let mut all: Vec<(Chip, &'static str, Option<usize>)> = vec![(Chip::All, "All", None)];
    all.extend(
        PAGES
            .iter()
            .filter(|p| COMMANDS.iter().any(|c| c.page == **p && !c.fixed))
            .map(|p| (Chip::Page(p), *p, None)),
    );
    all.push((Chip::Sigils, SIGILS_PAGE, None));
    let filters = [
        (Chip::Changed, "Changed", Some(changed)),
        (Chip::Unassigned, "Unassigned", Some(unassigned)),
        (Chip::Conflicts, "Conflicts", Some(conflicts)),
    ];
    let one = |(c, label, count): (Chip, &'static str, Option<usize>), at: usize| {
        let selected = c == chip;
        let warn = c == Chip::Conflicts && count.is_some_and(|n| n > 0);
        div()
            .id(("shortcut-chip", at))
            .role(Role::Tab)
            .aria_label(label)
            .aria_selected(selected)
            .tab_index(0)
            .focus_visible(focus_ring)
            .cursor_pointer()
            .flex()
            .flex_none()
            .h(px(28.))
            .items_center()
            .gap(px(6.))
            .rounded_full()
            .px(px(10.))
            .text_size(px(fs_sm()))
            .border_1()
            .map(|d| {
                if selected {
                    d.bg(t.bg_selected)
                        .border_color(t.border_strong)
                        .text_color(fg)
                } else {
                    d.border_color(t.border)
                        .text_color(muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                }
            })
            .child(label)
            .children(count.map(|n| {
                div()
                    .text_size(px(fs_xs()))
                    .text_color(if warn { t.warning } else { t.text_subtle })
                    .child(n.to_string())
            }))
            .on_click(cx.listener(move |d, _, _, cx| {
                d.shortcuts.chip = c;
                d.show_spot(Spot::None, cx);
                d.shortcuts
                    .list_scroll
                    .set_offset(gpui::point(px(0.), px(0.)));
            }))
    };
    let pages: Vec<_> = all
        .into_iter()
        .enumerate()
        .map(|(at, c)| one(c, at))
        .collect();
    let filters: Vec<_> = filters
        .into_iter()
        .enumerate()
        .map(|(at, c)| one(c, at + 100))
        .collect();
    div()
        .id("shortcut-chips")
        .role(Role::TabList)
        .aria_label("Shortcut pages and filters")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .overflow_x_scroll()
        .children(pages)
        .child(div().w(px(1.)).h(px(16.)).mx(px(4.)).bg(t.border))
        .children(filters)
        .into_any_element()
}

/// The directory: a row for each item, under its page's heading when more than one page shows.
fn directory(
    d: &mut SettingsDialog,
    items: &[Item],
    index: &Index,
    sigil_rows: Vec<AnyElement>,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let headed = !matches!(d.shortcuts.chip, Chip::Page(_));
    let mut out: Vec<AnyElement> = Vec::with_capacity(items.len() + PAGES.len());
    let mut listed_at = Vec::with_capacity(items.len());
    let mut page = "";
    for (at, item) in items.iter().enumerate() {
        // Each page's rows read as one card: the first rounds its top, the last its bottom.
        let first = at == 0 || (headed && item.page() != page);
        let last = items
            .get(at + 1)
            .is_none_or(|next| headed && next.page() != item.page());
        if headed && item.page() != page {
            page = item.page();
            out.push(heading(page, out.is_empty(), t));
        }
        listed_at.push(out.len());
        out.push(row(d, *item, at, (first, last), index, t, cx));
    }
    d.shortcuts.listed = items.to_vec();
    d.shortcuts.listed_at = listed_at;
    let empty = out.is_empty() && sigil_rows.is_empty();
    if !sigil_rows.is_empty() {
        out.push(heading(SIGILS_PAGE, out.is_empty(), t));
        out.extend(sigil_rows);
    }
    let message = match d.shortcuts.chip {
        Chip::Changed => "No shortcut has been changed.",
        Chip::Unassigned => "Every command has a key.",
        Chip::Conflicts => "No two commands share a key.",
        _ => "No shortcuts match.",
    };
    if empty {
        out.push(crate::settings::controls::empty(message, t).into_any_element());
    }
    out
}

/// A page's heading over its card of rows, drawn like a settings section's.
fn heading(page: &'static str, first: bool, t: &Theme) -> AnyElement {
    div()
        .mt(px(if first { 0. } else { GROUP_GAP }))
        .mb(px(8.))
        .text_size(px(fs_md()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text)
        .child(page)
        .into_any_element()
}

/// The space between one page's card of shortcuts and the next page's heading.
const GROUP_GAP: f32 = 24.;

/// A shortcut row's height: one line, its keys beside it.
const ROW_H: f32 = 40.;

/// The height of the search field and the Reset all button beside it.
const FIELD_H: f32 = 32.;

/// One row: the command's name, and its keys.
fn row(
    d: &SettingsDialog,
    item: Item,
    at: usize,
    (first, last): (bool, bool),
    index: &Index,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let spot = item.spot();
    let selected = d.shortcuts.spot == spot;
    let (fixed, clash) = match item {
        Item::Command(c) => (c.fixed, !index.conflicts(c.id).is_empty()),
        Item::Fixed(_) => (true, false),
    };
    let changed = matches!(item, Item::Command(c) if index.overrides.contains_key(c.id));
    let parts = index.parts(item);
    let recording = matches!(item, Item::Command(c) if d.shortcuts.recording == Some(c.id));
    let hover = t.bg_hover;
    div()
        .id(("shortcut-row", at))
        .role(Role::ListItem)
        .aria_label(SharedString::from(item.title()))
        .aria_selected(selected)
        .tab_index(0)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .relative()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(ROW_H))
        .px(px(16.))
        .border_x_1()
        .border_t_1()
        .border_color(t.border)
        .when(first, |r| r.rounded_t(px(radius_lg())))
        .when(last, |r| r.border_b_1().rounded_b(px(radius_lg())))
        .map(|r| {
            if selected {
                r.bg(t.bg_selected)
            } else {
                r.bg(tint(t)).hover(move |s| s.bg(hover))
            }
        })
        .when(selected, |r| {
            r.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(6.))
                    .bottom(px(6.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(t.accent),
            )
        })
        .on_click(cx.listener(move |d, _, _, cx| d.show_spot(spot, cx)))
        .child(
            div().flex_1().min_w_0().child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_color(if fixed { t.text_muted } else { t.text })
                    .child(div().truncate().child(item.title()))
                    .when(fixed, |r| {
                        r.child(
                            div()
                                .id(("shortcut-lock", at))
                                .flex_none()
                                .flex()
                                .items_center()
                                .text_color(t.text_subtle)
                                .child(icon(IconName::LockSimple, 11.)),
                        )
                    })
                    .when(changed, |r| {
                        r.child(
                            div()
                                .id(("shortcut-changed", at))
                                .flex_none()
                                .size(px(5.))
                                .rounded_full()
                                .bg(t.accent),
                        )
                    }),
            ),
        )
        .when(clash, |r| {
            r.child(
                div()
                    .id(("shortcut-clash", at))
                    .flex_none()
                    .flex()
                    .items_center()
                    .text_color(t.warning)
                    .child(icon(IconName::Warning, 13.)),
            )
        })
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .map(|r| {
                    if recording {
                        r.child(
                            div()
                                .px(px(6.))
                                .py(px(2.))
                                .rounded(px(radius_sm()))
                                .border_1()
                                .border_color(t.accent)
                                .bg(t.bg_accent_soft)
                                .text_size(px(fs_sm()))
                                .text_color(t.accent_text)
                                .child("Press keys…"),
                        )
                    } else if parts.is_empty() {
                        r.child(
                            div()
                                .text_size(px(fs_sm()))
                                .text_color(t.text_subtle)
                                .child("Unassigned"),
                        )
                    } else {
                        r.children(keys_or(&parts, t))
                    }
                }),
        )
        .into_any_element()
}
