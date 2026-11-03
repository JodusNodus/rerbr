//! The four physics files the RSF plugin `Plugins/rbr_rx.dll` builds from the BTB sources, byte-identical to its
//! `CACHE/` output. Layouts below were read off the 19 cached folders of the local install (see
//! `notes/btb.md`); every builder's output parses with the matching reader (`trk`, `mat`,
//! `markers`/`dls`, `lbs`).
//!
//! - `trk`: one `(3, 0x14)` driveline chunk (the first seven `K<i>` values as `f32`, the two `u16`s zero) and one
//!   `(3, 0x16)` chunk whose payload is a `u32` mesh count plus the `ksh.ksh` bytes ([`crate::ksh`]: one mesh per
//!   `CollisionModel` 1/2 object). [`trk_from`] builds it from the track alone; four cached folders build it empty.
//! - `mat`: nine entries (`dry`/`damp`/`wet` by `new`/`normal`/`worn`, 11 characters, space padded) with identical
//!   runs: `[MAPS]` count runs of 16x16 cells each, in `data0..data15` order.
//! - `dls`: a fixed 299-byte `MINAATAD` template (version 18) holding the first `count` pace notes as the
//!   `Driveline` set's marker keys; the rest (offsets aside) is constant across all cached folders.
//! - `lbs`: a `(2, 11)` start chunk (identity matrix with the spawn translation, the position twice, `(0, angleZ,
//!   0)` rotation), then empty `(2, 10)` and `(1, 0)` chunks.

use crate::track::{DrivelinePoint, PaceNote, Spawn, Track};

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn chunk(out: &mut Vec<u8>, kind: (u32, u32), payload: &[u8]) {
    put_u32(out, 8);
    put_u32(out, kind.0);
    put_u32(out, kind.1);
    put_u32(out, payload.len() as u32);
    out.extend_from_slice(payload);
}

/// The `(3, 0x14)` driveline chunk plus the `(3, 0x16)` collision chunk (`ksh_count` meshes in `ksh`, the raw
/// `ksh.ksh` mesh bytes without their count word). Parses with [`rbr_formats::trk::parse`].
pub fn trk(points: &[DrivelinePoint], ksh_count: u32, ksh: &[u8]) -> Vec<u8> {
    let mut driveline = Vec::with_capacity(4 + 32 * points.len());
    put_u32(&mut driveline, points.len() as u32);
    for p in points {
        for v in p.position {
            put_f32(&mut driveline, v as f32);
        }
        for v in p.tangent {
            put_f32(&mut driveline, v as f32);
        }
        put_f32(&mut driveline, p.distance as f32);
        put_u16(&mut driveline, 0);
        put_u16(&mut driveline, 0);
    }
    let mut collision = Vec::with_capacity(4 + ksh.len());
    put_u32(&mut collision, ksh_count);
    collision.extend_from_slice(ksh);
    let mut out = Vec::with_capacity(16 + driveline.len() + 16 + collision.len());
    chunk(&mut out, (3, 0x14), &driveline);
    chunk(&mut out, (3, 0x16), &collision);
    out
}

/// The whole `.trk` of a stage from its own files: the driveline and [`crate::ksh::build`]'s collision meshes.
pub fn trk_from(track: &Track, math: crate::ksh::Math) -> Result<Vec<u8>, String> {
    let (count, ksh) = crate::ksh::build(track, math)?;
    Ok(trk(&track.driveline()?.points, count, &ksh))
}

/// The 11-character `.mat` entry names, space padded, in file order.
pub const MAT_NAMES: [&str; 9] = [
    "dry new    ",
    "dry normal ",
    "dry worn   ",
    "damp new   ",
    "damp normal",
    "damp worn  ",
    "wet new    ",
    "wet normal ",
    "wet worn   ",
];

/// The `.mat` file: the nine entries with identical runs, one 264-byte run per surface map. Parses with
/// [`rbr_formats::mat::parse`].
pub fn mat(maps: &[[u8; 256]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 9 * (12 + 4 + maps.len() * 264));
    put_u32(&mut out, MAT_NAMES.len() as u32);
    for name in MAT_NAMES {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        put_u32(&mut out, maps.len() as u32);
        for cells in maps {
            put_u32(&mut out, 16);
            put_u32(&mut out, 16);
            out.extend_from_slice(cells);
        }
    }
    out
}

/// The fixed `.dls` template words: table 0's trigger-data record, its channel record, the replay-stream words
/// of table 2 and table 3's camera record. All constant across the cached folders; meanings unknown, kept raw.
const DLS_TRIGGER_DATA: [u32; 2] = [3333, 13];
const DLS_CHANNELS: [u32; 6] = [0, 0, 0, 0, 1125515264, 1];
const DLS_TABLE_2: [u32; 14] =
    [3333, 3333, 0, 0, 1084227584, 1112014848, 1073741824, 0, 0, 0, 1065353216, 1065353216, 1065353216, 1];
const DLS_TABLE_3: [u32; 6] = [3333, 0xffff_ffff, 1092616192, 0, 0, 1065353216];

