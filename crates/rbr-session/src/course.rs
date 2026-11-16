//! The geometry of a session: where its gates lie along the drive line, and whether the line is open or closed.

use rbr_formats::markers::StageMarkers;

/// A course's gates, in metres along the drive line.
#[derive(Debug, Clone, PartialEq)]
pub enum Course {
    /// A stage: the start line, the splits in ascending order and the finish line, all along an open line.
    PointToPoint { start: f32, sectors: Vec<f32>, finish: f32 },
    /// A closed line of `length` metres. Its seam is the lap line: the distance wraps to 0 there. `sectors` lie in
    /// `0..length`, ascending.
    Circuit { length: f32, sectors: Vec<f32> },
}

impl Course {
    /// A stage's course from its `.dls` keys. Like the timekeeper (`TimeKeeper::load`), a missing start or finish key
    /// is 0.
    pub fn stage(markers: &StageMarkers) -> Self {
        Course::PointToPoint {
            start: markers.start().unwrap_or(0.0),
            sectors: markers.splits().collect(),
            finish: markers.finish().unwrap_or(0.0),
        }
    }

    /// A circuit of `length` metres with sector gates at `sectors`: those outside `0..length` are dropped and the
    /// rest sorted. `None` for a length that is not a positive number.
    pub fn circuit(length: f32, sectors: impl IntoIterator<Item = f32>) -> Option<Self> {
        if !(length > 0.0 && length.is_finite()) {
            return None;
        }
        let mut sectors: Vec<f32> = sectors.into_iter().filter(|&s| s > 0.0 && s < length).collect();
        sectors.sort_by(f32::total_cmp);
        Some(Course::Circuit { length, sectors })
    }

    /// The sector (split) gates in ascending order.
    pub fn sectors(&self) -> &[f32] {
        match self {
            Course::PointToPoint { sectors, .. } | Course::Circuit { sectors, .. } => sectors,
        }
    }

    pub fn is_circuit(&self) -> bool {
        matches!(self, Course::Circuit { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_formats::markers::{keys_from_words, TAG_FINISH, TAG_SPLIT, TAG_START};

    #[test]
    fn a_stage_course_reads_the_keys() {
        let words =
            [TAG_START, 0, 5.0f32.to_bits(), TAG_SPLIT, 0, 100.0f32.to_bits(), TAG_FINISH, 0, 300.0f32.to_bits()];
        let course = Course::stage(&keys_from_words(&words));
        assert_eq!(course, Course::PointToPoint { start: 5.0, sectors: vec![100.0], finish: 300.0 });
        assert!(!course.is_circuit());
    }

    #[test]
    fn a_circuit_keeps_its_sectors_inside_the_lap_in_order() {
        let course = Course::circuit(1000.0, [700.0, 0.0, 300.0, 1000.0, 1200.0, f32::NAN]).unwrap();
        assert_eq!(course.sectors(), [300.0, 700.0]);
        assert!(course.is_circuit());
        assert!(Course::circuit(0.0, []).is_none() && Course::circuit(f32::NAN, []).is_none());
    }
}
