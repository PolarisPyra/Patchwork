//! What a capability is: one thing Patchwork can be asked to do from a chat app, with what it
//! takes, what it gives back, and what it needs before it runs.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use serde_json::Value;

use crate::threads::{AccessMode, Thread};
use crate::{Core, Error};

/// What a capability needs before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Permission {
    /// Reads and changes nothing, so it is never asked about.
    Read,
    /// Acts on chats as the person would from the chat list. The chats ask for themselves, by
    /// their own access.
    Act,
    /// Changes Patchwork's settings or files. Asked about in Supervised, and run in the other
    /// access modes.
    Change,
    /// Cannot be taken back, or widens what Patchwork may do. Asked about in every access mode.
    Confirm,
}

impl Permission {
    /// The name the registry lists it under.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Act => "act",
            Self::Change => "change",
            Self::Confirm => "confirm",
        }
    }
}

/// What an argument holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Text,
    Flag,
    /// Any JSON value, which the capability checks itself.
    Json,
}

/// One argument of a capability.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Arg {
    pub(super) name: &'static str,
    pub(super) kind: Kind,
    pub(super) required: bool,
    pub(super) about: &'static str,
}

/// A text argument that may be left out.
pub(super) const fn text(name: &'static str, about: &'static str) -> Arg {
    Arg {
        name,
        kind: Kind::Text,
        required: false,
        about,
    }
}

/// A text argument that must be given.
pub(super) const fn needed(name: &'static str, about: &'static str) -> Arg {
    Arg {
        name,
        kind: Kind::Text,
        required: true,
        about,
    }
}

/// An on or off argument that may be left out.
pub(super) const fn flag(name: &'static str, about: &'static str) -> Arg {
    Arg {
        name,
        kind: Kind::Flag,
        required: false,
        about,
    }
}

/// An argument of any JSON value, which must be given.
pub(super) const fn json(name: &'static str, about: &'static str) -> Arg {
    Arg {
        name,
        kind: Kind::Json,
        required: true,
        about,
    }
}

/// Who asked for a capability, and for how long it may run.
pub(crate) struct Ctx<'a> {
    pub(super) core: &'a Arc<Core>,
    /// The dispatcher's chat.
    pub(super) caller: &'a Thread,
    /// Whether the caller stopped waiting.
    pub(super) stopped: &'a dyn Fn() -> bool,
}

impl Ctx<'_> {
    /// Asks the person to allow what `title` and `detail` say, whatever the access mode, and
    /// waits for their answer.
    ///
    /// # Errors
    ///
    /// Returns an error when the person declines or the dispatcher's turn ends first.
    pub(super) fn confirm(&self, title: &str, detail: &str) -> Result<(), Error> {
        self.core
            .runner
            .confirm(self.core, &self.caller.id, title, detail)
    }

    /// Asks the person before a `Permission::Change` capability does something that widens what
    /// Patchwork may do, in the access modes where the registry did not ask already.
    ///
    /// # Errors
    ///
    /// Returns an error as `confirm` does.
    pub(super) fn escalate(&self, title: &str, detail: &str) -> Result<(), Error> {
        if self.caller.access_mode == AccessMode::Supervised {
            return Ok(());
        }
        self.confirm(title, detail)
    }
}

/// What runs a capability with its checked arguments, and returns what to tell the caller.
pub(crate) type Handler = fn(&Ctx<'_>, &Value) -> Result<String, Error>;

/// One thing Patchwork can be asked to do from a chat app.
#[derive(Clone, Copy)]
pub(crate) struct Capability {
    /// Its name, which never changes, and the name of its tool.
    pub(super) id: &'static str,
    /// The part of Patchwork it belongs to, such as `chats` or `themes`.
    pub(super) area: &'static str,
    /// What it does and when to use it, for the model.
    pub(super) about: &'static str,
    pub(super) args: &'static [Arg],
    /// What it gives back.
    pub(super) returns: &'static str,
    /// The kinds of thing it acts on, such as `chat` or `setting`.
    pub(super) resources: &'static [&'static str],
    pub(super) permission: Permission,
    /// Whether what it did can be taken back with another capability.
    pub(super) reversible: bool,
    /// Whether its work goes on after it returns.
    pub(super) asynchronous: bool,
    /// What the person is told about later, such as a chat finishing.
    pub(super) events: &'static [&'static str],
    pub(super) run: Handler,
}
