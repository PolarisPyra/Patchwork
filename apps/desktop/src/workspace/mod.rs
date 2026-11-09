//! The window: the toolbar, the activity bar, the sidebar, the chat and the status bar, laid out as
//! `features/workspace/Workspace.tsx` lays them out, over the state they all read.

mod actions;
mod add_project;
mod background_dialog;
mod bars;
mod canvas_area;
mod onboarding;
mod palette;
mod panel_area;
mod projects;
mod region;
mod render;
mod state;
mod threads;

pub use state::*;
