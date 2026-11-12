//! The tyre record (temperatures and wear) a live stepping process writes after each frame (`rbr-standalone --live`;
//! the viewer's HUD shows it, `rbr_render::hud::telemetry`). Ours. Little-endian:
//!
//! ```text
//! u32 tag         'TYR1'
//! u32 length      80
//! 4 wheels, 0 to 3, each:
//!   f32 surface   the mean of the tyre's eight sectors' surface temperatures, K (the tyre state's +0xd0 + 12 * k)
//!   f32 tread     the tread temperature under them, K (state +0x12c)
//!   f32 grip      the grip the tyre step last took from its temperatures (scratch +0x4bc: the tyre's `RubMyFct` at
//!                 the contact sector's surface, 0.6, and at the tread, 0.4, with the wet road's wear factor)
//!   f32 optimum   the temperature of the tyre's highest `RubMyFct` entry, K (its definition's +0x184 and +0x1a4)
//!   f32 wear      the mean of the eight sectors' wear, 0 new to 1 (the scratch's +0x414 table)
//! ```
//!
//! A wheel not yet built is five zeros. It is one of the tagged records (`crate::live`), so a reader that does not
//! know it skips it by its length.

use std::io;

use crate::{sized, word, WHEELS};

/// The tag.
pub const TYR1: [u8; 4] = *b"TYR1";
const FLOATS: usize = 5;
const PAYLOAD_BYTES: usize = 4 * FLOATS * WHEELS;

/// One tyre's temperatures and wear.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tyre {
    pub surface: f32,
    pub tread: f32,
    pub grip: f32,
    pub optimum: f32,
    pub wear: f32,
}

/// The tagged record of the four tyres.
pub fn record(tyres: [Tyre; WHEELS]) -> Vec<u8> {
    let floats = |t: &Tyre| [t.surface, t.tread, t.grip, t.optimum, t.wear];
    let payload: Vec<u8> = tyres.iter().flat_map(floats).flat_map(f32::to_le_bytes).collect();
    crate::live::record(TYR1, &payload)
}

/// Reads the record's payload (the bytes after its tag and length, `crate::live::read_records`).
pub fn from_payload(payload: &[u8]) -> io::Result<[Tyre; WHEELS]> {
    let payload: &[u8; PAYLOAD_BYTES] = sized(payload, format!("a tyre record of {} bytes", payload.len()))?;
    let float = |i: usize| f32::from_bits(word(payload, i));
    Ok(std::array::from_fn(|k| Tyre {
        surface: float(FLOATS * k),
        tread: float(FLOATS * k + 1),
        grip: float(FLOATS * k + 2),
        optimum: float(FLOATS * k + 3),
        wear: float(FLOATS * k + 4),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_round_trips() {
        let mut tyres = [Tyre::default(); WHEELS];
        tyres[1] = Tyre { surface: 351.5, tread: 338.0, grip: 0.97, optimum: 348.15, wear: 0.02 };
        let mut out = record(tyres);
        out.extend_from_slice(b"SCN1");
        let records = crate::live::read_records(&mut &out[..]).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, TYR1);
        assert_eq!(from_payload(&records[0].1).unwrap(), tyres);
        assert!(from_payload(&[0; 4]).is_err());
    }
}
