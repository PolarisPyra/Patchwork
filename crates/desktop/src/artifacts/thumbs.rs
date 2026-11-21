//! The pictures of artifacts' versions, shared by the gallery and the chats' artifact cards, and
//! taken one at a time off the UI thread the first time either asks.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::*;
use patchwork_core::Core;

use crate::ui::off_thread;

/// A version, by its artifact's id and its number.
type Key = (String, u32);

pub struct Thumbs {
    core: Arc<Core>,
    /// Each version's picture; None when it could not be taken.
    taken: HashMap<Key, Option<PathBuf>>,
    /// The versions asked for and not yet taken, the first asked first.
    waiting: Vec<Key>,
    /// Whether a picture is being taken.
    taking: bool,
}

impl Thumbs {
    pub fn new(core: Arc<Core>) -> Self {
        Self {
            core,
            taken: HashMap::new(),
            waiting: Vec::new(),
            taking: false,
        }
    }

    /// Version `n` of the artifact `id`'s picture: None while it is not taken yet, and Some(None)
    /// when it could not be.
    #[expect(
        clippy::option_option,
        reason = "not taken yet and could not be taken are drawn differently"
    )]
    pub fn get(&self, id: &str, n: u32) -> Option<Option<&PathBuf>> {
        self.taken.get(&(id.to_owned(), n)).map(Option::as_ref)
    }

    /// Asks for version `n` of the artifact `id`'s picture, unless it is taken or asked for.
    pub fn want(&mut self, id: &str, n: u32, cx: &mut Context<Self>) {
        let key = (id.to_owned(), n);
        if self.taken.contains_key(&key) || self.waiting.contains(&key) {
            return;
        }
        self.waiting.push(key);
        self.next(cx);
    }

    /// Takes the next picture asked for, when none is being taken.
    fn next(&mut self, cx: &mut Context<Self>) {
        if self.taking || self.waiting.is_empty() {
            return;
        }
        let key = self.waiting.remove(0);
        self.taking = true;
        let core = Arc::clone(&self.core);
        let (id, n) = key.clone();
        off_thread(
            cx,
            move || core.artifact_thumbnail(&id, n),
            move |this, taken, cx| {
                let path = taken
                    .inspect_err(
                        |e| log::warn!(error:% = e; "no picture of an artifact: {{error}}"),
                    )
                    .ok();
                this.taken.insert(key, path);
                this.taking = false;
                this.next(cx);
                cx.notify();
            },
        );
    }
}
