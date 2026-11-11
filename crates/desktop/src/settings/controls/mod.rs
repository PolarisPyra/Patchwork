//! The controls every settings pane draws: a row, a switch, a row of
//! choices, a text field, a dropdown, and the badges around them.

// Rust guideline compliant 2026-10-09

mod fields;
mod widgets;

pub use fields::{Field, FieldKind};
pub use widgets::{
    letter_badge, token_row, section_heading, switch, segmented, select_box, choice,
    minutes_stepper, alert, chip, list_box,
};
