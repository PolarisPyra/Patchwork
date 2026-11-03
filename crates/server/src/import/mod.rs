//! Imports conversations that the agent CLIs saved on disk, so work started
//! outside Patchwork shows up as threads. Each imported thread keeps the CLI's
//! session id, so follow-up messages resume the original conversation.

mod ops;
mod types;

pub use types::{ImportScan, ImportSource};

mod antigravity;
mod claude;
mod codex;
mod opencode;
mod t3;

mod sessions;

pub use sessions::*;

/// `format!`s a line into the import log, `logging::IMPORT_LOG`.
macro_rules! ilog {
    ($($arg:tt)*) => {
        $crate::logging::IMPORT_LOG.write(
            $crate::settings::LogLevel::Info,
            "",
            &$crate::logging::Source::default(),
            $crate::logging::Data::Text(format!($($arg)*)),
        )
    };
}
pub(crate) use ilog;
