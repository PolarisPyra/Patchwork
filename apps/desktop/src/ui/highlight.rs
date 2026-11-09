//! File kinds and syntax colours for the canvas and chat code blocks (`lib/highlight.ts`). The web
//! UI runs TextMate grammars; this reads each line with a small scanner that knows each
//! language's keywords, comments and strings, and colours them with the same `--syn-*` tokens.

use std::ops::Range;

use gpui::{FontStyle, HighlightStyle, Hsla};

use crate::ui::theme::Theme;

/// Files longer than this are shown without highlighting.
pub const MAX_HIGHLIGHT_LINES: usize = 5000;

/// A file's kind: its label, and the grammar it is coloured with, if any.
pub struct FileKind {
    pub label: String,
    pub grammar: Option<&'static str>,
}

const KINDS: &[(&str, &str, &str)] = &[
    ("css", "CSS", "css"),
    ("ts", "TypeScript", "typescript"),
    ("tsx", "TypeScript", "tsx"),
    ("js", "JavaScript", "javascript"),
    ("jsx", "JavaScript", "javascript"),
    ("mjs", "JavaScript", "javascript"),
    ("cjs", "JavaScript", "javascript"),
    ("rs", "Rust", "rust"),
    ("py", "Python", "python"),
    ("json", "JSON", "json"),
    ("md", "Markdown", "markdown"),
    ("html", "HTML", "xml"),
    ("toml", "TOML", "ini"),
    ("yml", "YAML", "yaml"),
    ("yaml", "YAML", "yaml"),
    ("sh", "Shell", "bash"),
    ("bash", "Shell", "bash"),
    ("zsh", "Shell", "bash"),
    ("go", "Go", "go"),
    ("c", "C", "c"),
    ("h", "C", "c"),
    ("cpp", "C++", "cpp"),
    ("hpp", "C++", "cpp"),
    ("cs", "C#", "csharp"),
    ("java", "Java", "java"),
    ("kt", "Kotlin", "kotlin"),
    ("swift", "Swift", "swift"),
    ("rb", "Ruby", "ruby"),
    ("php", "PHP", "php"),
    ("lua", "Lua", "lua"),
    ("sql", "SQL", "sql"),
    ("ps1", "PowerShell", "powershell"),
    ("xml", "XML", "xml"),
    ("svg", "SVG", "xml"),
    ("scss", "SCSS", "scss"),
    ("graphql", "GraphQL", "graphql"),
    ("diff", "Diff", "diff"),
    ("patch", "Diff", "diff"),
];

/// What a file is, by its extension (`describeFile`).
pub fn describe_file(path: &str) -> FileKind {
    let name = crate::ui::base_name(path);
    let ext = match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    };
    match KINDS.iter().find(|(e, ..)| *e == ext) {
        Some((_, label, grammar)) => FileKind {
            label: label.to_string(),
            grammar: Some(grammar),
        },
        None => FileKind {
            label: if ext.is_empty() {
                "Text".into()
            } else {
                ext.to_uppercase()
            },
            grammar: None,
        },
    }
}

/// The grammar a code block's language names, if one is known.
pub fn grammar_for_lang(lang: &str) -> Option<&'static str> {
    let lang = lang.trim().to_lowercase();
    let alias = match lang.as_str() {
        "rust" => "rs",
        "typescript" => "ts",
        "javascript" | "node" => "js",
        "python" | "python3" => "py",
        "shell" | "console" | "shellscript" => "sh",
        "golang" => "go",
        "c++" => "cpp",
        "csharp" | "c#" => "cs",
        "kotlin" => "kt",
        "ruby" => "rb",
        "powershell" | "pwsh" => "ps1",
        "markdown" => "md",
        "yml" => "yaml",
        "ini" => "toml",
        other => other,
    };
    KINDS.iter().find(|(e, ..)| *e == alias).map(|(.., g)| *g)
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

