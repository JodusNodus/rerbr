//! A recording's playback on the runner: each step's control emitted, then `StepForward`.

use rbr_physics::mem::Mem;
use rbr_recording::CONTROL_BYTES;

use super::heap::RunGuard;
use super::image::{app_state, CAR_PTR, LOAD_STATE};
use super::{Runner, EMIT, EMIT_OFFSET, SIGNALS, STEP_FORWARD};
use crate::car_body::UPDATE_SEED;
use crate::{Regs, Return};
use rbr_physics::fpu::Precision;

/// One playback step from the call slot `slot` (`StepForward`'s return address slot, `regs` the other registers):
/// the control at `control` (`None` for the image's own step, whose control was emitted before it) through NGP's
/// control input ([`emit`]), then `StepForward(1)`; its calls are host calls from `slot`.
pub fn step(runner: &Runner, regs: Regs, slot: u32, control: Option<u32>, substeps: u32) -> Return {
    let _run = RunGuard::new(&runner.run_depth);
    let _ngp = crate::ngp::Scope::new(runner.ngp_base);
    // The runner steps NGP physics only (production entries need NGP): the load's settle steps
    // run on the game's thread, at D3D's control word `0x7f`; the race's steps run on NGP's thread, at `0x27f`
    // (`notes/ngp.md`, "x87 precision").
    let pc53 = app_state(&runner.raw) != LOAD_STATE;
    rbr_physics::fpu::set_precision(if pc53 { Precision::Pc53 } else { Precision::Pc24 });
    if let Some(control) = control {
        emit(runner, regs, slot, control);
    }
    runner.raw.clone().set_u32(slot + 4, substeps);
    crate::call_memo::new_step();
    runner.enter(STEP_FORWARD, Regs { esp: slot, ..regs })
}

/// [`step`]'s `Emit` of the payload at `control`: NGP hands the payload to the player car's control input itself;
/// the car has no connection on the emit, and NGP's walk of the other listeners reaches no physics (`notes/ngp.md`,
/// "Controls under NGP").
pub(super) fn emit(runner: &Runner, regs: Regs, slot: u32, control: u32) {
    let at = runner.raw.u32(SIGNALS) + EMIT_OFFSET;
    let entry = Regs { ecx: at, esp: slot, ..regs };
    let car = runner.raw.u32(CAR_PTR);
    runner.with_frame(EMIT, &entry, || runner.host().call(UPDATE_SEED, car, &[control]));
}

/// The car's typed state held in typed storage over a step: lifted unless it is held already ([`Runner::lift_typed`]);
/// when it drops, marked changed and, on a stop, lowered and dropped.
struct HeldTyped<'a>(&'a Runner);

impl<'a> HeldTyped<'a> {
    fn lift(runner: &'a Runner) -> Self {
        runner.lift_typed();
        HeldTyped(runner)
    }
}

impl Drop for HeldTyped<'_> {
    fn drop(&mut self) {
        self.0.held_changed();
        if std::thread::panicking() {
            self.0.lower_typed();
        }
    }
}

/// The return address of the driver's `StepForward` call (`replay.py`'s `SENTINEL`).
pub const SENTINEL: u32 = 0xfeed_0000;

const FRAME: u32 = rbr_physics::car_body::transform::FRAME;

/// An image's playback: every step enters `StepForward` from the image's call slot with its registers, and a step's
/// control payload is written at the bottom of the image's stack.
pub struct Playback {
    pub runner: Runner,
    regs: Regs,
    slot: u32,
    control_at: u32,
}

impl Playback {
    /// The playback of a memory image built by the boot (`rbr-standalone --boot`): `StepForward` entered from
    /// `slot` with `regs`, the control payload written at `control_at`.
    pub fn from_image(mem: Mem, regs: Regs, slot: u32, control_at: u32, keep_going: bool) -> Self {
        let ngp_base = crate::ngp::base_in(&mem);
        Playback { runner: Runner { ngp_base, ..Runner::new(mem, keep_going) }, regs, slot, control_at }
    }

    /// [`step`] with the payload `control` (`None` for the image's own step). A stop panics as [`Runner::enter`].
    /// The car's typed state is in typed storage over the step: lifted before it unless it is held already, and held
    /// after it (lowered on a stop); readers between steps see it through [`Runner::image`]. The payload and the return slot are the step's own stack words.
    pub fn step(&self, control: Option<&[u8; CONTROL_BYTES]>, substeps: u32) -> Return {
        let mut mem = self.runner.raw.clone();
        let control = control.map(|c| {
            mem.map(self.control_at, c);
            self.control_at
        });
        mem.set_u32(self.slot, SENTINEL);
        let _typed = HeldTyped::lift(&self.runner);
        step(&self.runner, self.regs, self.slot, control, substeps)
    }

    /// `NotifyA`'s vector (`p[4..8]` of `CSigCarToCarInputFilter`): `MB560` of the car's vector at +0x220, as `CCar::Vf28x`
    /// (0x4ea020) walks it before the emit at +0x860. Called between steps, from the step's entry frame, with the walk's
    /// output in the scratch below it. Read-only on the car; the walk's node hooks store what a step stores. On the
    /// typed state when it is held across steps.
    pub fn notify_a_vector(&self) -> [f32; 4] {
        let _run = RunGuard::new(&self.runner.run_depth);
        let mem = self.runner.raw.clone();
        let entry = Regs { esp: self.slot, ..self.regs };
        let scratch = (self.slot - 0x100) & !0xf;
        let (car, frame) = (mem.u32(CAR_PTR), mem.u32(FRAME));
        // As values (`entities::op`): none of the call's words, which the next step's frames write before any read
        // (residue audit).
        self.runner.with_frame(STEP_FORWARD, &entry, || {
            let args = [car + 0x220, car, scratch];
            let [a, b, c] = args;
            crate::entities::op(self.runner.host(), 0, frame, crate::entities::Op::Mb560 { a, b, c });
        });
        self.runner.held_changed();
        mem.f32s(scratch)
    }

    /// Only the `Emit` of [`Playback::step`]: the state the next step starts from. On the typed state when it is held across steps.
    pub fn emit(&self, control: &[u8; CONTROL_BYTES]) {
        let _run = RunGuard::new(&self.runner.run_depth);
        self.runner.raw.clone().map(self.control_at, control);
        emit(&self.runner, self.regs, self.slot, self.control_at);
        self.runner.held_changed();
    }
}
