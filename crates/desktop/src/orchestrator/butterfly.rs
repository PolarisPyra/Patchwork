//! The butterfly: where it flies, which follows what the chats are doing, and its picture, a
//! pair of bending wings shaded in small characters around a body.

// Rust guideline compliant 2026-10-09

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "cell coordinates are a few hundred at most"
)]

use std::f32::consts::TAU;

use super::background::{Grid, Ink, scatter};

/// The characters a wing is shaded in, from the emptiest to the fullest.
const RAMP: &[u8; 10] = b" .:-=+*#%@";

/// Wing beats a second.
const BEAT: f32 = 1.1;

/// How fast it flies, in pixels a second. It never speeds up or slows down.
const SPEED: f32 = 90.;

/// How long a spark of its wake lasts.
const SPARK: f32 = 1.4;

/// How far along a wing its shape is worked out, in units, and in how many steps.
const SPAN: f32 = 2.9;
const STEPS: usize = 48;

/// A spark shed by a wing beat: where it is, where it is headed, and when it was shed.
struct Spark {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    born: f32,
}

/// The butterfly between two frames: where it is and how it moves.
#[derive(Default)]
pub(super) struct Flight {
    x: f32,
    y: f32,
    /// The way it is flying, eased so its lean does not snap when it turns.
    vx: f32,
    vy: f32,
    /// How many places it has flown to.
    leg: u32,
    /// How far through its beats the wings are, in turns.
    beats: f32,
    last: Option<f32>,
    /// The size of one unit of its picture in pixels: about a quarter of its height.
    unit: f32,
    sparks: Vec<Spark>,
}

impl Flight {
    /// Moves the butterfly to `time`: on from one place on the page to the next, at one speed.
    /// A `still` butterfly stays where it is.
    pub(super) fn step(&mut self, time: f32, page: (f32, f32), still: bool) {
        let (width, height) = page;
        self.unit = (height / 16.).clamp(38., 60.);
        let target = self.wander(page);
        let Some(last) = self.last.filter(|_| !still) else {
            // The first frame, or a still picture: it is where it is headed.
            (self.x, self.y) = target;
            (self.vx, self.vy) = (0., 0.);
            self.beats = 0.17;
            self.sparks.clear();
            self.last = Some(time);
            return;
        };
        let dt = (time - last).clamp(0., 0.1);
        self.last = Some(time);
        if dt <= 0. {
            return;
        }
        // It flies straight at the place, always at the same speed.
        let (dx, dy) = (target.0 - self.x, target.1 - self.y);
        let away = dx.hypot(dy);
        let reach = SPEED * dt;
        if away <= reach {
            // Having reached it, it picks the next.
            (self.x, self.y) = target;
            self.leg = self.leg.wrapping_add(1);
        } else {
            self.x = (self.x + dx / away * reach).clamp(0., width);
            self.y = (self.y + dy / away * reach).clamp(0., height);
            let ease = (4. * dt).min(1.);
            self.vx += (dx / away * SPEED - self.vx) * ease;
            self.vy += (dy / away * SPEED - self.vy) * ease;
        }

        let before = self.beats.floor();
        self.beats += BEAT * dt;
        if self.beats.floor() > before {
            self.shed(time);
        }
        for spark in &mut self.sparks {
            // Each curls a little as it slows.
            let (sin, cos) = (1.6 * dt).sin_cos();
            (spark.vx, spark.vy) = (
                (spark.vx * cos - spark.vy * sin) * (1. - dt),
                (spark.vx * sin + spark.vy * cos) * (1. - dt),
            );
            spark.x += spark.vx * dt;
            spark.y += spark.vy * dt;
        }
        self.sparks.retain(|spark| time - spark.born < SPARK);
    }

    /// The place it is flying to: a different one over the top of the page for each leg.
    fn wander(&self, page: (f32, f32)) -> (f32, f32) {
        let (width, height) = page;
        let across = (scatter(self.leg, 31) % 1000) as f32 / 1000.;
        let down = (scatter(self.leg, 32) % 1000) as f32 / 1000.;
        (
            width * (0.15 + 0.70 * across),
            // Low enough that the tips of its wings stay on the page.
            (height * (0.16 + 0.24 * down)).max(self.unit * 3.4),
        )
    }

    /// Sheds a spark from each wing tip, headed outward.
    fn shed(&mut self, time: f32) {
        let beat = self.beats as u32;
        for (index, side) in [-1_f32, 1.].into_iter().enumerate() {
            let h = scatter(beat, index as u32 + 77);
            let jitter = (h & 255) as f32 / 255. - 0.5;
            for burst in 0..2 {
                let spread = burst as f32 * 0.5 + jitter;
                self.sparks.push(Spark {
                    x: self.x + side * self.unit * 1.9,
                    y: self.y - self.unit * (0.6 - spread * 0.5),
                    vx: self.vx * 0.3 + side * (60. + 30. * spread),
                    vy: self.vy * 0.3 - 20. + 40. * spread,
                    born: time,
                });
            }
        }
    }