struct Grammar {
    keywords: &'static [&'static str],
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    /// Quotes that open a string.
    quotes: &'static [char],
}

const C_LIKE: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "do",
    "switch",
    "case",
    "default",
    "break",
    "continue",
    "return",
    "goto",
    "struct",
    "union",
    "enum",
    "typedef",
    "const",
    "static",
    "extern",
    "volatile",
    "inline",
    "sizeof",
    "void",
    "int",
    "char",
    "short",
    "long",
    "float",
    "double",
    "unsigned",
    "signed",
    "bool",
    "true",
    "false",
    "NULL",
    "nullptr",
    "class",
    "public",
    "private",
    "protected",
    "virtual",
    "override",
    "template",
    "typename",
    "namespace",
    "using",
    "new",
    "delete",
    "this",
    "throw",
    "try",
    "catch",
    "auto",
    "constexpr",
    "noexcept",
    "operator",
    "final",
    "abstract",
    "interface",
    "extends",
    "implements",
    "import",
    "package",
    "super",
    "null",
    "instanceof",
    "var",
    "val",
    "fun",
    "object",
    "when",
    "is",
    "in",
    "out",
    "internal",
    "data",
    "sealed",
    "lateinit",
    "companion",
    "let",
    "func",
    "guard",
    "self",
    "Self",
    "init",
    "deinit",
    "protocol",
    "extension",
    "where",
    "async",
    "await",
    "readonly",
    "sealed",
    "string",
    "foreach",
    "yield",
    "get",
    "set",
    "lock",
    "base",
    "ref",
    "params",
    "partial",
];

const JS: &[&str] = &[
    "const",
    "let",
    "var",
    "function",
    "return",
    "if",
    "else",
    "for",
    "while",
    "do",
    "switch",
    "case",
    "default",
    "break",
    "continue",
    "new",
    "delete",
    "typeof",
    "instanceof",
    "in",
    "of",
    "class",
    "extends",
    "super",
    "this",
    "import",
    "export",
    "from",
    "as",
    "async",
    "await",
    "yield",
    "try",
    "catch",
    "finally",
    "throw",
    "true",
    "false",
    "null",
    "undefined",
    "void",
    "interface",
    "type",
    "enum",
    "implements",
    "public",
    "private",
    "protected",
    "readonly",
    "static",
    "abstract",
    "declare",
    "namespace",
    "keyof",
    "satisfies",
    "infer",
    "is",
    "get",
    "set",
    "string",
    "number",
    "boolean",
    "any",
    "unknown",
    "never",
    "object",
    "symbol",
    "bigint",
];

const RUST: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "const",
    "continue",
    "crate",
    "dyn",
    "else",
    "enum",
    "extern",
    "false",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "match",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "return",
    "self",
    "Self",
    "static",
    "struct",
    "super",
    "trait",
    "true",
    "type",
    "unsafe",
    "use",
    "where",
    "while",
    "union",
    "macro_rules",
];

const PYTHON: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda",
    "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with", "yield", "True",
    "False", "None", "self", "match", "case",
];

const SHELL: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "return", "local", "export", "readonly", "declare", "set", "unset", "shift",
    "exit", "echo", "source", "cd", "true", "false", "test",
];

const GO: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "true",
    "false",
    "nil",
    "string",
    "int",
    "bool",
    "error",
    "byte",
    "rune",
    "float64",
    "int64",
    "uint",
    "any",
];

const RUBY: &[&str] = &[
    "def",
    "end",
    "if",
    "elsif",
    "else",
    "unless",
    "while",
    "until",
    "for",
    "in",
    "do",
    "return",
    "class",
    "module",
    "self",
    "nil",
    "true",
    "false",
    "and",
    "or",
    "not",
    "begin",
    "rescue",
    "ensure",
    "yield",
    "require",
    "include",
    "extend",
    "attr_accessor",
    "attr_reader",
    "then",
    "case",
    "when",
    "lambda",
    "proc",
    "super",
];

