//! The rally-school record `rbr-standalone --live` writes after MNU1, only for a school drive (flow mode 7): where
//! the lesson stands for the viewer's brief, toast and verdict panels (`rbr_render::hud::school`). A reader without
//! the record (an older process) shows no school panels. Little-endian body:
//!
//! ```text
//! u32 lesson    the lesson's index (0..16)
//! u32 state     0 the brief (the countdown, waiting for the confirm), 1 driving, 2 ended
//! u32 verdict   0 failed, 1..3 passed with grade n/4, 4 perfect; 0xffffffff before the end
//! f32 time      the end's race time, s (0 before the end)
//! f32 perfect   the lesson's perfect time, s (0 untimed)
//! f32 pass      the lesson's pass time, s (0 untimed)
//! u32 incident  the incident code of this answer (0 none, else the last `Incident` code of the answer's events)
//! ```

use std::io;

use crate::{sized, word};

/// The tag.
pub const SCH1: [u8; 4] = *b"SCH1";
/// The body's size.
pub const BODY_BYTES: usize = 28;
/// No verdict yet: the drive has not ended.
pub const NO_VERDICT: u32 = 0xffff_ffff;
/// The brief: the countdown waits for the confirm.
pub const STATE_BRIEF: u32 = 0;
/// The lesson is driven.
pub const STATE_DRIVING: u32 = 1;
/// The drive ended.
pub const STATE_ENDED: u32 = 2;

/// One rally-school record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct School {
    pub lesson: u32,
    pub state: u32,
    pub verdict: u32,
    pub time: f32,
    pub perfect: f32,
    pub pass: f32,
    pub incident: u32,
}

impl Default for School {
    /// The first lesson's brief, before the drive.
    fn default() -> School {
        School { lesson: 0, state: STATE_BRIEF, verdict: NO_VERDICT, time: 0.0, perfect: 0.0, pass: 0.0, incident: 0 }
    }
}

impl School {
    /// The tagged record.
    pub fn record(&self) -> Vec<u8> {
        let mut body = Vec::with_capacity(BODY_BYTES);
        for w in [self.lesson, self.state, self.verdict] {
            body.extend_from_slice(&w.to_le_bytes());
        }
        for f in [self.time, self.perfect, self.pass] {
            body.extend_from_slice(&f.to_le_bytes());
        }
        body.extend_from_slice(&self.incident.to_le_bytes());
        crate::live::record(SCH1, &body)
    }

    /// The record's body (the bytes after the tag and length).
    pub fn read_body(body: &[u8]) -> io::Result<School> {
        let body: &[u8; BODY_BYTES] = sized(body, format!("an SCH1 body of {} bytes", body.len()))?;
        let float = |i: usize| f32::from_bits(word(body, i));
        Ok(School {
            lesson: word(body, 0),
            state: word(body, 1),
            verdict: word(body, 2),
            time: float(3),
            perfect: float(4),
            pass: float(5),
            incident: word(body, 6),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_round_trips() {
        let school =
            School { lesson: 2, state: STATE_ENDED, verdict: 3, time: 9.4, perfect: 9.0, pass: 10.0, incident: 4 };
        let bytes = school.record();
        assert_eq!(&bytes[..4], b"SCH1");
        assert_eq!(bytes[4..8], (BODY_BYTES as u32).to_le_bytes());
        assert_eq!(School::read_body(&bytes[8..]).unwrap(), school);
        let brief = School::default();
        assert_eq!(brief.verdict, NO_VERDICT);
        assert_eq!(School::read_body(&brief.record()[8..]).unwrap(), brief);
        assert!(School::read_body(&bytes[8..12]).is_err());
    }
}
