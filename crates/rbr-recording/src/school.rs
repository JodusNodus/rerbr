//! The rally-school record `rbr-standalone --live` writes after MNU1, only for a school drive (flow mode 7): where
//! the lesson stands for the viewer's brief, toast and verdict panels (`rbr_render::hud::school`), and the frame's
//! speech for the school voice (`rbr_sound::voice::Speech::School`). A reader without the record (an older process)
//! shows no school panels. Little-endian body:
//!
//! ```text
//! u32 lesson    the lesson's index (0..16)
//! u32 state     0 the brief (the countdown, waiting for the confirm), 1 driving, 2 ended
//! u32 verdict   0 failed, 1..3 passed with grade n/4, 4 perfect; 0xffffffff before the end
//! f32 time      the end's race time, s (0 before the end)
//! f32 perfect   the lesson's perfect time, s (0 untimed)
//! f32 pass      the lesson's pass time, s (0 untimed)
//! u32 incident  the incident code of this answer (0 none, else the last `Incident` code of the answer's events)
//! u32 stop      1 when the flow moved since the last answer (brief -> driving -> ended, or a restart): the viewer
//!               stops the playing word and flushes the queue (`E25School::OnSignal` 0x4b8200 with `{0,0,0,raw 0}`)
//! u32 count     the ids that follow
//! u32[count]    the ids `Trigger` queued this answer, in order (each at priority 1, after a flush of the pending
//!               queue: `E25School::M80c0x` 0x4b80c0 over the pair's digit string)
//! u32 rcount    the reasons that follow (none before a failed end: `rbr_school::explain::failure_reasons`)
//! per reason: u32 lesson (0: a built-in key, 1: the lesson's own) + u32 len + [len]u8 key (UTF-8)
//! ```
//!
//! The first 28 bytes are the old body: a reader of it still works, and the new reader accepts it and the
//! speech-only body (with no reasons) too.

use std::io;

use crate::{sized, word};

/// The tag.
pub const SCH1: [u8; 4] = *b"SCH1";
/// The first body's size; longer bodies carry the frame's speech after it.
pub const BODY_BYTES: usize = 28;
/// No verdict yet: the drive has not ended.
pub const NO_VERDICT: u32 = 0xffff_ffff;
/// The brief: the countdown waits for the confirm.
pub const STATE_BRIEF: u32 = 0;
/// The lesson is driven.
pub const STATE_DRIVING: u32 = 1;
/// The drive ended.
pub const STATE_ENDED: u32 = 2;

/// One failure reason: a locale key for the verdict panel (`rbr_school::explain::Reason`), resolved through
/// the English string table at the viewer's end (unknown keys are skipped there).
#[derive(Debug, Clone, PartialEq)]
pub struct Reason {
    /// The key is the lesson's own (from its `.dls` string block), else a built-in.
    pub lesson: bool,
    /// The locale key.
    pub key: String,
}

/// One rally-school record.
#[derive(Debug, Clone, PartialEq)]
pub struct School {
    pub lesson: u32,
    pub state: u32,
    pub verdict: u32,
    pub time: f32,
    pub perfect: f32,
    pub pass: f32,
    pub incident: u32,
    /// The flow moved since the last answer: the viewer stops the speech and flushes its queue.
    pub stop: bool,
    /// The ids `Trigger` queued this answer, in order.
    pub speech: Vec<u32>,
    /// The failed end's reasons, in the original's order (empty before a failed end).
    pub reasons: Vec<Reason>,
}

impl Default for School {
    /// The first lesson's brief, before the drive.
    fn default() -> School {
        School {
            lesson: 0,
            state: STATE_BRIEF,
            verdict: NO_VERDICT,
            time: 0.0,
            perfect: 0.0,
            pass: 0.0,
            incident: 0,
            stop: false,
            speech: Vec::new(),
            reasons: Vec::new(),
        }
    }
}

impl School {
    /// The tagged record.
    pub fn record(&self) -> Vec<u8> {
        let mut body = Vec::with_capacity(BODY_BYTES + 8 + 4 * self.speech.len());
        for w in [self.lesson, self.state, self.verdict] {
            body.extend_from_slice(&w.to_le_bytes());
        }
        for f in [self.time, self.perfect, self.pass] {
            body.extend_from_slice(&f.to_le_bytes());
        }
        body.extend_from_slice(&self.incident.to_le_bytes());
        body.extend_from_slice(&u32::from(self.stop).to_le_bytes());
        body.extend_from_slice(&(self.speech.len() as u32).to_le_bytes());
        for id in &self.speech {
            body.extend_from_slice(&id.to_le_bytes());
        }
        body.extend_from_slice(&(self.reasons.len() as u32).to_le_bytes());
        for reason in &self.reasons {
            body.extend_from_slice(&u32::from(reason.lesson).to_le_bytes());
            body.extend_from_slice(&(reason.key.len() as u32).to_le_bytes());
            body.extend_from_slice(reason.key.as_bytes());
        }
        crate::live::record(SCH1, &body)
    }