const LUA: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

const SQL: &[&str] = &[
    "select",
    "from",
    "where",
    "insert",
    "into",
    "values",
    "update",
    "set",
    "delete",
    "create",
    "table",
    "drop",
    "alter",
    "add",
    "index",
    "on",
    "join",
    "left",
    "right",
    "inner",
    "outer",
    "group",
    "by",
    "order",
    "having",
    "limit",
    "offset",
    "as",
    "and",
    "or",
    "not",
    "null",
    "is",
    "in",
    "like",
    "primary",
    "key",
    "foreign",
    "references",
    "default",
    "unique",
    "distinct",
    "union",
    "all",
    "case",
    "when",
    "then",
    "else",
    "end",
    "integer",
    "text",
    "varchar",
    "begin",
    "commit",
    "exists",
    "if",
];

const PHP: &[&str] = &[
    "function",
    "return",
    "if",
    "else",
    "elseif",
    "foreach",
    "for",
    "while",
    "as",
    "class",
    "public",
    "private",
    "protected",
    "static",
    "new",
    "echo",
    "namespace",
    "use",
    "true",
    "false",
    "null",
    "array",
    "extends",
    "implements",
    "interface",
    "const",
    "throw",
    "try",
    "catch",
    "fn",
    "match",
];

const LITERALS: &[&str] = &["true", "false", "null"];

const POWERSHELL: &[&str] = &[
    "function", "param", "if", "else", "elseif", "foreach", "for", "while", "return", "switch",
    "try", "catch", "finally", "throw", "in", "begin", "process", "end",
];

const GRAPHQL: &[&str] = &[
    "query",
    "mutation",
    "subscription",
    "fragment",
    "on",
    "type",
    "input",
    "enum",
    "interface",
    "union",
    "scalar",
    "schema",
    "extend",
    "implements",
    "directive",
    "true",
    "false",
    "null",
];

fn grammar(name: &str) -> Option<Grammar> {
    let g = |keywords, line_comments, block_comment, quotes| Grammar {
        keywords,
        line_comments,
        block_comment,
        quotes,
    };
    const DQ: &[char] = &['"', '\''];
    Some(match name {
        "typescript" | "tsx" | "javascript" => {
            g(JS, &["//"], Some(("/*", "*/")), &['"', '\'', '`'])
        }
        "rust" => g(RUST, &["//"], Some(("/*", "*/")), &['"']),
        "c" | "cpp" | "csharp" | "java" | "kotlin" | "swift" => {
            g(C_LIKE, &["//"], Some(("/*", "*/")), DQ)
        }
        "go" => g(GO, &["//"], Some(("/*", "*/")), &['"', '\'', '`']),
        "python" => g(PYTHON, &["#"], None, DQ),
        "bash" => g(SHELL, &["#"], None, DQ),
        "ruby" => g(RUBY, &["#"], None, DQ),
        "yaml" | "ini" => g(LITERALS, &["#"], None, DQ),
        "lua" => g(LUA, &["--"], Some(("--[[", "]]")), DQ),
        "sql" => g(SQL, &["--"], Some(("/*", "*/")), DQ),
        "php" => g(PHP, &["//", "#"], Some(("/*", "*/")), DQ),
        "powershell" => g(POWERSHELL, &["#"], Some(("<#", "#>")), DQ),
        "graphql" => g(GRAPHQL, &["#"], None, &['"']),
        "json" => g(LITERALS, &[], None, &['"']),
        "css" | "scss" => g(&[], &["//"], Some(("/*", "*/")), DQ),
        "xml" | "markdown" | "diff" => g(&[], &[], None, &[]),
        _ => return None,
    })
}

/// One coloured run of a line.
pub type Run = (Range<usize>, HighlightStyle);

