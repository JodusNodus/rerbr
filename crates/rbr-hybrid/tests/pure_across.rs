//! The typed state held across steps (`Pure::typed_across_steps`) against lifted before and lowered after each step:
//! two playbacks of one snapshot over its recording's controls, with the between-step runs on the held state
//! (`notify_a_vector`, the damper reset), equal byte for byte in every region of the image, the stack from the entry
//! slot up and the allocator's blocks every `EVERY` steps (the held side lowered only then, through `Pure::image`), at
//! the end, after a restore and a step after it. The archive's snapshots are 1.02 ones, where `StepForward` declines
//! (NGP's port; the run keeps going), so this covers the lift, the runs between steps, the lowers and snapshot and
//! restore; the steps themselves are the standalone's drive with the switch on. Needs `RBR_SNAPS` and
//! `RBR_RECORDINGS`; skipped without.

use std::path::PathBuf;

use rbr_hybrid::pure::{Playback, ALLOCATOR_HEAP_BASE, ALLOCATOR_HEAP_END, CAR_PTR};
use rbr_physics::mem::Mem;
use rbr_physics::snapshot::Snapshot;

const SNAPSHOT: &str = "vcu-cov.snap-1000.bin";
const DRIVE: &str = "drive-tanner-corolla.bin";
const STEPS: usize = 60;
const EVERY: usize = 20;

fn inputs() -> Option<(PathBuf, PathBuf)> {
    let snap = PathBuf::from(std::env::var_os("RBR_SNAPS")?).join(SNAPSHOT);
    let drive = PathBuf::from(std::env::var_os("RBR_RECORDINGS")?).join(DRIVE);
    (snap.exists() && drive.exists()).then_some((snap, drive))
}

/// The first byte where `a` and `b` differ over `lo..hi` (both unmapped counts as equal).
fn first_difference(a: &Mem, b: &Mem, lo: u32, hi: u32) -> Option<u32> {
    const CHUNK: u32 = 0x1000;
    let mut at = lo;
    while at < hi {
        let len = CHUNK.min(hi - at) as usize;
        let (x, y) = (a.try_read(at, len), b.try_read(at, len));
        if x != y {
            let k = match (&x, &y) {
                (Some(x), Some(y)) => x.iter().zip(y).position(|(p, q)| p != q).unwrap_or(0),
                _ => 0,
            };
            return Some(at + k as u32);
        }
        at += len as u32;
    }
    None
}

fn assert_same(snap: &Snapshot, a: &Playback, b: &Playback, when: &str) {
    let h = snap.metadata();
    // The stack below the step's entry slot is left out: its words are dead between steps, and a run on the held state
    // leaves out call words a per-call run writes (`Host::call_in_place`).
    let dead = (h.stack_lo, h.registers.esp - 4);
    let ranges = snap
        .regions()
        .iter()
        .map(|r| (r.base, r.base + r.size))
        .chain([(h.stack_lo, h.stack_lo + h.stack_len), (ALLOCATOR_HEAP_BASE, ALLOCATOR_HEAP_END)])
        .flat_map(|(lo, hi)| [(lo, hi.min(dead.0)), (lo.max(dead.1), hi)])
        .filter(|(lo, hi)| lo < hi);
    for (lo, hi) in ranges {
        if let Some(at) = first_difference(a.pure.image(), b.pure.image(), lo, hi) {
            panic!("{when}: the images differ at {at:#x}");
        }
    }
}

#[test]
fn held_across_steps_is_the_per_step_lift_byte_for_byte() {
    let Some((snap_path, drive)) = inputs() else {
        eprintln!("RBR_SNAPS or RBR_RECORDINGS unset; skipped");
        return;
    };
    let snap = Snapshot::open(&snap_path).unwrap();
    let bytes = std::fs::read(drive).unwrap();
    let frames = rbr_recording::read_all(&bytes).unwrap();
    let h = snap.metadata();
    let first = frames.iter().position(|f| f.pre == h.step).expect("a frame at the snapshot's step");
    let [mut a, mut b] = [false, true].map(|across| {
        let mut playback = Playback::new(&snap, true);
        playback.pure.typed_across_steps = across;
        playback
    });
    let car = a.pure.image().u32(CAR_PTR);
    let struts: [u32; 4] = std::array::from_fn(|k| {
        let wheel = a.pure.image().u32(car + rbr_recording::sound::CAR_WHEELS + 4 * k as u32);
        wheel + rbr_recording::sound::WHEEL_STRUT
    });
    let mut start = None;
    for (k, frame) in frames[first..].iter().take(STEPS).enumerate() {
        let control = (k > 0).then_some(&frame.control);
        let substeps = if k == 0 { h.substeps } else { 1 };
        for p in [&a, &b] {
            p.step(control, substeps);
        }
        assert_eq!(a.notify_a_vector().map(f32::to_bits), b.notify_a_vector().map(f32::to_bits), "step {k}");
        if k % 7 == 3 {
            for p in [&a, &b] {
                p.pure.reset_strut_dampers(struts);
            }
            assert_eq!(format!("{:?}", a.pure.take_damper_signals()), format!("{:?}", b.pure.take_damper_signals()));
        }
        if k == 1 {
            start = Some([a.pure.snapshot(), b.pure.snapshot()]);
        }
        if (k + 1) % EVERY == 0 {
            assert_same(&snap, &a, &b, &format!("after step {k}"));
        }
    }
    assert_same(&snap, &a, &b, "at the end");
    let [sa, sb] = start.unwrap();
    a.pure.restore(&sa);
    b.pure.restore(&sb);
    assert_same(&snap, &a, &b, "restored");
    for p in [&a, &b] {
        p.step(Some(&frames[first + 2].control), 1);
    }
    assert_same(&snap, &a, &b, "a step after the restore");
}

/// With the `poison` feature, while the state held across steps is stale a read of a held word through the image's
/// memory fails (a clone kept from before the step); after `Pure::image` lowers it, it reads.
#[cfg(feature = "poison")]
#[test]
fn a_stale_read_of_a_held_word_fails() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let Some((snap_path, _)) = inputs() else {
        eprintln!("RBR_SNAPS or RBR_RECORDINGS unset; skipped");
        return;
    };
    let snap = Snapshot::open(&snap_path).unwrap();
    let mut playback = Playback::new(&snap, true);
    playback.pure.typed_across_steps = true;
    let kept = playback.pure.image().clone();
    let car = kept.u32(CAR_PTR);
    let held = rbr_physics::rigid_body::layout::CarSolids::lift(&kept, car).owned()[0].0.start;
    playback.step(None, snap.metadata().substeps);
    assert!(catch_unwind(AssertUnwindSafe(|| kept.u32(held))).is_err());
    let lowered = playback.pure.image().u32(held);
    assert_eq!(kept.u32(held), lowered);
}
