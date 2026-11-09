//! The setup's values as one list in [`Setup::specs`] order, each with its spec and the car's range ([`Field`]), and
//! how two setups differ, line by line as the page lists them: a value, an axle's pair changed alike, or a diff
//! map's knots.

use rbr_gameplay::setup::{Area, Range, Setup, Slot, Spec};

/// The wheels by [`Spec::index`], as the page names them.
pub const WHEEL_NAMES: [&str; 4] = ["left front", "right front", "left rear", "right rear"];
/// The diff maps by [`Spec::index`].
pub const MAP_NAMES: [&str; 4] = ["centre", "centre (left-foot brake)", "front", "rear"];
/// The axles of the wheels' pairs (wheel `w` is on axle `w / 2`).
pub const AXLES: [&str; 2] = ["front", "rear"];
/// The areas whose values are per wheel.
pub const WHEEL_AREAS: [Area; 4] = [Area::Tyres, Area::Springs, Area::Dampers, Area::Geometry];

/// A value of the setup: what it is, its range on the car, and for a per-wheel value the other wheel of its axle.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub spec: Spec,
    pub range: Range,
    /// The index of the same value on the other wheel of the axle.
    pub pair: Option<usize>,
}

impl Field {
    /// Whether the car's options fix the value.
    pub fn fixed(&self) -> bool {
        self.range.max <= self.range.min || self.range.step <= 0.0
    }
}

/// The car's fields: [`Setup::specs`] with the car options' `ranges` (in the same order).
pub fn fields(ranges: &[Range]) -> Vec<Field> {
    let specs = Setup::specs();
    let pair = |s: &Spec| {
        let w = s.index.filter(|_| WHEEL_AREAS.contains(&s.area))?;
        specs.iter().position(|o| o.area == s.area && o.label == s.label && o.index == Some(w ^ 1))
    };
    let pairs: Vec<Option<usize>> = specs.iter().map(pair).collect();
    specs.into_iter().zip(ranges).zip(pairs).map(|((spec, &range), pair)| Field { spec, range, pair }).collect()
}

/// The index of the field with `key`.
pub fn find(fields: &[Field], key: &str) -> Option<usize> {
    fields.iter().position(|f| f.spec.key == key)
}

/// The index of `area`'s value `label` on wheel `w`.
pub fn wheel(fields: &[Field], area: Area, label: &str, w: usize) -> Option<usize> {
    fields.iter().position(|f| f.spec.area == area && f.spec.label == label && f.spec.index == Some(w))
}

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

/// `setup` with the values of `list` ([`values`] order).
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

/// Edits `draft` as a list of values with `base`'s beside it ([`values`] order).
pub fn edit(draft: &mut Setup, base: &Setup, f: impl FnOnce(&mut [f32], &[f32])) {
    let mut now = values(draft);
    f(&mut now, &values(base));
    *draft = with_values(draft, &now);
}

/// A line of the differences.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub area: Area,
    /// What changed: `Spring rate, front`, `Diff map, centre`.
    pub what: String,
    /// The value compared with and the value now, as shown; for a diff map, empty and the count of its knots.
    pub from: String,
    pub to: String,
    /// The values ([`values`] order) the line covers.
    pub indices: Vec<usize>,
}