/// Colours `lines` with `grammar`, one list of runs per line. A grammar it does not know leaves
/// them plain.
pub fn highlight_lines(lines: &[&str], name: &str, t: &Theme) -> Vec<Vec<Run>> {
    let color = |c: Hsla| HighlightStyle {
        color: Some(c),
        ..Default::default()
    };
    let comment = HighlightStyle {
        color: Some(t.syn_comment),
        font_style: Some(FontStyle::Italic),
        ..Default::default()
    };
    match name {
        "xml" => return lines.iter().map(|l| markup_line(l, t)).collect(),
        "markdown" => {
            return lines
                .iter()
                .map(|l| {
                    let s = l.trim_start();
                    if s.starts_with('#') {
                        vec![(0..l.len(), color(t.syn_keyword))]
                    } else if s.starts_with("```") || s.starts_with('>') {
                        vec![(0..l.len(), comment)]
                    } else {
                        Vec::new()
                    }
                })
                .collect();
        }
        "diff" => {
            return lines
                .iter()
                .map(|l| {
                    let c = if l.starts_with("+++") || l.starts_with("---") {
                        Some(t.syn_keyword)
                    } else if l.starts_with('+') {
                        Some(t.success)
                    } else if l.starts_with('-') {
                        Some(t.danger)
                    } else if l.starts_with("@@") {
                        Some(t.syn_function)
                    } else {
                        None
                    };
                    c.map(|c| vec![(0..l.len(), color(c))]).unwrap_or_default()
                })
                .collect();
        }
        _ => {}
    }
    let Some(g) = grammar(name) else {
        return lines.iter().map(|_| Vec::new()).collect();
    };
    let css = matches!(name, "css" | "scss");
    let json = name == "json";
    let ini = matches!(name, "ini" | "yaml");
    let mut in_block = false;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let mut runs: Vec<Run> = Vec::new();
        let b = line.as_bytes();
        let mut i = 0;
        if in_block {
            let close = g.block_comment.map_or("*/", |(_, c)| c);
            match line.find(close) {
                Some(at) => {
                    runs.push((0..at + close.len(), comment));
                    i = at + close.len();
                    in_block = false;
                }
                None => {
                    runs.push((0..line.len(), comment));
                    out.push(runs);
                    continue;
                }
            }
        }
        // A TOML or INI section header, or a key before its `=` or `:`.
        if ini {
            let s = line.trim_start();
            let lead = line.len() - s.len();
            if s.starts_with('[') {
                runs.push((lead..line.len(), color(t.syn_type)));
                out.push(runs);
                continue;
            }
            if let Some(end) = s.find(['=', ':'])
                && !s.starts_with('#')
                && !s.starts_with('-')
            {
                let key = s[..end].trim_end();
                if !key.is_empty() && !key.contains(['"', '\'', ' ']) {
                    runs.push((lead..lead + key.len(), color(t.syn_property)));
                    i = lead + key.len();
                }
            }
        }
        while i < b.len() {
            let rest = &line[i..];
            if let Some((open, close)) = g.block_comment
                && rest.starts_with(open)
            {
                match rest[open.len()..].find(close) {
                    Some(at) => {
                        let end = i + open.len() + at + close.len();
                        runs.push((i..end, comment));
                        i = end;
                    }
                    None => {
                        runs.push((i..line.len(), comment));
                        in_block = true;
                        i = line.len();
                    }
                }
                continue;
            }
            if g.line_comments.iter().any(|c| rest.starts_with(c)) {
                // `#` inside a word, as in a URL's fragment, does not start a comment.
                let word_before = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'$');
                if !(rest.starts_with('#') && word_before) {
                    runs.push((i..line.len(), comment));
                    break;
                }
            }
            let ch = rest.chars().next().unwrap_or(' ');
            if g.quotes.contains(&ch) && !(ch == '\'' && name == "rust" && is_lifetime(rest)) {
                let open = i + ch.len_utf8();
                let mut end = line.len();
                let mut escaped = false;
                for (k, c) in line[open..].char_indices() {
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == ch {
                        end = open + k + c.len_utf8();
                        break;
                    }
                }
                // A JSON key is a string followed by a colon.
                let key = json && line[end..].trim_start().starts_with(':');
                let c = if key { t.syn_property } else { t.syn_string };
                runs.push((i..end, color(c)));
                i = end;
                continue;
            }
            if ch.is_ascii_digit() {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_')
                {
                    i += 1;
                }
                runs.push((start..i, color(t.syn_number)));
                continue;
            }
            if ch.is_alphabetic() || ch == '_' || (css && ch == '-') {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || (css && b[i] == b'-'))
                {
                    i += 1;
                }
                if i == start {
                    i += ch.len_utf8();
                    continue;
                }
                let word = &line[start..i];
                let after = line[i..].trim_start();
                let lower;
                let keyword = if name == "sql" {
                    lower = word.to_lowercase();
                    g.keywords.contains(&lower.as_str())
                } else {
                    g.keywords.contains(&word)
                };
                let c = if css {
                    after.starts_with(':').then_some(t.syn_property)
                } else if keyword {
                    Some(t.syn_keyword)
                } else if after.starts_with('(') || (name == "rust" && after.starts_with('!')) {
                    Some(t.syn_function)
                } else if word.chars().next().is_some_and(char::is_uppercase)
                    && word.chars().any(char::is_lowercase)
                {
                    Some(t.syn_type)
                } else {
                    None
                };
                if let Some(c) = c {
                    runs.push((start..i, color(c)));
                }
                continue;
            }
            if css && (ch == '.' || ch == '#') && i + 1 < b.len() && b[i + 1].is_ascii_alphabetic()
            {
                let start = i;
                i += 1;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'_')
                {
                    i += 1;
                }
                runs.push((start..i, color(t.syn_tag)));
                continue;
            }
            i += ch.len_utf8();
        }
        out.push(runs);
    }
    out
}