/// The `.dls` file with the first `notes.len()` pace notes as the `Driveline` set's marker keys (`tag`, `flag`,
/// `f32` distance). Parses with [`rbr_formats::dls::parse`] and [`rbr_formats::markers::parse`]..
pub fn dls(notes: &[PaceNote]) -> Vec<u8> {
    let base = 168 + 12 * notes.len() as u32;
    let mut out = Vec::with_capacity(base as usize + 131);
    out.extend_from_slice(b"MINAATAD");
    put_u32(&mut out, 18);
    put_u32(&mut out, 0);
    // Table offsets: animation sets, track groups (empty), table 2 (the replay stream's words), cameras, two
    // empty tables, sound spheres (empty), the name block, no finish zone, trigger names (empty).
    for offset in [56, base + 60, base, base + 64, base + 92, base + 96, base + 100, base + 108, u32::MAX, base + 104] {
        put_u32(&mut out, offset);
    }
    // Table 0: one set, its pair pointing at the record below.
    put_u32(&mut out, 1);
    put_u32(&mut out, 68);
    put_u32(&mut out, 68);
    // The 17-word record: name offset 0 (`Driveline`), the array counts and offsets. Counts 1 and 2 name the two
    // fixed arrays; count 6 is the key count, read from the key array at 168. The rest are empty, sharing offsets.
    for word in [0, 1, 2, 0, 0, 0, notes.len() as u32, 0, 0, 136, 144, 168, 168, 168, 168, 136, base] {
        put_u32(&mut out, word);
    }
    for word in DLS_TRIGGER_DATA {
        put_u32(&mut out, word);
    }
    for word in DLS_CHANNELS {
        put_u32(&mut out, word);
    }
    for note in notes {
        put_u32(&mut out, note.tag);
        put_u32(&mut out, note.flag);
        put_f32(&mut out, note.distance as f32);
    }
    // Table 2 (one trigger record) doubles as the record's replay stream; table 1 is empty.
    put_u32(&mut out, 1);
    for word in DLS_TABLE_2 {
        put_u32(&mut out, word);
    }
    put_u32(&mut out, 0);
    // Table 3 (one camera), empty tables 4, 5, 6 and 9, then the name block.
    put_u32(&mut out, 1);
    for word in DLS_TABLE_3 {
        put_u32(&mut out, word);
    }
    for _ in 0..4 {
        put_u32(&mut out, 0);
    }
    put_u32(&mut out, 19);
    out.extend_from_slice(b"Driveline\0");
    out.extend_from_slice(b"299 300\0\0");
    out
}

/// The `.lbs` file: the `(2, 11)` start chunk (identity matrix with the spawn translation, the position twice, the
/// `(0, angleZ, 0)` rotation), then the empty `(2, 10)` visibility and `(1, 0)` geometry chunks. Parses with
/// [`rbr_formats::lbs::parse`].
pub fn lbs(spawn: &Spawn) -> Vec<u8> {
    let translation = [spawn.matrix[12] as f32, spawn.matrix[13] as f32, spawn.matrix[14] as f32];
    let mut start = Vec::with_capacity(88);
    for v in [1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        put_f32(&mut start, v);
    }
    for v in translation {
        put_f32(&mut start, v);
    }
    put_f32(&mut start, 0.0);
    for v in translation {
        put_f32(&mut start, v);
    }
    put_f32(&mut start, 0.0);
    put_f32(&mut start, spawn.angles[2] as f32);
    put_f32(&mut start, 0.0);
    let mut out = Vec::with_capacity(144);
    chunk(&mut out, (2, 11), &start);
    chunk(&mut out, (2, 10), &[0, 0, 0, 0]);
    chunk(&mut out, (1, 0), &[0, 0, 0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mat_names_are_eleven_chars() {
        for name in MAT_NAMES {
            assert_eq!(name.len(), 11, "{name:?}");
        }
    }

    #[test]
    fn empty_dls_has_the_cached_length() {
        // No notes: the 299-byte `.dls` of the `notes/btb.md` layout: 168 head bytes, no keys, 131 tail bytes.
        let file = dls(&[]);
        assert_eq!(file.len(), 299);
        assert_eq!(&file[file.len() - 19..file.len() - 9], b"Driveline\0");
        assert_eq!(&file[file.len() - 9..], b"299 300\0\0");
    }

    #[test]
    fn empty_builds_have_the_cached_lengths() {
        // No notes, no maps: 299-byte `.dls`, one empty `.mat` run set, two framed `.trk` chunks, 144-byte `.lbs`.
        assert_eq!(dls(&[]).len(), 299);
        assert_eq!(mat(&[]).len(), 4 + 9 * (12 + 4));
        let spawn = Spawn { matrix: [0.0; 16], angles: [0.0; 3] };
        assert_eq!(lbs(&spawn).len(), 144);
        let file = trk(&[], 0, &[]);
        assert_eq!(file.len(), 16 + 4 + 16 + 4);
    }
}
