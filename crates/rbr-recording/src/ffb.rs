//! The steering-force record a live stepping process writes after each frame (`rbr-standalone --live`; the viewer
//! drives the wheel's force feedback from it). Ours: the game sends the force to the wheel, not a record.
//! Little-endian:
//!
//! ```text
//! u32 tag     'FFB1'
//! u32 length  8
//! f32 force   the steering force (`steering_ffb::force`), [-1, 1] before the player's settings
//! u32 surface the percentage NGP's car took its force-feedback sensitivity by: 0 gravel, 1 tarmac, 2 snow
//!             (`rbr_physics::ngp::ffb::surface_index`; the viewer's pause menu reshapes the force to a new one)
//! ```
//!
//! It is one of the tagged records (`crate::live`), so a reader that does not know it skips it by its length.

use std::io;

use crate::sized;

/// The tag.
pub const FFB1: [u8; 4] = *b"FFB1";
const PAYLOAD_BYTES: usize = 8;

/// One steering-force record.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Steering {
    pub force: f32,
    /// 0 gravel, 1 tarmac, 2 snow.
    pub surface: u32,
}

impl Steering {
    /// The record.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = self.force.to_le_bytes().to_vec();
        payload.extend_from_slice(&self.surface.to_le_bytes());
        crate::live::record(FFB1, &payload)
    }

    /// Reads the record's payload (the bytes after its tag and length, `crate::live::read_records`).
    pub fn from_payload(payload: &[u8]) -> io::Result<Steering> {
        let payload: &[u8; PAYLOAD_BYTES] = sized(payload, format!("a force record of {} bytes", payload.len()))?;
        let word = |k: usize| [payload[k], payload[k + 1], payload[k + 2], payload[k + 3]];
        Ok(Steering { force: f32::from_le_bytes(word(0)), surface: u32::from_le_bytes(word(4)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_round_trips() {
        let record = Steering { force: -0.375, surface: 2 };
        let mut reader = &record.to_bytes()[..];
        let mut out = Vec::new();
        out.extend_from_slice(reader);
        out.extend_from_slice(b"SCN1");
        reader = &out[..];
        let records = crate::live::read_records(&mut reader).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, FFB1);
        assert_eq!(Steering::from_payload(&records[0].1).unwrap(), record);
        assert!(Steering::from_payload(&[0; 4]).is_err());
    }
}