    /// Draws the sparks its wings shed, on the sky's layer.
    pub(super) fn wake(&self, grid: &mut Grid, time: f32) {
        for spark in &self.sparks {
            let (glyph, ink) = match (time - spark.born) / SPARK {
                young if young < 0.3 => ('*', Ink::Spark),
                mid if mid < 0.65 => ('+', Ink::Spark),
                _ => ('.', Ink::SparkFaint),
            };
            grid.put_at(spark.x, spark.y, glyph, ink);
        }
    }
}

/// A wing at one moment, worked out along its span: how far from the body each step shows on
/// the page, how high it is lifted, and how it catches the light.
struct Wing {
    across: [f32; STEPS],
    lift: [f32; STEPS],
    light: [f32; STEPS],
}

impl Wing {
    /// The wing `beats` turns into its beating. The tips lag behind the root and the whole wing
    /// is a little cupped, so it bends as it beats.
    fn at(beats: f32) -> Self {
        // It beats about level.
        let (mean, swing) = (0.12, 1.0);
        let mut wing = Self {
            across: [0.; STEPS],
            lift: [0.; STEPS],
            light: [0.; STEPS],
        };
        let (mut across, mut lift) = (0., 0.);
        let step = SPAN / STEPS as f32;
        for index in 0..STEPS {
            let out = index as f32 * step;
            let angle = mean + swing * (TAU * beats - 0.42 * out).sin() + 0.035 * out * out;
            across += angle.cos() * step;
            lift += angle.sin() * step;
            wing.across[index] = across;
            wing.lift[index] = lift;
            // The light comes from above and a little in front.
            wing.light[index] = 0.52 + 0.46 * (angle - 0.55).cos();
        }
        wing
    }

    /// How far along the wing the point that shows `across` from the body is, with its lift and
    /// light. None when the wing does not reach that far.
    fn under(&self, across: f32) -> Option<(f32, f32, f32)> {
        if across < 0. {
            return None;
        }
        let index = self.across.iter().position(|reach| *reach >= across)?;
        let out = (index as f32 + 0.5) * SPAN / STEPS as f32;
        Some((out, self.lift[index], self.light[index]))
    }
}

/// What a point of a wing is drawn as.
enum Mark {
    /// The dark border along the outer edge.
    Border,
    /// A pale spot in the border.
    Spot,
    /// A vein running out from the hinge.
    Vein,
    /// The lighter patch near the body.
    Patch,
    /// The open wing, with how far toward its edge the point is, from zero to one.
    Plain(f32),
}

/// The marking at a point of a wing, given along the wing and down it in units from the body's
/// middle. None outside the wing's outline.
fn marking(out: f32, down: f32) -> Option<Mark> {
    // The forewing, hinged above the middle: a pointed tip and a scalloped outer edge.
    let fore = {
        let (dx, dy) = (out, -0.22 - down);
        let angle = dy.atan2(dx);
        let tip = (1. - (angle - 0.80).abs() / 0.62).max(0.).powf(0.75);
        let scallop = if angle < 0.6 {
            0.04 * (angle * 15.).sin()
        } else {
            0.
        };
        let reach = (1.15 + 1.45 * tip) * (1. + scallop);
        ((-0.42..1.42).contains(&angle)).then(|| (dx.hypot(dy) / reach, angle, 9.))
    };
    // The hindwing, hinged below it: rounded, with a wavy edge.
    let hind = {
        let (dx, dy) = (out, down - 0.12);
        let angle = dy.atan2(dx);
        let round = ((angle - 0.72) * 1.25).cos().max(0.);
        let reach = (0.75 + 0.95 * round) * (1. + 0.05 * (angle * 11.).sin());
        ((-0.25..1.5).contains(&angle)).then(|| (dx.hypot(dy) / reach, angle, 7.))
    };
    let (edge, angle, veins) = [fore, hind]
        .into_iter()
        .flatten()
        .find(|(edge, _, _)| *edge < 1.)?;
    let mark = if edge > 0.84 {
        let spotted = (0.87..0.96).contains(&edge) && (angle * 17.).sin() > 0.72;
        if spotted { Mark::Spot } else { Mark::Border }
    } else if edge > 0.3 && (angle * veins).rem_euclid(1.) < 0.11 {
        Mark::Vein
    } else if edge < 0.3 {
        Mark::Patch
    } else {
        Mark::Plain(edge)
    };
    Some(mark)
}

