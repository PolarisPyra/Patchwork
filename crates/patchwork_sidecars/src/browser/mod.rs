//! Web pages inside Patchwork, drawn by the installed browser.
//!
//! The Chrome-family browser on the computer runs headless for each page and is driven over its
//! `DevTools` protocol: it streams the page as pictures the app shows, and takes the pointer and
//! keys the app sends it, the way the emulator's screen works.
//!
//! ```text
//! types    the commands, events and frames the app and a page exchange
//! chrome   finding the installed browser and starting it headless
//! page     one page over the DevTools protocol
//! keys     keys as the app names them, as the protocol's key events
//! session  a started page, its frames as they come, and commands sent to it
//! ```

// Rust guideline compliant 2026-10-09

mod chrome;
mod keys;
mod page;
mod session;
mod types;

pub use chrome::die_with_patchwork;
pub use chrome::{cool_browser, find_browser, warm_browser};
pub use page::address;
pub use session::{BrowserSender, BrowserSession};
pub use types::{
    BrowserCommand, BrowserEvent, BrowserFrame, BrowserMessage, INSTALL_URL, Modifiers,
    MouseAction, PointerButton,
};
