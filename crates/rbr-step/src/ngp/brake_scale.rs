//! The brake test's per-wheel ABS (ours, research tooling): a scale on each wheel's brake pedal pressure, set by the
//! autopilot (`--brake-abs`) on the runs of this thread. Unset (the default) the brake step reads the pedal as NGP does.

use std::cell::Cell;

thread_local! {
    static SCALE: Cell<Option<[f32; 4]>> = const { Cell::new(None) };
}

/// The scale (0-1) of each wheel's pedal pressure (FL FR RL RR) the steps on this thread take from now on (`None`:
/// the pedal as it is).
pub fn set_brake_scale(scale: Option<[f32; 4]>) {
    SCALE.set(scale);
}

/// The pedal pressure `pedal` wheel `wheel` (0-3) brakes with: scaled when a scale is set, else untouched.
pub(crate) fn scaled_pedal(wheel: u32, pedal: f32) -> f32 {
    match SCALE.get() {
        Some(scale) => pedal * scale[wheel as usize],
        None => pedal,
    }
}
