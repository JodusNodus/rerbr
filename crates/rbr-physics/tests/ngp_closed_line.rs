//! Ours: NGP's drive line search on a closed line in the image (`ngp::layout::drive_line::position_of`): the first
//! entry's tag marks it ([`rbr_formats::trk::CLOSED_LINE_WORDS`]), and the search comes in at the seam. An untagged line
//! of the same entries stops at its end, as every retail stage's does.

use rbr_physics::engine_driveline::layout::{entry_at, COUNT_AT};
use rbr_physics::math::vec::Vec4;
use rbr_physics::mem::Mem;
use rbr_physics::ngp::layout::drive_line::position_of;

const LINE: u32 = 0x0100_0000;
const SEGMENTS: u32 = 100;
const STEP: f32 = std::f32::consts::TAU / SEGMENTS as f32;

/// A circle of `SEGMENTS` segments round the origin, radius 100, the last entry the first's copy; `tag` on entry 0.
fn ring(tag: u32) -> Mem {
    let mut mem = Mem::new();
    for k in 0..=SEGMENTS {
        let a = (k % SEGMENTS) as f32 * STEP;
        let at = entry_at(LINE, k);
        Vec4 { x: 100.0 * a.cos(), y: 100.0 * a.sin(), z: 0.0, w: 1.0 }.store(&mut mem, at);
        Vec4 { x: -100.0 * STEP * a.sin(), y: 100.0 * STEP * a.cos(), z: 0.0, w: 0.0 }.store(&mut mem, at + 0x10);
        mem.set_f32(at + 0x20, k as f32 * 100.0 * STEP);
        mem.set_u32(at + 0x24, if k == 0 { tag } else { 0 });
    }
    mem.set_u32(LINE + COUNT_AT, SEGMENTS + 1);
    mem
}

/// Drives from entry 90 to entry 110 (round the seam) in steps of a third of an entry, the hint the last answer.
fn drive(mem: &Mem) -> Vec<f32> {
    let mut x = 90.0;
    (0..60)
        .map(|k| {
            let a = (90.0 + k as f32 / 3.0) * STEP;
            x = position_of(mem, LINE, [103.0 * a.cos(), 103.0 * a.sin(), 0.0], x);
            x
        })
        .collect()
}

#[test]
fn the_tagged_line_wraps_at_the_seam() {
    let [high, low] = rbr_formats::trk::CLOSED_LINE_WORDS;
    let drove = drive(&ring(u32::from(high) << 16 | u32::from(low)));
    let wraps = drove.windows(2).filter(|w| w[1] < w[0] - 50.0).count();
    assert_eq!(wraps, 1, "{drove:?}");
    assert!((drove[59] - 9.67).abs() < 0.05, "{}", drove[59]);
}

#[test]
fn the_untagged_line_stops_at_its_end() {
    let drove = drive(&ring(0));
    assert!(drove.windows(2).all(|w| w[1] >= w[0] - 0.01), "{drove:?}");
    assert!(drove[59] >= (SEGMENTS - 1) as f32);
}