/// The character and color for a point of a wing with this marking in this light.
fn shade(mark: &Mark, light: f32) -> (char, Ink) {
    let light = light.clamp(0., 1.);
    // A wing is dense: even its darkest part is drawn with a full character.
    let level = |low: f32, high: f32| {
        let index = (low + (high - low) * light).round() as usize;
        RAMP[index.min(RAMP.len() - 1)] as char
    };
    match mark {
        Mark::Spot => ('@', Ink::Spot),
        Mark::Border => (level(6., 8.), Ink::Border),
        Mark::Vein => (level(2., 4.), Ink::WingDeep),
        Mark::Patch if light > 0.86 => (level(7., 9.), Ink::WingHot),
        Mark::Patch => (level(6., 9.), Ink::WingBright),
        Mark::Plain(edge) => {
            let ink = match light {
                hot if hot > 0.93 => Ink::WingHot,
                bright if bright > 0.74 => Ink::WingBright,
                mid if mid > 0.42 => Ink::Wing,
                _ => Ink::WingDim,
            };
            // It thins out from the body toward the border.
            (level(8. - 6. * edge, 10. - 5. * edge), ink)
        }
    }
}

/// The body at a point given across and down in units: a head, a thorax and a tapering abdomen.
fn body(across: f32, down: f32) -> Option<(char, Ink)> {
    let within = |middle: f32, wide: f32, tall: f32| {
        (across / wide).powi(2) + ((down - middle) / tall).powi(2) < 1.
    };
    if within(-0.60, 0.12, 0.12) {
        return Some(('@', Ink::Body));
    }
    if within(-0.25, 0.15, 0.30) {
        return Some((if across.abs() < 0.07 { '#' } else { '%' }, Ink::Body));
    }
    let along = (down - 0.02) / 1.05;
    if (0. ..1.).contains(&along) && across.abs() < 0.11 * (1. - along).powf(0.7) + 0.02 {
        // The abdomen is ringed.
        let glyph = if (down * 9.).rem_euclid(1.) < 0.3 {
            '='
        } else {
            '#'
        };
        return Some((glyph, Ink::BodyDim));
    }
    None
}

/// Draws the butterfly on its own layer of small characters.
pub(super) fn draw(grid: &mut Grid, flight: &Flight) {
    let unit = flight.unit;
    if unit <= 0. {
        return;
    }
    let wing = Wing::at(flight.beats);
    // It leans into the way it is flying.
    let lean = (flight.vx / 700.).clamp(-0.22, 0.22);
    let (left, top) = grid.cell(flight.x - unit * 3.4, flight.y - unit * 3.2);
    let (right, bottom) = grid.cell(flight.x + unit * 3.4, flight.y + unit * 2.6);
    for row in top..=bottom {
        for col in left..=right {
            let (x, y) = grid.middle(col, row);
            let down = (y - flight.y) / unit;
            let across = (x - flight.x) / unit + down * lean;
            if let Some((glyph, ink)) = body(across, down) {
                grid.put(col, row, glyph, ink);
                continue;
            }
            // A sliver of sky shows between the body and each wing.
            let Some((out, raised, light)) = wing.under(across.abs() - 0.24) else {
                continue;
            };
            // A lifted wing shows higher on the page, seen a little from above.
            let Some(mark) = marking(out, down + raised * 0.34) else {
                continue;
            };
            // The far wing is a touch darker than the near one.
            let (glyph, ink) = shade(&mark, light - 0.05 * across.signum() * lean.signum());
            grid.put(col, row, glyph, ink);
        }
    }
    extras(grid, flight, lean);
}

/// The thin parts drawn over the wings: the antennae, each a dotted line ending in a knob, and
/// the legs tucked under the thorax.
fn extras(grid: &mut Grid, flight: &Flight, lean: f32) {
    let unit = flight.unit;
    let at = |across: f32, down: f32| {
        (
            flight.x + (across - down * lean) * unit,
            flight.y + down * unit,
        )
    };
    for side in [-1_f32, 1.] {
        let mut last = None;
        let mut dots = 0;
        for step in 0..=24 {
            let u = step as f32 / 24.;
            // Each curves up and out from the head.
            let (x, y) = at(side * (0.06 + 0.62 * u * u), -0.70 - 0.85 * u);
            let cell = grid.cell(x, y);
            if last == Some(cell) {
                continue;
            }
            last = Some(cell);
            dots += 1;
            if step == 24 {
                grid.put(cell.0, cell.1, 'o', Ink::Body);
            } else if dots % 2 == 0 {
                grid.put(cell.0, cell.1, '.', Ink::BodyDim);
            }
        }
        for (down, glyph) in [(-0.38, '\''), (-0.20, ','), (-0.04, ',')] {
            let (x, y) = at(side * 0.19, down);
            grid.put_at(x, y, glyph, Ink::BodyDim);
        }
    }
}