/// A Rust lifetime or label (`'a`), which is not a character literal.
fn is_lifetime(rest: &str) -> bool {
    let mut chars = rest.chars().skip(1);
    let first = chars.next();
    let second = chars.next();
    first.is_some_and(|c| c.is_alphabetic() || c == '_') && second != Some('\'')
}

/// Tags, attribute names and their values in HTML, XML and SVG.
fn markup_line(line: &str, t: &Theme) -> Vec<Run> {
    let color = |c: Hsla| HighlightStyle {
        color: Some(c),
        ..Default::default()
    };
    let b = line.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0;
    let mut in_tag = false;
    while i < b.len() {
        if line[i..].starts_with("<!--") {
            let end = line[i..].find("-->").map_or(line.len(), |at| i + at + 3);
            runs.push((
                i..end,
                HighlightStyle {
                    color: Some(t.syn_comment),
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                },
            ));
            i = end;
            continue;
        }
        if b[i] == b'<' {
            let start = i;
            i += 1;
            if i < b.len() && (b[i] == b'/' || b[i] == b'?' || b[i] == b'!') {
                i += 1;
            }
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b':' | b'_'))
            {
                i += 1;
            }
            runs.push((start..i, color(t.syn_tag)));
            in_tag = true;
            continue;
        }
        if in_tag {
            if b[i] == b'>' || line[i..].starts_with("/>") {
                in_tag = false;
                i += 1;
                continue;
            }
            if b[i] == b'"' || b[i] == b'\'' {
                let q = b[i];
                let end = line[i + 1..]
                    .bytes()
                    .position(|c| c == q)
                    .map_or(line.len(), |at| i + 1 + at + 1);
                runs.push((i..end, color(t.syn_string)));
                i = end;
                continue;
            }
            if b[i].is_ascii_alphabetic() {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b':' | b'_'))
                {
                    i += 1;
                }
                runs.push((start..i, color(t.syn_property)));
                continue;
            }
        }
        i += line[i..].chars().next().map_or(1, char::len_utf8);
    }
    runs
}
