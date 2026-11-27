//! The controls every settings pane draws: a section's card of rows, a switch, a row of
//! choices, a text field, a dropdown, and the badges around them.

// Rust guideline compliant 2026-10-09

mod fields;
mod found;
mod section;
mod widgets;

pub use fields::{Field, FieldKind};
pub use found::{anchored, center_when_drawn, flashing, seek, sought};
pub use section::{Section, key_cap, tint};
pub use widgets::{
    ACCESS_MODES, letter_badge, token_row, section_heading, switch, segmented, select_box, choice,
    choices, switch_row, radio, form_footer, remove_button, empty, search_box, detail_header,
    minutes_stepper, alert, labeled, chip, list_box, tildify, tile, tile_grid, example,
    vault_notice,
};
