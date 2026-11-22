//! Steering input (ours, `notes/steering-input.md`): a gamepad stick and the keys, shaped beside the 1.02 filter.
//!
//! The device side ([`stick`]) reads a stick's round gate as square and takes its dead zone and saturation; the poll
//! then selects the filter's stick cell ([`STICK_CELL`]), which the 1.02 poll never selects. In the filter
//! ([`Steering::steer`]) the stick's target follows the speed curve ([`shaped`]) and steps at our rates with no fade;
//! the keys' cell steps at our rates with the grid's fade. Without [`Steering`]'s parts the grid steps as the 1.02's.

use crate::filter::Cell;

/// The filter's `STEER::STICK` cell: a gamepad stick ([`stick`]).
pub const STICK_CELL: u32 = 1;
/// The filter's `STEER::BUTTON_DIGITAL` cell: keys and buttons.
pub const BUTTON_CELL: u32 = 3;

/// The speed curve's `k` leaves 0 at this speed (m/s, the fade's speed: `crate::filter::notify_a`). Fitted on the
/// wheel drives (`harness/steer_fit.py`, `notes/steering-input.md`): a square from about 3.5 m/s, linear only at walking
/// pace and in big slides, where the fade's speed drops.
const CURVE_FROM: f32 = 1.5;
/// Its rise per m/s above [`CURVE_FROM`], up to 1 (a square).
const CURVE_SLOPE: f32 = 0.5;

/// A rate limit's rise and fall per second, against the output (`Cell::rise`, `Cell::fall`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    pub rise: f32,
    pub fall: f32,
}

impl Rates {
    /// `RISE,FALL` (`--key-steering`).
    pub fn parse(text: &str) -> Option<Rates> {
        let [rise, fall] = numbers(text)?;
        Some(Rates { rise, fall })
    }

    pub fn arg(&self) -> String {
        format!("{},{}", self.rise, self.fall)
    }
}

/// The stick's filtering: its rates and the speed curve's share of `k` (1 is [`k`] as fitted, 0 linear).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stick {
    pub rates: Rates,
    pub curve: f32,
}

impl Stick {
    /// `RISE,FALL,CURVE`, the curve in percent (`--stick-steering`).
    pub fn parse(text: &str) -> Option<Stick> {
        let [rise, fall, curve] = numbers(text)?;
        Some(Stick { rates: Rates { rise, fall }, curve: curve / 100.0 })
    }

    pub fn arg(&self) -> String {
        format!("{},{},{}", self.rates.rise, self.rates.fall, self.curve * 100.0)
    }
}

/// `N` finite numbers of at least 0, comma separated.
fn numbers<const N: usize>(text: &str) -> Option<[f32; N]> {
    let values: Vec<f32> = text
        .split(',')
        .map(|v| v.trim().parse().ok().filter(|v: &f32| v.is_finite() && *v >= 0.0))
        .collect::<Option<_>>()?;
    values.try_into().ok()
}

/// Ours in the filter, for the steering channel: the stick's cell and the keys' rates. `None` leaves the grid's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steering {
    pub stick: Option<Stick>,
    pub keys: Option<Rates>,
}

impl Steering {
    /// The cell and target the steering channel steps with, for the cell `select` the poll chose, the grid's cell
    /// `grid` and the raw target `a` at the fade's speed `speed`.
    pub fn steer(&self, select: u32, grid: &Cell, a: f32, speed: f32) -> (Cell, f32) {
        match (select, self.stick, self.keys) {
            (STICK_CELL, Some(stick), _) => {
                let cell = Cell {
                    active: true,
                    nonlinear: false,
                    lin: 1.0,
                    c2: 0.0,
                    c3: 0.0,
                    rise: stick.rates.rise,
                    fall: stick.rates.fall,
                    fade: 0.0,
                    fade_speed: 0.0,
                };
                (cell, shaped(a, speed, stick.curve))
            }
            (BUTTON_CELL, _, Some(keys)) => (Cell { active: true, rise: keys.rise, fall: keys.fall, ..*grid }, a),
            _ => (*grid, a),
        }
    }
}

