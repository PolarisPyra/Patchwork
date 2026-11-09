//! Settings → Extensions (`Extensions` in `SettingsDialog.tsx`): what agents can be given, one
//! kind at a time: custom agents, skills, and hooks. Each tab draws itself in its own file.

mod agents;
mod hooks;
mod skills;
mod view;

pub(super) use crate::settings::controls::list_box;
pub use view::*;
