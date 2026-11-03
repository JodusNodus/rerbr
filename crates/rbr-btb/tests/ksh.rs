//! The collision meshes `rbr_btb::ksh` builds against the plugin's `CACHE/ksh.ksh` and `trk.trk` (`RBR_RSF`, skipped
//! when unset). The first word of each placement is a heap address in the cache and 0 in the build: both sides are
//! compared with those words zeroed. A cache is built with one of two D3DX libraries (`rbr_btb::D3dx`); the one that
//! reproduces it is used, and every cached folder must match with one of them.

use rbr_btb::{classic, ksh, track};
use std::path::PathBuf;

fn tracks() -> Option<Vec<track::Track>> {
    let root = PathBuf::from(std::env::var_os("RBR_RSF")?);
    let tracks: Vec<_> =
        track::scan(&root).into_iter().map(|t| t.unwrap_or_else(|(f, e)| panic!("{f:?}: {e}"))).collect();
    (!tracks.is_empty()).then_some(tracks)
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}

/// Zeroes the placement key words of `count` meshes' bytes in place.
fn zero_keys(ksh: &mut [u8]) {
    let mut p = 0;
    while p < ksh.len() {
        p += ksh[p..].iter().position(|&b| b == 0).unwrap() + 1 + 6 + 4;
        let vertices = u32_at(ksh, p);
        p += 4 + 12 * vertices;
        let faces = u32_at(ksh, p);
        p += 4 + 13 * faces;
        let clones = u32_at(ksh, p);
        p += 4;
        for _ in 0..clones {
            ksh[p..p + 4].fill(0);
            p += 44;
        }
    }
}

/// The offset of the `ksh` bytes inside a `trk.trk`: after the driveline chunk, the `(3, 0x16)` header and its count.
fn ksh_offset(trk: &[u8]) -> usize {
    let driveline = 16 + u32_at(trk, 12);
    driveline + 16 + 4
}

fn masked(mut bytes: Vec<u8>, from: usize) -> Vec<u8> {
    zero_keys(&mut bytes[from..]);
    bytes
}

fn first_difference(a: &[u8], b: &[u8]) -> String {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => format!("first difference at byte {i} (of {} and {})", a.len(), b.len()),
        None => format!("lengths {} and {}", a.len(), b.len()),
    }
}

#[test]
fn cached_collision_meshes_rebuild() {
    let Some(tracks) = tracks() else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    let (mut cached, mut matched) = (0, 0);
    for track in &tracks {
        let (Ok(want_ksh), Ok(want_trk)) =
            (std::fs::read(track.folder.join("CACHE/ksh.ksh")), std::fs::read(track.folder.join("CACHE/trk.trk")))
        else {
            continue;
        };
        cached += 1;
        let mut want_ksh = want_ksh;
        zero_keys(&mut want_ksh);
        let want_trk = masked_trk(want_trk);
        let mut report = Vec::new();
        let mut done = None;
        for math in [rbr_btb::D3dx::Windows, rbr_btb::D3dx::Wine] {
            let (count, mut got) = ksh::build(track, math).unwrap_or_else(|e| panic!("{}: {e}", track.name));
            zero_keys(&mut got);
            let trk = classic::trk_from(track, math).unwrap_or_else(|e| panic!("{}: {e}", track.name));
            let trk = masked_trk(trk);
            if got == want_ksh && trk == want_trk {
                assert_eq!(count == 0, want_ksh.is_empty(), "{}", track.name);
                done = Some(math);
                break;
            }
            report.push(format!(
                "{math:?}: ksh {}; trk {}",
                first_difference(&got, &want_ksh),
                first_difference(&trk, &want_trk)
            ));
        }
        match done {
            Some(math) => {
                matched += 1;
                eprintln!("{}: identical ({math:?})", track.name);
            }
            None => eprintln!("{}: DIFFERS\n  {}", track.name, report.join("\n  ")),
        }
    }
    eprintln!("{matched} of {cached} cached folders identical");
    assert_eq!(cached, 19, "the local install caches 19 BTB folders");
    assert_eq!(matched, cached);
}

fn masked_trk(trk: Vec<u8>) -> Vec<u8> {
    let from = ksh_offset(&trk);
    masked(trk, from)
}

#[test]
fn every_folder_builds_a_parsing_trk() {
    let Some(tracks) = tracks() else {
        eprintln!("skipping: RBR_RSF not set");
        return;
    };
    for track in &tracks {
        let trk = classic::trk_from(track, rbr_btb::D3dx::Windows).unwrap_or_else(|e| panic!("{}: {e}", track.name));
        rbr_formats::trk::parse(&trk).unwrap_or_else(|e| panic!("{}: {e}", track.name));
    }
    eprintln!("{} folders build a parsing trk", tracks.len());
}

#[test]
fn box_corners_follow_the_plugin() {
    // `colBox=0,1, 3, 0,1` reads (0, 1, 3): x half extent 0, y 1.5, z from -0.1 * 0.5 to 1.
    let corners = ksh::box_corners([0.0, 1.0, 3.0]);
    assert_eq!(corners[0].map(f32::to_bits), [(-0.0f32).to_bits(), (-1.5f32).to_bits(), (-0.05f32).to_bits()]);
    assert_eq!(corners[7], [0.0, 1.5, 1.0]);
}

#[test]
fn rsqrtss_is_the_intel_table() {
    // Values the instruction returns (i9-13900HX).
    for (x, want) in [
        (0x3f80_0000u32, 0x3f7f_f000u32),
        (0x3fa0_0000, 0x3f64_f000),
        (0x3fc0_0000, 0x3f51_0000),
        (0x3fe0_0000, 0x3f41_8000),
    ] {
        assert_eq!(ksh::rsqrtss(f32::from_bits(x)).to_bits(), want, "rsqrtss({x:#x})");
    }
}

#[test]
fn identity_matrix_is_the_identity_quaternion() {
    let m = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
    for math in [rbr_btb::D3dx::Windows, rbr_btb::D3dx::Wine] {
        let [x, y, z, w] = ksh::quaternion(&m, math);
        assert_eq!([x, y, z], [0.0; 3]);
        assert!((w - 1.0).abs() < 1e-3, "{math:?}: w {w}");
    }
}