/// The speed curve's blend at `speed` (m/s): 0 (linear) to 1 (a square).
pub fn k(speed: f32) -> f32 {
    ((speed - CURVE_FROM) * CURVE_SLOPE).clamp(0.0, 1.0)
}

/// The stick's target `x` (-1..1) at `speed`: `m + (m^2 - m) k` of its size `m` with `k` the share `curve` of [`k`],
/// so more of the travel goes to small angles the faster the car, while the end of the travel gives full lock at
/// every speed (OpenCMR2's response, `notes/steering-input.md`).
pub fn shaped(x: f32, speed: f32, curve: f32) -> f32 {
    let m = x.abs().min(1.0);
    let k = (k(speed) * curve).clamp(0.0, 1.0);
    (m + (m * m - m) * k).copysign(x)
}

/// A stick's axis `x` (-1..1) with its partner `y`: the round gate read as square (pushed to the edge on a diagonal,
/// the axis nearer the push reads full; along the axes and towards the centre nothing changes), then the dead zone
/// and the saturation (shares of the travel: below the dead zone 0, from the saturation on full).
pub fn stick(x: f32, y: f32, dead_zone: f32, saturation: f32) -> f32 {
    let longest = x.abs().max(y.abs());
    if longest <= 0.0 {
        return 0.0;
    }
    let square = (x * (x * x + y * y).sqrt() / longest).clamp(-1.0, 1.0);
    let m = square.abs();
    let v = if m <= dead_zone {
        0.0
    } else if m >= saturation || saturation <= dead_zone {
        1.0
    } else {
        (m - dead_zone) / (saturation - dead_zone)
    };
    v.copysign(square)
}

/// The player's steering settings (the launcher's `[steering]` in the bindings file): the stick's dead zone and
/// saturation (percent, the viewer's), its filtering and the keys' rates (the stepping process's flags).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub stick_dead_zone: f32,
    pub stick_saturation: f32,
    pub stick: Stick,
    pub keys: Rates,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            stick_dead_zone: 5.0,
            stick_saturation: 95.0,
            stick: Stick { rates: Rates { rise: 3.0, fall: 5.0 }, curve: 1.0 },
            keys: Rates { rise: 2.5, fall: 5.0 },
        }
    }
}

/// The settings' section in the bindings file.
pub const SECTION: &str = "steering";
/// The rates' range, per second.
pub const RATE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=20.0;
/// The curve's range, percent of the fitted `k`.
pub const CURVE_RANGE: std::ops::RangeInclusive<f32> = 0.0..=200.0;

impl Settings {
    /// `[steering]` of the bindings file; a key missing or out of range keeps its default.
    pub fn parse(text: &str) -> Settings {
        let mut out = Settings::default();
        let sections = rbr_formats::ini::parse(text).unwrap_or_default();
        let Some(section) = sections.iter().find(|s| s.name == SECTION) else {
            return out;
        };
        let number = |key: &str, range: std::ops::RangeInclusive<f32>, default: f32| {
            section.get(key).and_then(|v| v.parse().ok()).filter(|v| range.contains(v)).unwrap_or(default)
        };
        out.stick_dead_zone = number("stick_dead_zone", 0.0..=50.0, out.stick_dead_zone);
        out.stick_saturation = number("stick_saturation", 50.0..=100.0, out.stick_saturation);
        out.stick.rates.rise = number("stick_rise", RATE_RANGE, out.stick.rates.rise);
        out.stick.rates.fall = number("stick_fall", RATE_RANGE, out.stick.rates.fall);
        out.stick.curve = number("stick_curve", CURVE_RANGE, out.stick.curve * 100.0) / 100.0;
        out.keys.rise = number("key_rise", RATE_RANGE, out.keys.rise);
        out.keys.fall = number("key_fall", RATE_RANGE, out.keys.fall);
        out
    }

