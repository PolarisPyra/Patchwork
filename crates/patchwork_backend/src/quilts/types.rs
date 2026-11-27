//! A quilt, what kind of content it holds, and its versions.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

/// What a quilt holds, which decides how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuiltKind {
    /// A web page, scripts and styles included.
    Html,
    /// An SVG picture.
    Svg,
    /// A document in Markdown.
    Markdown,
    /// A Mermaid diagram.
    Mermaid,
    /// A React component, whose default export is drawn.
    React,
    /// Source code, shown as it is.
    Code,
    /// A document in Markdown, drawn as a page of paper.
    Doc,
    /// A slide deck: one HTML `section` per 1280 by 720 slide, shown one at a time.
    Slides,
    /// A spreadsheet in CSV, whose cells may be formulas.
    Sheet,
    /// A visual design, such as an icon, poster or screen, as SVG or HTML of a fixed size, shown
    /// on a canvas.
    Design,
    /// A dashboard of charts, numbers and tables, as a whole HTML page.
    Data,
    /// A board of notes, shapes and arrows, as one SVG, on an endless board to pan and zoom.
    Whiteboard,
}

impl QuiltKind {
    /// Every kind, in the order the tool lists them.
    pub const ALL: [Self; 12] = [
        Self::Doc,
        Self::Slides,
        Self::Sheet,
        Self::Design,
        Self::Data,
        Self::Whiteboard,
        Self::Html,
        Self::Svg,
        Self::Markdown,
        Self::Mermaid,
        Self::React,
        Self::Code,
    ];

    /// The kinds the Quilts page has a tile for, in the order they are shown.
    pub const MADE_NEW: [Self; 6] = [
        Self::Doc,
        Self::Slides,
        Self::Sheet,
        Self::Design,
        Self::Data,
        Self::Whiteboard,
    ];

    /// The tile in `MADE_NEW` a quilt of this kind is counted under: web pages, components
    /// and SVG pictures are designs, Markdown is a doc, and a diagram goes on the whiteboard.
    /// Code has no tile.
    #[must_use]
    pub fn tile(self) -> Option<Self> {
        match self {
            Self::Doc | Self::Markdown => Some(Self::Doc),
            Self::Slides => Some(Self::Slides),
            Self::Sheet => Some(Self::Sheet),
            Self::Design | Self::Html | Self::React | Self::Svg => Some(Self::Design),
            Self::Data => Some(Self::Data),
            Self::Whiteboard | Self::Mermaid => Some(Self::Whiteboard),
            Self::Code => None,
        }
    }

    /// What the person sees the kind called.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Html => "Web page",
            Self::Svg => "SVG",
            Self::Markdown => "Markdown",
            Self::Mermaid => "Diagram",
            Self::React => "React",
            Self::Code => "Code",
            Self::Doc => "Doc",
            Self::Slides => "Slides",
            Self::Sheet => "Sheet",
            Self::Design => "Design",
            Self::Data => "Data",
            Self::Whiteboard => "Whiteboard",
        }
    }

    /// The kind with its article, as in "Making a web page".
    #[must_use]
    pub fn noun(self) -> &'static str {
        match self {
            Self::Html => "a web page",
            Self::Svg => "an SVG picture",
            Self::Markdown => "a Markdown document",
            Self::Mermaid => "a diagram",
            Self::React => "a React component",
            Self::Code => "some code",
            Self::Doc => "a doc",
            Self::Slides => "a slide deck",
            Self::Sheet => "a sheet",
            Self::Design => "a design",
            Self::Data => "a dashboard",
            Self::Whiteboard => "a whiteboard",
        }
    }

    /// The kind's short name, as the tool and the database name it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Svg => "svg",
            Self::Markdown => "markdown",
            Self::Mermaid => "mermaid",
            Self::React => "react",
            Self::Code => "code",
            Self::Doc => "doc",
            Self::Slides => "slides",
            Self::Sheet => "sheet",
            Self::Design => "design",
            Self::Data => "data",
            Self::Whiteboard => "whiteboard",
        }
    }

    /// The kind's media type, which agents trained on Claude's artifacts may give instead.
    #[must_use]
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Html => "text/html",
            Self::Svg => "image/svg+xml",
            Self::Markdown => "text/markdown",
            Self::Mermaid => "application/vnd.ant.mermaid",
            Self::React => "application/vnd.ant.react",
            Self::Code => "application/vnd.ant.code",
            Self::Doc => "application/vnd.ant.artifact.doc",
            Self::Slides => "application/vnd.ant.artifact.slides",
            Self::Sheet => "text/csv",
            Self::Design => "application/vnd.ant.artifact.design",
            Self::Data => "application/vnd.patchwork.data",
            Self::Whiteboard => "application/vnd.patchwork.whiteboard",
        }
    }

    /// The kind a short name or media type names.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|k| k.name() == text || k.media_type() == text)
    }

    /// The name of the file a version is saved as, for its extension.
    #[must_use]
    pub fn extension(self, language: Option<&str>) -> String {
        match self {
            Self::Html | Self::Slides | Self::Design | Self::Data => "html".into(),
            Self::Svg | Self::Whiteboard => "svg".into(),
            Self::Markdown | Self::Doc => "md".into(),
            Self::Sheet => "csv".into(),
            Self::Mermaid => "mmd".into(),
            Self::React => "jsx".into(),
            Self::Code => language
                .filter(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or("txt")
                .to_ascii_lowercase(),
        }
    }

    /// Whether it is drawn as a page, rather than shown as code alone.
    #[must_use]
    pub fn drawn(self) -> bool {
        self != Self::Code
    }
}

/// Something an agent made for the person to look at, as it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Quilt {
    pub id: String,
    /// The chat whose agent made it.
    pub chat_id: String,
    /// What the agent calls it, unique in its chat.
    pub key: String,
    pub kind: QuiltKind,
    pub title: String,
    /// The language of code, such as `rust`.
    pub language: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    /// Whether the person pinned it, which keeps it first in the gallery.
    pub pinned: bool,
    /// How many versions it has; the newest is this number.
    pub versions: u32,
}

/// One version of a quilt's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuiltVersion {
    /// Its number, from 1.
    pub n: u32,
    pub content: String,
    pub created_at: u64,
}
