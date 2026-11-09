//! File kinds and syntax colours for the canvas and chat code blocks.
//!
//! syntect reads each line with the language's Sublime Text grammar, from the grammars two-face
//! bundles, and each scope it finds is sorted into a [`Token`]. The theme colours a token only when
//! it is drawn, so a change of theme does not read the code again.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use gpui::{FontStyle, HighlightStyle};
use syntect::highlighting::ScopeSelectors;
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

use crate::ui::theme::Theme;

/// Files longer than this are shown without highlighting.
pub const MAX_HIGHLIGHT_LINES: usize = 5000;

/// Code blocks whose colours are kept, so a transcript redraws without reading them again. Past
/// this the cache starts over; it is about one long chat's worth of blocks.
const MAX_CACHED_BLOCKS: usize = 256;

/// What a run of code is; the theme picks its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Keyword,
    String,
    Property,
    Type,
    Number,
    Comment,
    Function,
    Tag,
    Inserted,
    Deleted,
}

impl Token {
    /// How the token is drawn in `t`.
    pub fn style(self, t: &Theme) -> HighlightStyle {
        let color = match self {
            Self::Keyword => t.syn_keyword,
            Self::String => t.syn_string,
            Self::Property => t.syn_property,
            Self::Type => t.syn_type,
            Self::Number => t.syn_number,
            Self::Comment => t.syn_comment,
            Self::Function => t.syn_function,
            Self::Tag => t.syn_tag,
            Self::Inserted => t.success,
            Self::Deleted => t.danger,
        };
        HighlightStyle {
            color: Some(color),
            font_style: (self == Self::Comment).then_some(FontStyle::Italic),
            ..Default::default()
        }
    }
}

/// One coloured run of a line or a code block.
pub type Run = (Range<usize>, Token);

/// The grammars, read once. Loading them takes a moment, so [`warm`] starts it off the UI thread.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Which scopes take which token. Of the selectors that match, the most specific wins, so
/// `keyword.operator` stays plain inside a `keyword`. None leaves the text plain.
static SCOPES: LazyLock<Vec<(ScopeSelectors, Option<Token>)>> = LazyLock::new(|| {
    [
        ("comment", Some(Token::Comment)),
        ("string, punctuation.definition.string", Some(Token::String)),
        (
            "constant.numeric, constant.language, constant.character",
            Some(Token::Number),
        ),
        ("keyword, storage, markup.heading", Some(Token::Keyword)),
        ("keyword.operator, punctuation", None),
        (
            "entity.name.function, support.function, variable.function, meta.diff.range",
            Some(Token::Function),
        ),
        (
            "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, \
             entity.name.trait, entity.name.section, entity.other.inherited-class, support.type, \
             support.class, storage.type.primitive",
            Some(Token::Type),
        ),
        ("entity.name.tag", Some(Token::Tag)),
        (
            "entity.other.attribute-name, variable.other.member, variable.other.property, \
             support.type.property-name, meta.mapping.key, meta.object-literal.key, \
             entity.name.tag.yaml",
            Some(Token::Property),
        ),
        ("markup.raw, markup.quote", Some(Token::Comment)),
        ("markup.inserted", Some(Token::Inserted)),
        ("markup.deleted", Some(Token::Deleted)),
    ]
    .into_iter()
    .filter_map(|(selector, token)| Some((ScopeSelectors::from_str(selector).ok()?, token)))
    .collect()
});

/// Code blocks already read, by a hash of their language and text.
static BLOCKS: LazyLock<Mutex<HashMap<u64, Arc<[Run]>>>> = LazyLock::new(Mutex::default);

/// Loads the grammars, so the first file or code block shown does not wait on them.
pub fn warm() {
    LazyLock::force(&SYNTAXES);
    LazyLock::force(&SCOPES);
}

/// Labels for common extensions; any other is shown in capitals.
const LABELS: &[(&str, &str)] = &[
    ("css", "CSS"),
    ("ts", "TypeScript"),
    ("tsx", "TypeScript"),
    ("js", "JavaScript"),
    ("jsx", "JavaScript"),
    ("mjs", "JavaScript"),
    ("cjs", "JavaScript"),
    ("rs", "Rust"),
    ("py", "Python"),
    ("json", "JSON"),
    ("md", "Markdown"),
    ("html", "HTML"),
    ("toml", "TOML"),
    ("yml", "YAML"),
    ("yaml", "YAML"),
    ("sh", "Shell"),
    ("bash", "Shell"),
    ("zsh", "Shell"),
    ("go", "Go"),
    ("c", "C"),
    ("h", "C"),
    ("cpp", "C++"),
    ("hpp", "C++"),
    ("cs", "C#"),
    ("java", "Java"),
    ("kt", "Kotlin"),
    ("swift", "Swift"),
    ("rb", "Ruby"),
    ("php", "PHP"),
    ("lua", "Lua"),
    ("sql", "SQL"),
    ("ps1", "PowerShell"),
    ("xml", "XML"),
    ("svg", "SVG"),
    ("scss", "SCSS"),
    ("graphql", "GraphQL"),
    ("diff", "Diff"),
    ("patch", "Diff"),
];