    /// The record's body (the bytes after the tag and length): the 28-byte body, the speech body (with no
    /// reasons), or the full body with the failed end's reasons.
    pub fn read_body(body: &[u8]) -> io::Result<School> {
        if body.len() == BODY_BYTES {
            let body: &[u8; BODY_BYTES] = sized(body, format!("an SCH1 body of {} bytes", body.len()))?;
            return Ok(school_of(body, false, Vec::new(), Vec::new()));
        }
        if body.len() < BODY_BYTES + 8 {
            return Err(crate::invalid(format!("an SCH1 body of {} bytes", body.len())));
        }
        let base: &[u8; BODY_BYTES] = sized(&body[..BODY_BYTES], format!("an SCH1 body of {} bytes", body.len()))?;
        let stop = u32::from_le_bytes(body[28..32].try_into().map_err(|_| crate::invalid("an SCH1 stop"))?) != 0;
        let count = u32::from_le_bytes(body[32..36].try_into().map_err(|_| crate::invalid("an SCH1 count"))?) as usize;
        if BODY_BYTES + 8 + 4 * count > body.len() {
            return Err(crate::invalid(format!("an SCH1 body of {} bytes", body.len())));
        }
        let speech = (0..count)
            .map(|k| u32::from_le_bytes(body[36 + 4 * k..40 + 4 * k].try_into().expect("an SCH1 id")))
            .collect();
        let reasons = read_reasons(&body[BODY_BYTES + 8 + 4 * count..])?;
        Ok(school_of(base, stop, speech, reasons))
    }
}

/// The reasons after the speech ids: a count, then each reason's lesson flag, key length and key bytes.
fn read_reasons(mut tail: &[u8]) -> io::Result<Vec<Reason>> {
    if tail.is_empty() {
        return Ok(Vec::new());
    }
    let invalid = || crate::invalid(format!("an SCH1 reasons tail of {} bytes", tail.len()));
    if tail.len() < 4 {
        return Err(invalid());
    }
    let count = u32::from_le_bytes(tail[..4].try_into().map_err(|_| invalid())?) as usize;
    tail = &tail[4..];
    let mut reasons = Vec::with_capacity(count);
    for _ in 0..count {
        if tail.len() < 8 {
            return Err(invalid());
        }
        let lesson = u32::from_le_bytes(tail[..4].try_into().map_err(|_| invalid())?) != 0;
        let len = u32::from_le_bytes(tail[4..8].try_into().map_err(|_| invalid())?) as usize;
        tail = &tail[8..];
        if tail.len() < len {
            return Err(invalid());
        }
        let key = std::str::from_utf8(&tail[..len]).map_err(|_| invalid())?.to_owned();
        tail = &tail[len..];
        reasons.push(Reason { lesson, key });
    }
    if !tail.is_empty() {
        return Err(invalid());
    }
    Ok(reasons)
}

fn school_of(base: &[u8; BODY_BYTES], stop: bool, speech: Vec<u32>, reasons: Vec<Reason>) -> School {
    let float = |i: usize| f32::from_bits(word(base, i));
    School {
        lesson: word(base, 0),
        state: word(base, 1),
        verdict: word(base, 2),
        time: float(3),
        perfect: float(4),
        pass: float(5),
        incident: word(base, 6),
        stop,
        speech,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_round_trips() {
        let school = School {
            lesson: 2,
            state: STATE_ENDED,
            verdict: 3,
            time: 9.4,
            perfect: 9.0,
            pass: 10.0,
            incident: 4,
            stop: true,
            speech: vec![294, 295],
            reasons: Vec::new(),
        };
        let bytes = school.record();
        assert_eq!(&bytes[..4], b"SCH1");
        assert_eq!(bytes[4..8], ((BODY_BYTES + 8 + 8 + 4) as u32).to_le_bytes());
        assert_eq!(School::read_body(&bytes[8..]).unwrap(), school);
        let brief = School::default();
        assert_eq!(brief.verdict, NO_VERDICT);
        assert_eq!(School::read_body(&brief.record()[8..]).unwrap(), brief);
        assert!(School::read_body(&bytes[8..12]).is_err());
    }

    #[test]
    fn the_reasons_round_trip() {
        let school = School {
            state: STATE_ENDED,
            verdict: 0,
            speech: vec![294],
            reasons: vec![
                Reason { lesson: true, key: "RS_NOBRAKES_F".into() },
                Reason { lesson: false, key: "TIMEFAIL".into() },
            ],
            ..School::default()
        };
        let bytes = school.record();
        assert_eq!(School::read_body(&bytes[8..]).unwrap(), school);
        // A cut key is no body.
        assert!(School::read_body(&bytes[8..bytes.len() - 1]).is_err());
    }

    #[test]
    fn the_new_reader_accepts_the_old_body() {
        let mut body = Vec::new();
        for w in [2, STATE_DRIVING, NO_VERDICT] {
            body.extend_from_slice(&w.to_le_bytes());
        }
        for f in [0.0f32, 9.0, 10.0] {
            body.extend_from_slice(&f.to_le_bytes());
        }
        body.extend_from_slice(&4u32.to_le_bytes());
        assert_eq!(body.len(), BODY_BYTES);
        let school = School::read_body(&body).unwrap();
        assert_eq!((school.lesson, school.state, school.incident), (2, STATE_DRIVING, 4));
        assert!(!school.stop && school.speech.is_empty());
        let mut cut = body.clone();
        cut.extend_from_slice(&1u32.to_le_bytes());
        assert!(School::read_body(&cut).is_err(), "a cut count is no body");
    }
}