    /// The `[steering]` keys and values [`Settings::parse`] reads.
    pub fn entries(&self) -> [(&'static str, String); 7] {
        [
            ("stick_dead_zone", self.stick_dead_zone.to_string()),
            ("stick_saturation", self.stick_saturation.to_string()),
            ("stick_rise", self.stick.rates.rise.to_string()),
            ("stick_fall", self.stick.rates.fall.to_string()),
            ("stick_curve", (self.stick.curve * 100.0).to_string()),
            ("key_rise", self.keys.rise.to_string()),
            ("key_fall", self.keys.fall.to_string()),
        ]
    }

    /// The `[steering]` section [`Settings::parse`] reads.
    pub fn to_text(&self) -> String {
        self.entries().iter().fold(format!("[{SECTION}]\n"), |text, (key, value)| text + &format!("{key} = {value}\n"))
    }

    /// The stepping process's flags (`--stick-steering`, `--key-steering`), which a replay's header keeps.
    pub fn stepping_args(&self) -> [String; 4] {
        ["--stick-steering".into(), self.stick.arg(), "--key-steering".into(), self.keys.arg()]
    }

    /// The stick's dead zone and saturation as shares of the travel ([`stick`]).
    pub fn stick_shares(&self) -> (f32, f32) {
        (self.stick_dead_zone / 100.0, self.stick_saturation / 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::step;

    const DT: f32 = 0.007;

    #[test]
    fn the_curve_is_linear_when_slow_and_keeps_full_lock_at_every_speed() {
        for x in [0.0, 0.25, 0.5, 0.75] {
            assert_eq!(shaped(x, 0.0, 1.0), x);
            assert_eq!(shaped(-x, CURVE_FROM, 1.0), -x);
        }
        for speed in [0.0, 10.0, 20.0, 30.0, 60.0] {
            assert_eq!(shaped(1.0, speed, 1.0), 1.0);
            assert_eq!(shaped(-1.0, speed, 2.0), -1.0);
        }
        assert_eq!(shaped(0.5, 60.0, 1.0), 0.25, "a square at top speed");
        assert_eq!(shaped(0.5, 60.0, 0.0), 0.5, "no curve at 0 %");
        let mid = shaped(0.5, CURVE_FROM + 1.0, 1.0);
        assert!(0.25 < mid && mid < 0.5, "{mid}");
    }

    #[test]
    fn the_curve_never_turns_back() {
        for speed in [0.0, 15.0, 40.0] {
            let mut last = 0.0;
            for i in 1..=100 {
                let y = shaped(i as f32 / 100.0, speed, 2.0);
                assert!(y >= last, "{speed} {i}");
                last = y;
            }
        }
    }

    #[test]
    fn a_round_gate_reads_square() {
        let d = std::f32::consts::FRAC_1_SQRT_2;
        assert!(stick(d, d, 0.0, 1.0) > 0.9999, "the diagonal's edge is full");
        assert_eq!(stick(d, d, 0.05, 0.95), 1.0);
        assert_eq!(stick(-0.5, 0.0, 0.0, 1.0), -0.5, "along the axis nothing changes");
        assert!((stick(0.05, 1.0, 0.0, 1.0) - 0.05).abs() < 1e-3, "pushed up, the side stays small");
        assert_eq!(stick(0.0, 0.0, 0.05, 0.95), 0.0);
    }

    #[test]
    fn the_dead_zone_and_saturation_span_the_travel() {
        assert_eq!(stick(0.04, 0.0, 0.05, 0.95), 0.0);
        assert_eq!(stick(-0.96, 0.0, 0.05, 0.95), -1.0);
        assert!((stick(0.5, 0.0, 0.05, 0.95) - 0.5).abs() < 1e-6);
        assert_eq!(stick(0.6, 0.0, 0.5, 0.4), 1.0, "a saturation inside the dead zone is a switch");
    }

    /// Steps from `from` towards `to` until it is reached; the seconds it took.
    fn seconds(cell: &Cell, from: f32, to: f32) -> f32 {
        let (mut b, mut steps) = (from, 0);
        while b != to {
            b = step(cell, to, b, DT, 0.0);
            steps += 1;
            assert!(steps < 10_000);
        }
        steps as f32 * DT
    }

    #[test]
    fn the_stick_steps_at_its_rates_with_no_fade() {
        let settings = Settings::default();
        let steering = Steering { stick: Some(settings.stick), keys: None };
        let grid = Cell { fade: 0.5, fade_speed: 27.0, ..Cell::INITIAL };
        let (cell, a) = steering.steer(STICK_CELL, &grid, 1.0, 40.0);
        assert_eq!((cell.fade, a), (0.0, 1.0));
        let up = seconds(&cell, 0.0, 1.0);
        assert!((up - 1.0 / 3.0).abs() < 2.0 * DT, "{up}");
        let across = seconds(&cell, 1.0, -1.0);
        assert!((across - (1.0 / 5.0 + 1.0 / 3.0)).abs() < 3.0 * DT, "{across}");
    }

    #[test]
    fn the_keys_take_our_rates_and_keep_the_grid_s_fade() {
        let steering = Steering { stick: None, keys: Some(Rates { rise: 2.5, fall: 5.0 }) };
        let grid = Cell { rise: 0.3, fall: 0.5, fade: 0.5, fade_speed: 27.0, ..Cell::INITIAL };
        let (cell, _) = steering.steer(BUTTON_CELL, &grid, 1.0, 0.0);
        assert_eq!((cell.rise, cell.fall, cell.fade, cell.fade_speed), (2.5, 5.0, 0.5, 27.0));
        assert!(cell.active);
    }

    #[test]
    fn without_ours_the_grid_steps() {
        let grid = Cell { rise: 0.3, ..Cell::INITIAL };
        for select in [0, STICK_CELL, BUTTON_CELL] {
            assert_eq!(Steering::default().steer(select, &grid, 0.7, 30.0), (grid, 0.7));
        }
        let keys_only = Steering { stick: None, keys: Some(Rates { rise: 1.0, fall: 1.0 }) };
        assert_eq!(keys_only.steer(0, &grid, 0.7, 30.0), (grid, 0.7), "the axis cell is the grid's");
    }

    #[test]
    fn the_filter_steps_the_stick_with_ours() {
        use crate::filter::{Filter, Grid, Raw};
        let grid = Grid::from_settings(&crate::ini::Ini::parse(""));
        let mut raw = Raw::default();
        (raw.channels[0], raw.select[0]) = (0.5, STICK_CELL);
        let ours = Steering { stick: Some(Settings::default().stick), keys: None };
        let mut filter = Filter::new(grid.clone()).with_steering(ours);
        filter.notify(&raw);
        filter.set_speed(60.0);
        let first = filter.tic(DT).channels[0];
        assert!((first - 3.0 * DT).abs() < 1e-6, "eased at the rise: {first}");
        let last = (0..200).map(|_| filter.tic(DT).channels[0]).last();
        assert_eq!(last, Some(0.25), "the square at top speed");
        let mut theirs = Filter::new(grid);
        theirs.notify(&raw);
        assert_eq!(theirs.tic(DT).channels[0], 0.5, "the grid's stick cell has no rates");
    }

    #[test]
    fn settings_round_trip_through_the_file_and_the_flags() {
        let defaults = Settings::default();
        assert_eq!(Settings::parse(""), defaults);
        let text = "[steering]\nstick_dead_zone = 8\nstick_curve = 150\nkey_rise = 99\n";
        let read = Settings::parse(text);
        assert_eq!((read.stick_dead_zone, read.stick.curve, read.keys.rise), (8.0, 1.5, defaults.keys.rise));
        assert_eq!(Settings::parse(&read.to_text()), read);
        let [_, stick, _, keys] = read.stepping_args();
        assert_eq!((Stick::parse(&stick), Rates::parse(&keys)), (Some(read.stick), Some(read.keys)));
        assert_eq!(Stick::parse("1,2"), None);
        assert_eq!(Rates::parse("1,-2"), None);
    }
}
