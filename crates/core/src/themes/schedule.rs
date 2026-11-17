//! Whether it is dark now by the clock or by the sun, and how long until that changes.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use chrono::{Datelike, Local, Timelike, Utc};

use super::applied::Follow;

/// The seconds in a day.
const DAY: u32 = 24 * 60 * 60;

/// How long until the sun is looked at again where it does not rise or set today.
const POLAR_WAIT: Duration = Duration::from_secs(6 * 60 * 60);

/// Whether a window that follows `follow` is dark now, and how long that holds. None when it
/// follows the system, which tells the window itself.
pub(super) fn dark_now(follow: Follow) -> Option<(bool, Duration)> {
    match follow {
        Follow::System => None,
        Follow::Clock { light_at, dark_at } => {
            let now = Local::now().num_seconds_from_midnight();
            let (light_at, dark_at) = (minutes(light_at), minutes(dark_at));
            Some(between(now, light_at, dark_at))
        }
        Follow::Sun {
            latitude,
            longitude,
        } => {
            let now = Utc::now();
            let day = now.ordinal();
            Some(match sun(day, latitude, longitude) {
                Sun::Rises { rise, set } => between(now.num_seconds_from_midnight(), rise, set),
                Sun::Up => (false, POLAR_WAIT),
                Sun::Down => (true, POLAR_WAIT),
            })
        }
    }
}

/// The minutes after midnight that a time of day written `7:30` or `19:05` is.
#[must_use]
pub fn clock_minutes(text: &str) -> Option<u16> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let (hours, minutes) = (
        hours.trim().parse::<u16>().ok()?,
        minutes.trim().parse::<u16>().ok()?,
    );
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// A time of day in minutes after midnight, written `7:30`.
#[must_use]
pub fn clock_text(minutes: u16) -> String {
    format!("{}:{:02}", minutes / 60 % 24, minutes % 60)
}

/// A time of day in minutes, as seconds after midnight.
fn minutes(of_day: u16) -> u32 {
    u32::from(of_day) * 60 % DAY
}

/// Whether `now` is outside the part of the day from `light_at` round to `dark_at`, and the
/// time until it crosses into or out of it. All are seconds after midnight.
fn between(now: u32, light_at: u32, dark_at: u32) -> (bool, Duration) {
    let until = |at: u32| (at + DAY - now - 1) % DAY + 1;
    let (to_light, to_dark) = (until(light_at), until(dark_at));
    // The next of the two to come is the one that ends what holds now.
    let dark = to_light <= to_dark;
    let wait = to_light.min(to_dark);
    (dark, Duration::from_secs(u64::from(wait)))
}

/// What the sun does on one day at one place.
enum Sun {
    /// It rises and sets, at these seconds after midnight in UTC.
    Rises { rise: u32, set: u32 },
    /// It stays up all day.
    Up,
    /// It stays down all day.
    Down,
}

/// The sun on the `day`th day of the year at a latitude and longitude in degrees, by the
/// equations NOAA publishes for its solar calculator.
fn sun(day: u32, latitude: f32, longitude: f32) -> Sun {
    let (latitude, longitude) = (f64::from(latitude.clamp(-89., 89.)), f64::from(longitude));
    let year = std::f64::consts::TAU / 365. * (f64::from(day) - 1.);
    // How far the sun's noon is from the clock's, in minutes.
    let offset = 229.18
        * (0.000_075 + 0.001_868 * year.cos()
            - 0.032_077 * year.sin()
            - 0.014_615 * (2. * year).cos()
            - 0.040_849 * (2. * year).sin());
    let tilt = 0.006_918 - 0.399_912 * year.cos() + 0.070_257 * year.sin()
        - 0.006_758 * (2. * year).cos()
        + 0.000_907 * (2. * year).sin()
        - 0.002_697 * (3. * year).cos()
        + 0.001_48 * (3. * year).sin();
    let latitude = latitude.to_radians();
    // The sun's center is 90.833 degrees from overhead when its edge meets the horizon.
    let cosine =
        90.833_f64.to_radians().cos() / (latitude.cos() * tilt.cos()) - latitude.tan() * tilt.tan();
    if cosine < -1. {
        return Sun::Up;
    }
    if cosine > 1. {
        return Sun::Down;
    }
    let half_day = cosine.acos().to_degrees();
    let at = |minutes: f64| {
        let seconds = (minutes * 60.).rem_euclid(f64::from(DAY));
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the remainder is between zero and the seconds in a day"
        )]
        let seconds = seconds as u32;
        seconds % DAY
    };
    Sun::Rises {
        rise: at(720. - 4. * (longitude + half_day) - offset),
        set: at(720. - 4. * (longitude - half_day) - offset),
    }
}
