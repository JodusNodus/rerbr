//! The damage record a live stepping process writes after each HUD record (`harness/live.py`'s `damage`,
//! `rbr-standalone --live`), which the viewer dents the car from (`notes/renderer.md` "Damage visuals").
//! Little-endian:
//!
//! ```text
//! u32 tag       'DMG1' (0x31474d44)
//! f32 zones[64] the damage handler's zone values (`*(car+0x9f4)+0x3808`, 0 to 1)
//! ```
//!
//! The window cracks come from the step's `CSigCarDamage` emits (PEV1, `crate::particles::CarDamage`), not from the
//! handler's record, which its functors rewrite without emitting.

use std::io::{self, Read};

use crate::{invalid, word};

/// The tag.
pub const DMG1: u32 = 0x3147_4d44;
/// The tagged CDM1 record (`rbr_recording::live`, after FMB1): the handler's words the service park reads, as
/// `rbr_gameplay::car_damage::CarriedDamage`'s `to_bytes` lays them out. The HUD's car status reads each repair part's
/// level from it under NGP's rules.
pub const CDM1: [u8; 4] = *b"CDM1";
/// Zone values in the record.
pub const ZONES: usize = 64;
/// The record's size.
pub const DMG1_BYTES: usize = 4 + 4 * ZONES;
/// `CCar` +0x9f4: the damage handler pointer.
pub const CAR_DAMAGE_HANDLER: u32 = 0x9f4;
/// Handler offset: the zone values.
pub const HANDLER_ZONES: u32 = 0x3808;

/// One damage record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Damage {
    pub zones: [f32; ZONES],
}

impl Default for Damage {
    fn default() -> Self {
        Damage { zones: [0.0; ZONES] }
    }
}

impl Damage {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DMG1_BYTES);
        out.extend_from_slice(&DMG1.to_le_bytes());
        for zone in self.zones {
            out.extend_from_slice(&zone.to_le_bytes());
        }
        out
    }

    pub fn read(reader: &mut impl Read) -> io::Result<Damage> {
        let mut bytes = [0u8; DMG1_BYTES];
        reader.read_exact(&mut bytes)?;
        if word(&bytes, 0) != DMG1 {
            let tag = word(&bytes, 0);
            return Err(invalid(format!("{tag:#010x} is not a damage record")));
        }
        Ok(Damage { zones: std::array::from_fn(|zone| f32::from_bits(word(&bytes, 1 + zone))) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let mut damage = Damage::default();
        damage.zones[1] = 1.0;
        damage.zones[63] = 0.25;
        let bytes = damage.to_bytes();
        assert_eq!(bytes.len(), DMG1_BYTES);
        assert_eq!(&bytes[..4], b"DMG1");
        assert_eq!(Damage::read(&mut bytes.as_slice()).unwrap(), damage);
    }

    #[test]
    fn other_tags_are_errors() {
        let mut bytes = Damage::default().to_bytes();
        bytes[3] = b'2';
        assert_eq!(Damage::read(&mut bytes.as_slice()).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
