//! The circuit record `rbr-standalone --live` writes after SCH1, only for a circuit session (`--laps`,
//! `notes/session.md`): where the laps stand, for the viewer's lap panel (`rbr_render::hud::laps`). A reader without
//! the record (an older process) shows no lap panel. Little-endian body:
//!
//! ```text
//! u32 lap       the lap in progress, from 1; 0 before the first lap starts
//! u32 laps      the session's laps; 0 for free practice
//! u32 finished  1 once the end rule was met
//! f32 elapsed   the time into the lap in progress, s (0 before the start and once finished)
//! f32 last      the last completed lap's time, s (0 none)
//! f32 best      the best completed lap's time, s (0 none)
//! u32 count     the sector times of the lap in progress that follow
//! f32[count]    this lap's sector times so far, s since the lap began
//! u32 bcount    the sector times of the best lap that follow
//! f32[bcount]   the best lap's sector times, s since its lap began
//! ```

use std::io;

use crate::live::record;

/// The tag.
pub const LAP1: [u8; 4] = *b"LAP1";

/// One circuit record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Laps {
    pub lap: u32,
    pub laps: u32,
    pub finished: bool,
    pub elapsed: f32,
    pub last: f32,
    pub best: f32,
    pub sectors: Vec<f32>,
    pub best_sectors: Vec<f32>,
}

const HEADER_BYTES: usize = 24;

impl Laps {
    /// The tagged record.
    pub fn record(&self) -> Vec<u8> {
        let mut body = Vec::with_capacity(HEADER_BYTES + 8 + 4 * (self.sectors.len() + self.best_sectors.len()));
        for w in [self.lap, self.laps, u32::from(self.finished)] {
            body.extend_from_slice(&w.to_le_bytes());
        }
        for f in [self.elapsed, self.last, self.best] {
            body.extend_from_slice(&f.to_le_bytes());
        }
        for times in [&self.sectors, &self.best_sectors] {
            body.extend_from_slice(&(times.len() as u32).to_le_bytes());
            for t in times {
                body.extend_from_slice(&t.to_le_bytes());
            }
        }
        record(LAP1, &body)
    }

    /// The record's body (the bytes after the tag and length).
    pub fn read_body(body: &[u8]) -> io::Result<Laps> {
        let invalid = || crate::invalid(format!("a LAP1 body of {} bytes", body.len()));
        let word = |at: usize| -> io::Result<u32> {
            Ok(u32::from_le_bytes(body.get(at..at + 4).ok_or_else(invalid)?.try_into().map_err(|_| invalid())?))
        };
        let times = |at: usize| -> io::Result<(Vec<f32>, usize)> {
            let count = word(at)? as usize;
            let end = at + 4 + 4 * count;
            let times = (0..count).map(|k| word(at + 4 + 4 * k).map(f32::from_bits)).collect::<io::Result<Vec<_>>>()?;
            Ok((times, end))
        };
        let (sectors, next) = times(HEADER_BYTES)?;
        let (best_sectors, end) = times(next)?;
        if end != body.len() {
            return Err(invalid());
        }
        Ok(Laps {
            lap: word(0)?,
            laps: word(4)?,
            finished: word(8)? != 0,
            elapsed: f32::from_bits(word(12)?),
            last: f32::from_bits(word(16)?),
            best: f32::from_bits(word(20)?),
            sectors,
            best_sectors,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_round_trips() {
        let laps = Laps {
            lap: 3,
            laps: 5,
            finished: false,
            elapsed: 41.5,
            last: 445.25,
            best: 440.0,
            sectors: vec![150.5],
            best_sectors: vec![148.0, 301.25],
        };
        let bytes = laps.record();
        assert_eq!(&bytes[..4], &LAP1);
        let body = &bytes[8..];
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, body.len());
        assert_eq!(Laps::read_body(body).unwrap(), laps);
        assert_eq!(Laps::read_body(&Laps::default().record()[8..]).unwrap(), Laps::default());
    }

    #[test]
    fn a_short_or_long_body_is_refused() {
        let body = Laps { sectors: vec![1.0], ..Laps::default() }.record()[8..].to_vec();
        assert!(Laps::read_body(&body[..body.len() - 1]).is_err());
        assert!(Laps::read_body(&[body.clone(), vec![0; 4]].concat()).is_err());
        assert!(Laps::read_body(&[]).is_err());
    }
}
