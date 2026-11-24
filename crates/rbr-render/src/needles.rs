//! Moving gauge needles (ours): a model's gauges file (`rbr_formats::car_gauges`, `<model>_gauges.ini` beside its
//! `.sgc`) names its needle meshes, and the loader marks them ([`crate::damage::MeshDamage::needle`]). Each frame the
//! needles turn about their pivots by the engine speed, the speed or the boost of the frame shown ([`dials::Values`]),
//! in the interior set only. The DiRT Rally converter writes the file; RBRHUD gauges ([`crate::dials`]) are apart.
use glam::{Mat4, Vec3};
use rbr_formats::car_gauges::{self, Needle, Reading};
use rbr_formats::data::Data;
use std::path::Path;

use crate::dials;
use crate::playback::Playback;

/// Standard air pressure, Pa: the boost the game gives is absolute, the gauges show it over the air's.
const AIR: f32 = 101_325.0;
const PA_PER_BAR: f32 = 100_000.0;

/// The needles of the model at `model` (its `.sgc`); none without a gauges file, or with a bad one (reported).
pub fn of_model(model: &Path) -> Vec<Needle> {
    let Some(stem) = model.file_stem().and_then(|s| s.to_str()) else { return Vec::new() };
    let path = model.with_file_name(car_gauges::file_name(stem));
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    car_gauges::parse(&text).unwrap_or_else(|e| {
        eprintln!("needles: {}: {e}", path.display());
        Vec::new()
    })
}

/// A car's needles.
#[derive(Debug, Clone, PartialEq)]
pub struct Needles(Vec<Needle>);

impl Needles {
    /// Car `name`'s needles; `None` without any.
    pub fn load(data: &Data, name: &str) -> Option<Needles> {
        let model = crate::car::model_file(data, name).ok()?;
        Some(Needles(of_model(&model))).filter(|n| !n.0.is_empty())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Turns the needle meshes of `playback` to `values` (call before [`Playback::apply`]).
    pub fn turn(&self, playback: &mut Playback, values: &dials::Values) {
        for (k, needle) in self.0.iter().enumerate() {
            playback.set_needle_motion(k, motion(needle, values));
        }
    }
}

/// Needle `needle`'s turn at `values`, about its pivot.
fn motion(needle: &Needle, values: &dials::Values) -> Mat4 {
    let value = match needle.reading {
        Reading::Rpm => values.rpm,
        Reading::Kmh => values.kmh,
        Reading::Bar => values.boost.map_or(0.0, |pa| (pa - AIR) / PA_PER_BAR),
    };
    let pivot = Vec3::from(needle.pivot);
    let axis = Vec3::from(needle.axis).normalize_or(Vec3::Z);
    Mat4::from_translation(pivot) * Mat4::from_axis_angle(axis, needle.angle(value)) * Mat4::from_translation(-pivot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_needle_turns_about_its_pivot_by_its_reading() {
        let needle = Needle {
            node: "i_needle_tach".into(),
            reading: Reading::Rpm,
            from: 0.0,
            to: 10000.0,
            pivot: [1.0, 2.0, 3.0],
            axis: [0.0, 0.0, 1.0],
            sweep: -270.0,
        };
        let at = |rpm: f32| motion(&needle, &dials::Values { rpm, ..dials::Values::default() });
        assert_eq!(at(0.0), Mat4::IDENTITY);
        // The pivot stays; a point above it goes 135 degrees clockwise about +z at half range.
        assert!(at(5000.0).transform_point3(Vec3::new(1.0, 2.0, 3.0)).distance(Vec3::new(1.0, 2.0, 3.0)) < 1e-5);
        let tip = at(5000.0).transform_point3(Vec3::new(1.0, 3.0, 3.0));
        let expected = Vec3::new(1.0 + (135f32).to_radians().sin(), 2.0 + (135f32).to_radians().cos(), 3.0);
        assert!(tip.distance(expected) < 1e-5, "{tip}");
    }
}
