//! The detached car parts a live stepping process writes after each damage record (`rbr-standalone --live`), which the
//! viewer draws at their physics poses (rbr-recomp `notes/car-damage-visuals.md`, section 8). Little-endian:
//!
//! ```text
//! u32 tag       'DET1'
//! u32 len       the entries' bytes (a multiple of ENTRY_BYTES)
//! per entry (32 bytes):
//! u32 part      the index into PARTS
//! f32 position[3]     the body's world position, the physics frame (m)
//! f32 orientation[4]  its world orientation, [x, y, z, w]
//! ```
//!
//! An entry is a detached part whose body is awake: in the physics awake list (`+0x450`) with its parent cleared by the
//! detach. A part that falls asleep is left out, and the viewer keeps its last pose. A reader skips a tag it does not
//! know by its length.

use std::io;

use crate::{invalid, word};

/// The tag.
pub const DET1: [u8; 4] = *b"DET1";
/// The detachable parts, by their exterior node names (`CAR_PARTS` in `rbr-standalone`): the doors and bonnet (hinged
/// bodies), the exhaust, the bumpers and the wing (mounted bodies). The index is the record's `part`.
pub const PARTS: [&str; 7] = ["e_door_l", "e_door_r", "e_bonnet", "e_exhaust", "e_bumper_f", "e_bumper_b", "e_wing"];
/// The bytes of one entry.
pub const ENTRY_BYTES: usize = 32;

/// The index in [`PARTS`] of the node `name`.
pub fn part_index(name: &str) -> Option<u32> {
    PARTS.iter().position(|&part| part == name).map(|i| i as u32)
}

/// One detached part's body pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartPose {
    pub part: u32,
    pub position: [f32; 3],
    pub orientation: [f32; 4],
}

/// The attached doors' hinge angles a live stepping process writes after each DET1 record
/// (`rbr-standalone --live`), which the viewer hangs the doors by. Little-endian:
///
/// ```text
/// u32 tag       'DOR1'
/// u32 len       the entries' bytes (a multiple of DOOR_ENTRY_BYTES)
/// per entry (8 bytes):
/// u32 part      the index into PARTS (0 or 1, the doors)
/// f32 angle     the hinge angle, rad (the hinged body's +0x3FC)
/// ```
///
/// An entry is an attached door: its hinged body's slot (`CCar` +0x480 + 4 i) is still set. A detached door
/// leaves the record, and the viewer keeps drawing it by its DET1 body.
pub const DOR1: [u8; 4] = *b"DOR1";
/// The bytes of one door-angle entry.
pub const DOOR_ENTRY_BYTES: usize = 8;

/// One attached door's hinge angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoorAngle {
    pub part: u32,
    pub angle: f32,
}

/// The record with `angles` as its entries.
pub fn door_angles_to_bytes(angles: &[DoorAngle]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(DOOR_ENTRY_BYTES * angles.len());
    for angle in angles {
        payload.extend_from_slice(&angle.part.to_le_bytes());
        payload.extend_from_slice(&angle.angle.to_le_bytes());
    }
    crate::live::record(DOR1, &payload)
}

/// The entries of a door-angles record's payload (the bytes after its tag and length).
pub fn parse_door_angles(payload: &[u8]) -> io::Result<Vec<DoorAngle>> {
    if !payload.len().is_multiple_of(DOOR_ENTRY_BYTES) {
        return Err(invalid(format!("{} bytes of door angles", payload.len())));
    }
    payload
        .chunks_exact(DOOR_ENTRY_BYTES)
        .map(|entry| {
            let part = word(entry, 0);
            if part as usize >= PARTS.len() {
                return Err(invalid(format!("door angle for part {part}")));
            }
            Ok(DoorAngle { part, angle: f32::from_bits(word(entry, 1)) })
        })
        .collect()
}

/// The record with `poses` as its entries.
pub fn to_bytes(poses: &[PartPose]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(ENTRY_BYTES * poses.len());
    for pose in poses {
        payload.extend_from_slice(&pose.part.to_le_bytes());
        for value in pose.position.into_iter().chain(pose.orientation) {
            payload.extend_from_slice(&value.to_le_bytes());
        }
    }
    crate::live::record(DET1, &payload)
}

/// The entries of a record's payload (the bytes after its tag and length).
pub fn parse(payload: &[u8]) -> io::Result<Vec<PartPose>> {
    if !payload.len().is_multiple_of(ENTRY_BYTES) {
        return Err(invalid(format!("{} bytes of detached parts", payload.len())));
    }
    payload
        .chunks_exact(ENTRY_BYTES)
        .map(|entry| {
            let float = |i: usize| f32::from_bits(word(entry, i));
            let part = word(entry, 0);
            if part as usize >= PARTS.len() {
                return Err(invalid(format!("detached part {part}")));
            }
            Ok(PartPose {
                part,
                position: [float(1), float(2), float(3)],
                orientation: [float(4), float(5), float(6), float(7)],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let poses = [
            PartPose { part: 4, position: [1.0, -2.5, 0.25], orientation: [0.0, 0.0, 0.0, 1.0] },
            PartPose { part: 0, position: [0.0; 3], orientation: [0.5, 0.5, 0.5, 0.5] },
        ];
        let bytes = to_bytes(&poses);
        assert_eq!(&bytes[..4], b"DET1");
        assert_eq!(bytes.len(), 8 + 2 * ENTRY_BYTES);
        assert_eq!(parse(&bytes[8..]).unwrap(), poses);
        assert_eq!(parse(&[]).unwrap(), []);
    }

    #[test]
    fn bad_payloads_are_errors() {
        assert!(parse(&[0; 31]).is_err());
        let mut bytes = to_bytes(&[PartPose { part: 0, position: [0.0; 3], orientation: [0.0; 4] }]);
        bytes[8] = PARTS.len() as u8;
        assert!(parse(&bytes[8..]).is_err());
    }

    #[test]
    fn part_names_index_the_record() {
        assert_eq!(part_index("e_bumper_f"), Some(4));
        assert_eq!(part_index("e_body"), None);
    }

    #[test]
    fn door_angles_round_trip() {
        let angles = [DoorAngle { part: 0, angle: 0.01 }, DoorAngle { part: 1, angle: -0.3957 }];
        let bytes = door_angles_to_bytes(&angles);
        assert_eq!(&bytes[..4], b"DOR1");
        assert_eq!(bytes.len(), 8 + 2 * DOOR_ENTRY_BYTES);
        assert_eq!(parse_door_angles(&bytes[8..]).unwrap(), angles);
        assert_eq!(parse_door_angles(&[]).unwrap(), []);
    }

    #[test]
    fn bad_door_angles_are_errors() {
        assert!(parse_door_angles(&[0; 7]).is_err());
        let mut bytes = door_angles_to_bytes(&[DoorAngle { part: 0, angle: 0.0 }]);
        bytes[8] = PARTS.len() as u8;
        assert!(parse_door_angles(&bytes[8..]).is_err());
    }
}