/// A file's extension, lowercased, or its whole name when it has none, as in `Makefile`.
fn extension(path: &str) -> String {
    let name = crate::ui::base_name(path);
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => name.to_string(),
    }
}

/// What a file is, by its extension: "Rust", "TOML", or the extension in capitals.
pub fn file_label(path: &str) -> String {
    let name = crate::ui::base_name(path);
    let ext = match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => return "Text".into(),
    };
    LABELS
        .iter()
        .find(|(e, _)| *e == ext)
        .map_or_else(|| ext.to_uppercase(), |(_, label)| (*label).to_string())
}

/// Lines of a file; a single trailing newline does not start a new line.
pub fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        return Vec::new();
    }
    content
        .strip_suffix('\n')
        .unwrap_or(content)
        .split('\n')
        .collect()
}

/// Colours a file's `lines` by the grammar its path names, one list of runs per line. A file no
/// grammar knows comes back plain. Reads the whole file, so call it off the UI thread.
pub fn highlight_file(path: &str, lines: &[&str]) -> Vec<Vec<Run>> {
    match SYNTAXES.find_syntax_by_token(&extension(path)) {
        Some(syntax) => highlight_lines(syntax, lines),
        None => vec![Vec::new(); lines.len()],
    }
}

/// A code block's runs, with offsets into `code`, for the language its fence names. Empty when the
/// language is not named or not known. A block read before is not read again.
pub fn highlight_code(code: &str, lang: &str) -> Arc<[Run]> {
    let Some(syntax) = syntax_for_lang(lang) else {
        return Arc::from([]);
    };
    let mut hasher = DefaultHasher::new();
    (lang, code).hash(&mut hasher);
    let key = hasher.finish();
    if let Some(runs) = BLOCKS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return Arc::clone(runs);
    }
    let lines: Vec<&str> = code.split('\n').collect();
    let mut runs = Vec::new();
    let mut at = 0;
    for (line, line_runs) in lines.iter().zip(highlight_lines(syntax, &lines)) {
        runs.extend(
            line_runs
                .into_iter()
                .map(|(r, token)| (r.start + at..r.end + at, token)),
        );
        at += line.len() + 1;
    }
    let runs: Arc<[Run]> = runs.into();
    let mut blocks = BLOCKS.lock().unwrap_or_else(PoisonError::into_inner);
    if blocks.len() >= MAX_CACHED_BLOCKS {
        blocks.clear();
    }
    blocks.insert(key, Arc::clone(&runs));
    runs
}

/// The grammar a code fence's language names, by its name or extension.
fn syntax_for_lang(lang: &str) -> Option<&'static SyntaxReference> {
    let lang = lang.trim().to_lowercase();
    let token = match lang.as_str() {
        "" | "text" | "plaintext" | "txt" => return None,
        "shell" | "console" | "shellscript" | "zsh" => "sh",
        "node" => "js",
        "python3" => "py",
        "golang" => "go",
        "c++" => "cpp",
        "csharp" | "c#" => "cs",
        "kotlin" => "kt",
        "powershell" | "pwsh" => "ps1",
        "jsonc" => "json",
        other => other,
    };
    SYNTAXES.find_syntax_by_token(token)
}

/// Reads `lines` in order with `syntax`, one list of runs per line. A line the grammar cannot read
/// stops the colouring there, and the lines after it are left plain.
fn highlight_lines(syntax: &SyntaxReference, lines: &[&str]) -> Vec<Vec<Run>> {
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut token = None;
    // The grammars expect each line to end in its newline.
    let mut text = String::new();
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        text.clear();
        text.push_str(line);
        text.push('\n');
        let Ok(ops) = state.parse_line(&text, &SYNTAXES) else {
            break;
        };
        let mut runs = Vec::new();
        let mut at = 0;
        for (offset, op) in ops {
            let end = offset.min(line.len());
            push_run(&mut runs, at..end, token);
            at = end;
            if stack.apply(&op).is_err() {
                break;
            }
            token = token_for(&stack);
        }
        push_run(&mut runs, at..line.len(), token);
        out.push(runs);
    }
    out.resize_with(lines.len(), Vec::new);
    out
}

/// The token of the most specific selector that matches `stack`.
fn token_for(stack: &ScopeStack) -> Option<Token> {
    SCOPES
        .iter()
        .filter_map(|(selector, token)| Some((selector.does_match(stack.as_slice())?, *token)))
        .max_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .and_then(|(_, token)| token)
}

/// Adds `range` as `token`, joining it to the run before when they are the same token.
fn push_run(runs: &mut Vec<Run>, range: Range<usize>, token: Option<Token>) {
    let Some(token) = token.filter(|_| !range.is_empty()) else {
        return;
    };
    match runs.last_mut() {
        Some((last, t)) if *t == token && last.end == range.start => last.end = range.end,
        _ => runs.push((range, token)),
    }
}