/// The lines in which `now` differs from `base` (both [`values`] of the car of `fields`), in spec order; `show` gives
/// a value as the page shows it. An axle's two wheels changed from and to the same values are one line, a diff map's
/// knots one line.
pub fn changes(fields: &[Field], base: &[f32], now: &[f32], show: impl Fn(usize, f32) -> String) -> Vec<Change> {
    let differs = |i: usize| base.get(i) != now.get(i);
    let mut out: Vec<Change> = Vec::new();
    for (i, spec) in fields.iter().map(|f| &f.spec).enumerate().filter(|&(i, _)| differs(i)) {
        if let (Area::Differentials, Some(m)) = (spec.area, spec.index) {
            let what = format!("{}, {}", spec.label, MAP_NAMES[m]);
            match out.iter_mut().find(|c| c.what == what) {
                Some(c) => c.indices.push(i),
                None => {
                    out.push(Change { area: spec.area, what, from: String::new(), to: String::new(), indices: vec![i] })
                }
            }
            continue;
        }
        let (from, to) = (show(i, base[i]), show(i, now[i]));
        let (Some(w), pair) = (spec.index.filter(|_| WHEEL_AREAS.contains(&spec.area)), fields[i].pair) else {
            out.push(Change { area: spec.area, what: spec.label.to_owned(), from, to, indices: vec![i] });
            continue;
        };
        let alike = pair.filter(|&j| differs(j) && base[j] == base[i] && now[j] == now[i]);
        match alike {
            Some(_) if w % 2 == 1 => {} // listed with its left wheel
            Some(j) => {
                let what = format!("{}, {}", spec.label, AXLES[w / 2]);
                out.push(Change { area: spec.area, what, from, to, indices: vec![i, j] });
            }
            None => {
                let what = format!("{}, {}", spec.label, WHEEL_NAMES[w]);
                out.push(Change { area: spec.area, what, from, to, indices: vec![i] });
            }
        }
    }
    for c in out.iter_mut().filter(|c| c.from.is_empty()) {
        c.to = if c.indices.len() == 1 { "1 knot".to_owned() } else { format!("{} knots", c.indices.len()) };
    }
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// The fields of a car whose every range is 0 to 1e6 by 1.
    pub fn any_car() -> Vec<Field> {
        let range = Range { min: 0.0, max: 1e6, step: 1.0, wrap: false };
        fields(&vec![range; Setup::specs().len()])
    }

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
    fn a_wheels_pair_is_the_same_value_across_the_axle() {
        let fields = any_car();
        let at = |label, w| wheel(&fields, Area::Springs, label, w).unwrap();
        assert_eq!(fields[at("Spring rate", 2)].pair, Some(at("Spring rate", 3)));
        assert_eq!(fields[at("Spring rate", 1)].pair, Some(at("Spring rate", 0)));
        assert_eq!(fields[find(&fields, "FrontRollBarStiffness").unwrap()].pair, None);
    }

    #[test]
    fn an_axle_changed_alike_is_one_line_and_a_diff_map_one_line() {
        let fields = any_car();
        let mut now = Setup::default();
        let base = values(&now);
        for corner in &mut now.corners[..2] {
            corner.spring_stiffness = 28_000.0;
        }
        now.corners[3].tyre_pressure = 2.1e5;
        now.diff_maps[0].throttle[3] = 0.4;
        now.diff_maps[0].brake[5] = 0.2;
        now.front_roll_bar = 30_000.0;
        let lines = changes(&fields, &base, &values(&now), |_, v| format!("{v}"));
        let what: Vec<&str> = lines.iter().map(|c| c.what.as_str()).collect();
        assert_eq!(
            what,
            ["Front anti-roll bar", "Diff map, centre", "Spring rate, front", "Pressure, right rear"],
            "{lines:?}"
        );
        assert_eq!(lines[1].to, "2 knots");
        assert_eq!(lines[2].indices.len(), 2);
        assert_eq!((lines[2].from.as_str(), lines[2].to.as_str()), ("0", "28000"));
        // Taking each line's values back gives the comparison.
        let mut list = values(&now);
        for &i in lines.iter().flat_map(|c| &c.indices) {
            list[i] = base[i];
        }
        assert_eq!(list, base);
    }

    #[test]
    fn an_axle_changed_unlike_is_a_line_per_wheel() {
        let fields = any_car();
        let mut now = Setup::default();
        let base = values(&now);
        now.corners[0].bump = 4000.0;
        now.corners[1].bump = 4500.0;
        let lines = changes(&fields, &base, &values(&now), |_, v| format!("{v}"));
        let what: Vec<&str> = lines.iter().map(|c| c.what.as_str()).collect();
        assert_eq!(what, ["Bump (slow), left front", "Bump (slow), right front"]);
    }
}
