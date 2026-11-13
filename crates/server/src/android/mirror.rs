//! A phone's screen in the panel. A phone has no stream to read as an emulator has, so adb is
//! asked for a picture again and again, about once a second, and a press on the picture is sent
//! back as the tap or swipe it made.
//!
//! Each screen runs on a thread of its own, which ends when the [`Screen`] is dropped or the
//! phone stops answering.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::{TryRecvError, UnboundedReceiver, unbounded};
use tokio::sync::watch;

use super::device::{adb, raw_screen, screen_size, swipe, tap};
use super::devices::ADB_LIMIT;
use super::screen::{Latest, Screen, adb_args};
use super::types::{ScreenFrame, ScreenInput, Sdk};

/// The pause between two pictures. Each one is the whole screen sent uncompressed, so asking
/// faster only keeps the phone and adb busy.
const BETWEEN_FRAMES: Duration = Duration::from_millis(700);

/// How long the thread sleeps while it waits for input or the next picture.
const IDLE: Duration = Duration::from_millis(30);

/// How far the pointer may move, as a fraction of the screen, and still have made a tap.
const TAP_SLOP: f32 = 0.02;

/// The shortest and longest swipe sent, in milliseconds: shorter than the first Android reads
/// a fling whatever the drag was, and a press held longer than the second is a drag all the same.
const SWIPE_MILLIS: std::ops::RangeInclusive<u128> = 80..=3000;

/// How far a line of the wheel's turn scrolls, as a fraction of the screen's height, and how
/// long its swipe takes, slow enough that Android scrolls by it and does not fling.
const WHEEL_DRAG: f32 = 0.06;
const WHEEL_MILLIS: u32 = 300;

/// Failed pictures in a row before the screen is given up: a phone that was unplugged.
const FAILURES: u32 = 3;

/// A press that has not been released yet.
struct Press {
    from: (f32, f32),
    to: (f32, f32),
    at: Instant,
}

/// The pixel a fraction of a screen `pixels` across falls on.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "a point on a screen a few thousand pixels across"
)]
fn place(fraction: f32, pixels: u32) -> u32 {
    (fraction.clamp(0., 1.) * pixels.saturating_sub(1) as f32) as u32
}

/// Sends what `input` did to the phone. A press is sent when it is released, as a tap when the
/// pointer stayed where it was, and as a swipe otherwise.
fn send(sdk: &Sdk, serial: &str, size: (u32, u32), press: &mut Option<Press>, input: &ScreenInput) {
    let point = |(x, y): (f32, f32)| (place(x, size.0), place(y, size.1));
    let sent = match *input {
        ScreenInput::Touch { x, y, down: true } => {
            match press {
                Some(press) => press.to = (x, y),
                None => {
                    *press = Some(Press {
                        from: (x, y),
                        to: (x, y),
                        at: Instant::now(),
                    });
                }
            }
            return;
        }
        ScreenInput::Touch { down: false, .. } => {
            let Some(press) = press.take() else { return };
            let moved = (press.to.0 - press.from.0)
                .abs()
                .max((press.to.1 - press.from.1).abs());
            if moved <= TAP_SLOP {
                let (x, y) = point(press.from);
                tap(sdk, serial, x, y)
            } else {
                let held = press.at.elapsed().as_millis();
                let millis = held.clamp(*SWIPE_MILLIS.start(), *SWIPE_MILLIS.end());
                let millis = u32::try_from(millis).unwrap_or(WHEEL_MILLIS);
                swipe(sdk, serial, point(press.from), point(press.to), millis)
            }
        }
        ScreenInput::Wheel { x, y, dy } => {
            let to = (y + dy * WHEEL_DRAG).clamp(0., 1.);
            swipe(sdk, serial, point((x, y)), point((x, to)), WHEEL_MILLIS)
        }
        ScreenInput::Key(_) | ScreenInput::Text(_) => {
            let Some(args) = adb_args(input) else { return };
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            adb(sdk, serial, &args, ADB_LIMIT).map(|_| ())
        }
        // A phone's picture comes at the phone's own size.
        ScreenInput::Fit { .. } => return,
    };
    if let Err(e) = sent {
        log::warn!(error:% = e; "the phone did not take the input: {{error}}");
    }
}

/// Sends the phone's screen into `latest` and what comes in on `inputs` to the phone, until the
/// [`Screen`] is dropped or the phone stops answering.
fn run(
    sdk: &Sdk,
    serial: &str,
    size: (u32, u32),
    latest: &watch::Sender<Latest>,
    mut inputs: UnboundedReceiver<ScreenInput>,
) {
    let mut press = None;
    let mut next_frame = Instant::now();
    let mut failures = 0;
    while !latest.is_closed() {
        let mut acted = false;
        loop {
            let input = match inputs.try_recv() {
                Ok(input) => input,
                Err(TryRecvError::Empty) => break,
                // The screen was dropped.
                Err(TryRecvError::Closed) => return,
            };
            acted |= !matches!(
                input,
                ScreenInput::Fit { .. } | ScreenInput::Touch { down: true, .. }
            );
            send(sdk, serial, size, &mut press, &input);
        }
        if !acted && Instant::now() < next_frame {
            std::thread::sleep(IDLE);
            continue;
        }
        match raw_screen(sdk, serial) {
            Ok((width, height, bgra)) => {
                failures = 0;
                latest.send_replace(Some(Ok(Arc::new(ScreenFrame {
                    width,
                    height,
                    bgra,
                }))));
            }
            Err(e) => {
                failures += 1;
                if failures == FAILURES {
                    latest.send_replace(Some(Err(e.to_string())));
                    return;
                }
            }
        }
        next_frame = Instant::now() + BETWEEN_FRAMES;
    }
}

/// The screen of the phone `serial`, mirrored until the returned [`Screen`] is dropped.
///
/// # Errors
///
/// Returns an error when the thread that reads the screen cannot start.
pub(super) fn start(sdk: Sdk, serial: &str) -> Result<Screen, std::io::Error> {
    // The size a phone is most often made with, for one that does not tell its own.
    let size = screen_size(&sdk, serial).unwrap_or((1080, 2400));
    let (latest_tx, latest) = watch::channel(None);
    let (input, inputs) = unbounded();
    let serial = serial.to_string();
    std::thread::Builder::new()
        .name(format!("screen {serial}"))
        .spawn(move || run(&sdk, &serial, size, &latest_tx, inputs))?;
    Ok(Screen::new(latest, input, size))
}
