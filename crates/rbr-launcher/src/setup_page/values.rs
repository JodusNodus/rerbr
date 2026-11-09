//! The setup's values as one list in [`Setup::specs`] order, and how two setups differ, line by line as the page
//! lists it: a value, an axle's pair changed alike, or a diff map's knots.

use rbr_gameplay::setup::{Area, Setup, Slot, Spec};

/// The wheels by [`Spec::index`], as the page names them.
pub const WHEEL_NAMES: [&str; 4] = ["left front", "right front", "left rear", "right rear"];
/// The diff maps by [`Spec::index`].
pub const MAP_NAMES: [&str; 4] = ["centre", "centre (left-foot brake)", "front", "rear"];
/// The axles of the wheels' pairs (wheel `w` is on axle `w / 2`).
pub const AXLES: [&str; 2] = ["front", "rear"];

/// Every value of `setup`, in [`Setup::specs`] order (the drop gear as a number).
pub fn values(setup: &Setup) -> Vec<f32> {
    let mut copy = *setup;
    let mut out = Vec::new();
    copy.visit(&mut |_, slot| {
        out.push(match slot {
            Slot::Real(v) => *v,
            Slot::Unsigned(v) => *v as f32,
        })
    });
    out
}

/// `setup` with the values of `list` ([`values`] order) that differ from its own.
pub fn with_values(setup: &Setup, list: &[f32]) -> Setup {
    let mut out = *setup;
    let mut n = 0;
    out.visit(&mut |_, slot| {
        if let Some(&value) = list.get(n) {
            match slot {
                Slot::Real(v) => *v = value,
                Slot::Unsigned(v) => *v = value as u32,
            }
        }
        n += 1;
    });
    out
}

/// Whether the spec's values are per wheel (the tyre, spring, damper and geometry pages).
pub fn per_wheel(spec: &Spec) -> bool {
    matches!(spec.area, Area::Tyres | Area::Springs | Area::Dampers | Area::Geometry)
}

/// A line of the differences.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub area: Area,
    /// What changed: `Spring rate, front`, `Diff map, centre`.
    pub what: String,
    /// The value compared with and the value now, as shown; empty for a diff map's knots.
    pub from: String,
    pub to: String,
    /// The values ([`values`] order) the line covers.
    pub indices: Vec<usize>,
}

/// The lines in which `now` differs from `base` (both [`values`] of the same car), in spec order; `show` gives a
/// value as the page shows it. An axle's two wheels changed from and to the same values are one line, a diff map's
/// knots one line.
pub fn changes(specs: &[Spec], base: &[f32], now: &[f32], show: impl Fn(usize, f32) -> String) -> Vec<Change> {
    let differs = |i: usize| base.get(i) != now.get(i);
    let mut out: Vec<Change> = Vec::new();
    for (i, spec) in specs.iter().enumerate().filter(|&(i, _)| differs(i)) {
        if spec.area == Area::Differentials && spec.index.is_some() {
            let m = spec.index.unwrap_or(0);
            let what = format!("{}, {}", spec.label, MAP_NAMES[m]);
            match out.iter_mut().find(|c| c.what == what) {
                Some(c) => {
                    c.indices.push(i);
                    c.to = format!("{} knots", c.indices.len());
                }
                None => out.push(Change {
                    area: spec.area,
                    what,
                    from: String::new(),
                    to: "1 knot".to_owned(),
                    indices: vec![i],
                }),
            }
            continue;
        }
        let (from, to) = (show(i, base[i]), show(i, now[i]));
        let Some(w) = spec.index.filter(|_| per_wheel(spec)) else {
            out.push(Change { area: spec.area, what: spec.label.to_owned(), from, to, indices: vec![i] });
            continue;
        };
        // The other wheel of the axle: the same label at index `w ^ 1`.
        let pair = specs.iter().position(|s| s.label == spec.label && s.area == spec.area && s.index == Some(w ^ 1));
        let alike = pair.is_some_and(|j| differs(j) && base[j] == base[i] && now[j] == now[i]);
        if alike && w % 2 == 1 {
            continue; // listed with its left wheel
        }
        let (what, indices) = match (alike, pair) {
            (true, Some(j)) => (format!("{}, {}", spec.label, AXLES[w / 2]), vec![i, j]),
            _ => (format!("{}, {}", spec.label, WHEEL_NAMES[w]), vec![i]),
        };
        out.push(Change { area: spec.area, what, from, to, indices });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_go_back_into_the_setup_they_came_from() {
        let mut setup = Setup::default();
        setup.corners[2].spring_stiffness = 21_000.0;
        setup.drop_gear = 3;
        let list = values(&setup);
        assert_eq!(list.len(), Setup::specs().len());
        assert_eq!(with_values(&Setup::default(), &list), setup);
    }

    #[test]
    fn an_axle_changed_alike_is_one_line_and_a_diff_map_one_line() {
        let specs = Setup::specs();
        let mut now = Setup::default();
        let base = values(&now);
        for corner in &mut now.corners[..2] {
            corner.spring_stiffness = 28_000.0;
        }
        now.corners[3].tyre_pressure = 2.1e5;
        now.diff_maps[0].throttle[3] = 0.4;
        now.diff_maps[0].brake[5] = 0.2;
        now.front_roll_bar = 30_000.0;
        let lines = changes(&specs, &base, &values(&now), |_, v| format!("{v}"));
        let what: Vec<&str> = lines.iter().map(|c| c.what.as_str()).collect();
        assert_eq!(
            what,
            ["Front anti-roll bar", "Diff map, centre", "Spring rate, front", "Pressure, right rear"],
            "{lines:?}"
        );
        assert_eq!(lines[1].to, "2 knots");
        assert_eq!(lines[2].indices.len(), 2);
        assert_eq!((lines[2].from.as_str(), lines[2].to.as_str()), ("0", "28000"));
        // Reverting a line's values gives the comparison back.
        let mut list = values(&now);
        for c in &lines {
            for &i in &c.indices {
                list[i] = base[i];
            }
        }
        assert_eq!(list, base);
    }

    #[test]
    fn an_axle_changed_unlike_is_a_line_per_wheel() {
        let specs = Setup::specs();
        let mut now = Setup::default();
        let base = values(&now);
        now.corners[0].bump = 4000.0;
        now.corners[1].bump = 4500.0;
        let lines = changes(&specs, &base, &values(&now), |_, v| format!("{v}"));
        let what: Vec<&str> = lines.iter().map(|c| c.what.as_str()).collect();
        assert_eq!(what, ["Bump (slow), left front", "Bump (slow), right front"]);
    }
}
