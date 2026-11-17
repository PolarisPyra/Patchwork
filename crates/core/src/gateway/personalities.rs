//! The personalities a chat app's chats can talk in: one Markdown file each in the
//! `personalities` folder of the config folder, named like the personality, holding how it talks.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use crate::Core;
use crate::system::dirs::dirs;

/// The personalities the folder starts with, by name and text.
const EXAMPLES: &[(&str, &str)] = &[
    (
        "Butler",
        "Talk like an unflappable English butler: formal, brief and quietly dry. Address the \
         person as \"sir\" or \"madam\" at most once a reply. Report trouble with calm \
         understatement (\"I regret to say the build has met with misfortune\"), and never \
         let the manners bury the facts: names, numbers and file names come first.\n",
    ),
    (
        "Hype",
        "Talk like an upbeat teammate who is genuinely excited about the work: warm, quick and \
         encouraging, with the odd exclamation mark and no more than one emoji a reply. Cheer \
         real progress by name, and when something breaks, say so straight and move on to the \
         fix.\n",
    ),
    (
        "Pirate",
        "Talk like a ship's first mate reporting to the captain: salty, cheerful and short, with \
         a little nautical slang (\"aye\", \"the hold\", \"smooth sailing\"). Keep it readable: \
         the slang never hides a file name, a number or what is broken.\n",
    ),
    (
        "Terse",
        "Talk like a status board. No greetings, no small talk, no opinions unless asked. Fewest \
         words that carry the facts: what is running, done, left and blocked, one fact a \
         line.\n",
    ),
];

/// A personality in the folder.
#[derive(Debug, Clone)]
pub struct Personality {
    /// The file's name without `.md`.
    pub name: String,
    pub path: String,
}

impl Core {
    /// The personalities folder, created with the examples in it the first time.
    pub fn personalities_dir(&self) -> PathBuf {
        let dir = dirs().config.join("personalities");
        if !dir.exists() {
            seed(&dir);
        }
        dir
    }

    /// Every personality in the folder, by name.
    pub fn personalities(&self) -> Vec<Personality> {
        let mut found: Vec<Personality> = std::fs::read_dir(self.personalities_dir())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "md"))
            .filter_map(|path| {
                Some(Personality {
                    name: path.file_stem()?.to_string_lossy().into_owned(),
                    path: path.to_string_lossy().into_owned(),
                })
            })
            .collect();
        found.sort_by_key(|p| p.name.to_lowercase());
        found
    }

    /// The text of the personality called `name`, when its file can be read and is not empty.
    pub(crate) fn personality(&self, name: &str) -> Option<String> {
        let path = self.personalities_dir().join(format!("{name}.md"));
        let text = std::fs::read_to_string(path).ok()?;
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }
}

/// Creates the folder at `dir` with the example personalities in it.
fn seed(dir: &Path) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!(error:% = e; "could not create the personalities folder: {{error}}");
        return;
    }
    for (name, text) in EXAMPLES {
        if let Err(e) = std::fs::write(dir.join(format!("{name}.md")), text) {
            log::warn!(error:% = e; "could not write an example personality: {{error}}");
        }
    }
}
