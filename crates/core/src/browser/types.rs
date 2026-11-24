//! What the app and a browser session say to each other: the commands the app sends the page,
//! and the frames and events it gets back.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

/// Where to get a browser when none is installed.
pub const INSTALL_URL: &str = "https://www.google.com/chrome/";

/// What the app asks the page to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserCommand {
    /// Load an address.
    Open {
        url: String,
    },
    /// The page's size in logical pixels, and how many device pixels each one is.
    Resize {
        width: u32,
        height: u32,
        scale: f32,
    },
    /// The pointer moved, pressed or released over the page, or left it, at a point in logical
    /// pixels.
    Mouse {
        x: f32,
        y: f32,
        action: MouseAction,
        button: PointerButton,
        clicks: u32,
        modifiers: Modifiers,
    },
    /// The wheel or touchpad scrolled the page by `dx`, `dy` pixels over a point: positive
    /// scrolls right and down.
    Wheel {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        modifiers: Modifiers,
    },
    /// A key pressed: its name as the app's keyboard names it (`"enter"`, `"a"`, `"left"`), and
    /// the text it types, if any.
    Key {
        key: String,
        text: Option<String>,
        modifiers: Modifiers,
    },
    /// Text typed into the page at once, such as a paste from the app's clipboard.
    Text {
        text: String,
    },
    /// Whether the page's `prefers-color-scheme` is dark, as the app's own theme is.
    Theme {
        dark: bool,
    },
    /// Outlines the element under a point, in logical pixels, while one is being picked.
    Highlight {
        x: f32,
        y: f32,
    },
    /// Picks the element under a point, which comes back as [`BrowserEvent::Picked`], and
    /// clears the outline.
    Pick {
        x: f32,
        y: f32,
    },
    /// Clears the outline without picking anything.
    Unhighlight,
    Back,
    Forward,
    Reload,
    Stop,
}

/// What the pointer did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseAction {
    Move,
    Down,
    Up,
    Leave,
}

/// Which mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    #[default]
    Left,
    Middle,
    Right,
}

/// The modifier keys held during an input.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is a key held or not, as the keyboard reports them"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

/// What the session tells the app about the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserEvent {
    /// The page's address changed.
    Address { url: String },
    /// The page's title changed.
    Title { title: String },
    /// The page started or stopped loading, and whether there is history either way.
    Loading {
        loading: bool,
        back: bool,
        forward: bool,
    },
    /// The page's script threw, logged an error, or a file it loads failed.
    PageError { message: String },
    /// The element picked: a selector that finds it, its HTML, and the styles it is drawn with.
    Picked {
        selector: String,
        html: String,
        styles: String,
    },
}

/// A picture of the page, in device pixels, four bytes each in blue, green, red, alpha order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// One message from the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserMessage {
    Frame(BrowserFrame),
    Event(BrowserEvent),
}
