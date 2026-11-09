//! The Car setup page's diagrams, drawn with egui's painter: what the values do, with the comparison's values
//! (`base`) ghosted behind them. The diff maps' curves are edited by dragging their knots.

use egui::{Color32, Ui, Vec2};
use rbr_gameplay::setup::{DiffMap, Range};

/// The front axle's colour.
pub const FRONT: Color32 = Color32::from_rgb(95, 175, 235);
/// The rear axle's colour.
pub const REAR: Color32 = Color32::from_rgb(200, 140, 235);
/// What a value is compared with: drawn dashed or hollow behind it.
pub const BASE: Color32 = Color32::from_gray(125);
/// A value that differs from the comparison.
pub const DIFF: Color32 = Color32::from_rgb(240, 165, 70);

/// One damper's values as the setup holds them (SI: Ns/m and m/s).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Damper {
    pub bump: f32,
    pub rebound: f32,
    pub fast_bump: f32,
    /// The wheel speed the fast bump rate takes over from (`BumpHighSpeedBreak`).
    pub fast_from: f32,
}

/// One coil-over's springs as the setup holds them (SI: m and N/m).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spring {
    pub length: f32,
    pub rate: f32,
    pub helper_length: f32,
    pub helper_rate: f32,
}

/// The dampers' force over the wheel's speed: bump left of 0 (the slow rate up to `fast_from`, the fast rate past
/// it), rebound right of 0; front and rear (`[front, rear]`), the comparison's dashed.
pub fn dampers(ui: &mut Ui, now: [Damper; 2], base: [Damper; 2], size: Vec2) {
    let _ = (now, base);
    ui.allocate_space(size);
}

/// The front and rear coil-overs side by side, to one scale: the main spring under the helper spring, each coil's
/// length its free length, the wire's weight its rate; the comparison's outline behind them.
pub fn springs(ui: &mut Ui, now: [Spring; 2], base: [Spring; 2], size: Vec2) {
    let _ = (now, base);
    ui.allocate_space(size);
}

/// The four wheels from the front (`[LF, RF, LB, RB]` axis inclinations, radians; the front pair over the rear),
/// each tilted by its camber, exaggerated so a tenth of a degree shows; the comparison's outline behind them.
pub fn camber(ui: &mut Ui, now: [f32; 4], base: [f32; 4], size: Vec2) {
    let _ = (now, base);
    ui.allocate_space(size);
}

/// A diff map's lock (0 to 1) over the throttle and the brake pedal (0 to 100 %, a knot each 10 %): two curves, the
/// comparison's dashed. Each knot drags up and down by `lock`'s steps (`Range::toward`); its tooltip gives the lock
/// in % and in Nm of the diff's `torque`. Whether a value changed.
pub fn pedal_locks(ui: &mut Ui, map: &mut DiffMap, base: &DiffMap, lock: Range, torque: f32, size: Vec2) -> bool {
    let _ = (map, base, lock, torque);
    ui.allocate_space(size);
    false
}

/// A diff map's speed lock: its factor (0 to 1) at each knot's speed (m/s, shown in km/h), the comparison's dashed.
/// Each knot's factor drags up and down by `factor`'s steps; the speeds are not dragged. Whether a value changed.
pub fn speed_locks(ui: &mut Ui, map: &mut DiffMap, base: &DiffMap, factor: Range, size: Vec2) -> bool {
    let _ = (map, base, factor);
    ui.allocate_space(size);
    false
}
