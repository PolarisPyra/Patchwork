//! What an agent is told to write a color scheme, an animated background or a code theme from a
//! description.

// Rust guideline compliant 2026-10-09

use super::library::{CODE, SCENES, THEMES};
use super::types::Scheme;
use crate::Server;

/// A color scheme's file format, as the agent is told it.
const THEME_FORMAT: &str = r##"A color scheme is one JSON file:

{
  "name": "Night Reef",
  "scheme": "dark",            // "dark" or "light": which kind of window it is for
  "backdrop": "night-reef",    // optional: the animated background picked along with it, by
                               // its file's name without .json, or "butterfly" or "none"
  "colors": {                  // six-digit hex colors; the rest of the app is mixed from these
    "surface": "#0a1230",      // the window's background
    "text": "#e6ecff",         // the main text; it must read clearly on surface
    "muted": "#8f9bc4",        // the faint text; it must still be readable on surface
    "accent": "#3fd0c0"        // buttons, links and what is selected
  },
  "overrides": {               // optional: any of the colors below, in place of the mixed one
    "bgPanel": "#0e1838",      // the sidebar and the panels
    "border": "#1c2a55"
  }
}

"overrides" may name: bgPanel, bgEditor, bgInput, bgHover, bgSelected, bgSelectedMuted,
bgElevated (popups and dialogs), border, borderStrong, textSubtle (the faintest text),
textOnAccent, accentHover, accentText (links), success, danger, warning, bubble (the user's
message), and "syntax": an object with all of keyword, string, property, ty (type names),
number, comment, function and tag. Leave "overrides" out unless the description asks for
something the four colors cannot give.

Two more keys are optional, for a description that asks for them: "fonts", an object with "ui"
(the app's text) and "code" (code and terminals), each the name of a font installed on the
machine, and "radius", how round corners are, from 0 for square to 12 (the app's own is 6).

Rules:
- Real JSON: no comments, no trailing commas. The comments above only explain.
- Only these keys. Text on surface needs a contrast of 7:1 or more, muted 4.5:1 or more.
- The accent holds white or near-black text on buttons, so keep it away from mid-gray."##;

/// An animated background's file format, as the agent is told it.
const SCENE_FORMAT: &str = r##"An animated background is one JSON file, drawn in text characters:

{
  "name": "Night Reef",
  "scheme": "dark",                       // "dark" when the sky is dark, "light" when it is light
  "sky": ["#0a1230", "#061a3a"],          // one color, or two fading from the top to the bottom
  "dust": {                               // specks over the whole page; leave out for none
    "glyphs": ".o",                       // each speck is one of these characters
    "color": "#5fb8c8",
    "density": 0.008,                     // the share of cells with a speck, 0 to 0.2
    "twinkle": true,
    "drift": [0, -12]                     // pixels a second across and down; negative rises
  },
  "shootingStars": "#cfe8ff",             // leave out for none
  "sprites": [
    {
      "art": [                            // rows of characters, top first; a space is empty
        "   __   ",
        " _/##\\_ ",
        "<_####_>",
        "  \\__/  "
      ],
      "colors": { "#": "#5b46d6", "_": "#2fb7a6", "/": "#2fb7a6", "\\": "#2fb7a6", "<": "#2fb7a6", ">": "#2fb7a6" },
      "at": [0.82, 0.62],                 // its middle, as shares of the page's width and height
      "size": "large",                    // "large" is the size of code; "small" shades finer shapes
      "filled": true,                     // each character sits on a square of its color, as a tile
      "glow": true,                       // a soft light under it
      "motion": { "kind": "bob", "height": 10, "period": 6 }
    }
  ]
}

"motion" is one of:
  { "kind": "still" }
  { "kind": "bob", "height": 10, "period": 6 }      // up and down by pixels, once every so many seconds
  { "kind": "drift", "speed": [14, 0] }             // pixels a second, coming back in at the other side
  { "kind": "wander", "speed": 40 }                 // a slow loop around the page

Rules:
- Real JSON: no comments, no trailing commas. The comments above only explain.
- At most 24 sprites, each at most 80 rows of 160 characters. Three to eight sprites read best.
- The middle of the page holds a text field and a list, so keep sprites toward the edges and corners.
- A "filled" sprite is pixel art: draw it with a character for each tile, 10 to 30 tiles across.
- Colors of sprites and dust should sit well against the sky, and stay quiet enough to read text over."##;

/// A code theme's file format, as the agent is told it.
const CODE_FORMAT: &str = r##"A code theme is one JSON file, the colors code is highlighted in:

{
  "name": "GitHub Dark",
  "scheme": "dark",            // "dark" or "light": which kind of window it is for
  "colors": {                  // six-digit hex colors, all eight of them
    "keyword": "#ff7b72",      // fn, let, if, return
    "string": "#a5d6ff",
    "property": "#79c0ff",     // fields and keys
    "ty": "#ffa657",           // the names of types
    "number": "#79c0ff",
    "comment": "#8b949e",      // quieter than the rest, and still readable
    "function": "#d2a8ff",     // the names of functions
    "tag": "#7ee787"           // the tags of markup
  }
}

Rules:
- Real JSON: no comments, no trailing commas. The comments above only explain.
- Only these keys, and every one of the eight colors.
- Each color needs a contrast of 4.5:1 or more against the background code sits on, and
  keywords, strings and functions should be told apart at a glance."##;

impl Server {
    /// The message that has an agent write a color scheme of `description` into the themes
    /// folder, and the id the color scheme will have.
    #[must_use]
    pub fn theme_prompt(&self, description: &str) -> (String, String) {
        let id = THEMES.free_id(description);
        let path = THEMES.path(&id);
        make_dir(&THEMES.dir());
        let prompt = format!(
            "Write a Patchwork color scheme: {description}\n\nSave it as {} and write nothing \
             else. Read the file back and check it is valid JSON before you finish.\n\n\
             {THEME_FORMAT}",
            path.display()
        );
        (prompt, id)
    }

    /// The message that has an agent write an animated background of `description` into the
    /// scenes folder, and the id the background will have.
    #[must_use]
    pub fn scene_prompt(&self, description: &str) -> (String, String) {
        let id = SCENES.free_id(description);
        let path = SCENES.path(&id);
        make_dir(&SCENES.dir());
        let prompt = format!(
            "Write a Patchwork animated background: {description}\n\nSave it as {} and write \
             nothing else. Read the file back and check it is valid JSON before you finish.\n\n\
             {SCENE_FORMAT}",
            path.display()
        );
        (prompt, id)
    }
}

impl Server {
    /// The message that has an agent write a code theme of `description` for the windows of
    /// `scheme` into the code themes folder, and the id the code theme will have.
    #[must_use]
    pub fn code_theme_prompt(&self, description: &str, scheme: Scheme) -> (String, String) {
        let id = CODE.free_id(description);
        let path = CODE.path(&id);
        make_dir(&CODE.dir());
        let (kind, ground) = match scheme {
            Scheme::Light => ("light", "a near-white background"),
            Scheme::Dark => ("dark", "a near-black background"),
        };
        let prompt = format!(
            "Write a Patchwork code theme: {description}\n\nIt is for {kind} windows, where code \
             sits on {ground}, so its \"scheme\" is \"{kind}\". Save it as {} and write nothing \
             else. Read the file back and check it is valid JSON before you finish.\n\n\
             {CODE_FORMAT}",
            path.display()
        );
        (prompt, id)
    }
}

/// Makes the folder an agent is about to write a file into.
fn make_dir(dir: &std::path::Path) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!(error:% = e; "could not make {}: {{error}}", dir.display());
    }
}
