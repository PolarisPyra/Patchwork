//! How actions read and describe themselves.

// Rust guideline compliant 2026-10-09

use super::actions::*;

#[test]
fn names_map_to_keys() {
    key("Enter").unwrap();
    key("esc").unwrap();
    key("a").unwrap();
    key("not-a-key").unwrap_err();
    modifier("ctrl").unwrap();
    modifier("Hyper").unwrap_err();
}

#[test]
fn actions_describe_themselves() {
    assert_eq!(
        describe(&Action::Click {
            x: 10,
            y: 20,
            button: "left".into(),
            double: true
        }),
        "Double-click left at 10, 20"
    );
    assert_eq!(
        describe(&Action::Key {
            key: "s".into(),
            modifiers: vec!["ctrl".into()]
        }),
        "Press ctrl+s"
    );
    let long = "x".repeat(60);
    assert!(describe(&Action::Type { text: long }).ends_with("…\""));
}

#[test]
fn an_action_reads_from_json_the_way_the_model_sends_it() {
    let click: Action = serde_json::from_str(r#"{"kind":"click","x":1,"y":2}"#).expect("a click");
    assert_eq!(
        click,
        Action::Click {
            x: 1,
            y: 2,
            button: "left".into(),
            double: false
        }
    );
    let key: Action =
        serde_json::from_str(r#"{"kind":"key","key":"t","modifiers":["ctrl","shift"]}"#)
            .expect("a key");
    assert_eq!(
        key,
        Action::Key {
            key: "t".into(),
            modifiers: vec!["ctrl".into(), "shift".into()]
        }
    );
}

/// Ignored by default: it needs a display, and it moves nothing on a machine without one.
/// Run it with `cargo test -p patchwork-server -- --ignored screenshot_captures_the_screen`.
#[test]
#[ignore = "needs a display"]
fn screenshot_captures_the_screen() {
    let started = std::time::Instant::now();
    let shot = screenshot().expect("a screenshot");
    eprintln!(
        "capture took {:?} ({}x{} from {}x{})",
        started.elapsed(),
        shot.width,
        shot.height,
        shot.screen_width,
        shot.screen_height
    );
    assert!(shot.width > 0 && shot.height > 0);
    // The PNG header, so the bytes really are a picture.
    assert_eq!(
        &shot.png[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
}
