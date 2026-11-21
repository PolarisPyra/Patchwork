//! Artifacts: what a chat's agent makes for the person to look at, such as a page, chart,
//! diagram or document, kept with every version and drawn on the Artifacts page.
//!
//! ```text
//! types     an artifact, its kinds and its versions
//! calls     the artifact tool's calls in a transcript, and what each saved
//! changes   what changed between two versions, line by line
//! files     where an artifact's files are kept, in its project's .patchwork folder
//! tool      the artifact tool agents make and change them with
//! versions  reading them, restoring a version, and writing a version as a page
//! render    a version's content as a whole web page
//! typed     the pages of docs, slides, sheets, designs and whiteboards
//! sheet     a sheet's CSV as cells, with its formulas worked out
//! thumbs    a picture of a version, for the gallery
//! ```

// Rust guideline compliant 2026-10-09

mod calls;
mod changes;
mod files;
mod render;
mod sheet;
mod thumbs;
mod tool;
mod typed;
mod types;
mod versions;

pub(crate) use calls::{CLAUDE_NAME, Written, fill_in};
pub use calls::{is_artifact_call, saved_by};
pub use changes::{ChangeRow, artifact_changes};
pub use tool::TOOL;
pub use types::{Artifact, ArtifactKind, ArtifactVersion};
