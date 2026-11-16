//! The driver assists and the retire paths (`rbr_physics::car_body::assist`).

use crate::stage::on_stage;
use rbr_physics::car_body::assist::{AssistCalls, RetireCalls};
use rbr_physics::car_body::probe::ProbeCalls;
use rbr_physics::car_body::store::Cell;
use rbr_physics::car_body::{self as car};
use rbr_physics::mem::Mem;
use rbr_physics::ntweak;

use super::car_calls::{car_calls, car_calls_at, Car};
use super::seed::SLOT_S158;
use super::Frame;
use crate::car_body::on_car;
use crate::engine_driveline::SIGNALS;
use crate::enter;
use crate::pure::OUTSIDE_ASSISTANCE_EMIT;
use crate::vcu::SET_DIRTY;
use crate::{Ctx, Decline, Host, Regs, Return};

/// The calls `Assist8DF0` makes out of its own body (`assist.rs`): the lane test, the outside emit with its code word in
/// scratch, S158 through the car's vtable, `UpdateSeed` on the seed block, and the state's `SetDirty`.
impl AssistCalls for Car<'_> {
    fn lane_test(&mut self, car: u32) -> u32 {
        self.host.call(LANE_TEST, car, &[]).eax
    }

    fn emit_outside(&mut self, code_at: u32) {
        let emitter = self.mem.u32(SIGNALS) + OUTSIDE_EMITTER;
        self.host.call(OUTSIDE_ASSISTANCE_EMIT, emitter, &[code_at]);
    }

    fn reset_car(&mut self, car: u32) {
        self.slot(car, SLOT_S158, &[]);
    }

    fn apply_control(&mut self, car: u32, block: u32) {
        // The block's words +0 and +0x1c are not written.
        crate::fill_residue(&mut self.mem, block, 1);
        crate::fill_residue(&mut self.mem, block + 0x1c, 1);
        super::seed::update_seed_at(self.host, &mut self.mem, 0, car, block);
    }

    /// On the held car `SetDirty`'s one write, the cell header's dirty word (a header stays on the image), here
    /// ([`Host::ran_direct`]); else the host.
    fn set_dirty(&mut self, state: u32) {
        let held = car::layout::cell_at(self.host.typed().car.own.at, Cell::FfbTorque) == state;
        if !held {
            self.host.call(SET_DIRTY, state, &[]);
            return;
        }
        self.host.ran_direct();
        ntweak::set_dirty(&mut self.mem, state);
    }
}

/// The calls `E34_RetireA` makes: the retire emit with its code word's address, and `Assist8DF0` on the car.
impl RetireCalls for Car<'_> {
    fn emit_retire(&mut self, code_at: u32) {
        let emitter = self.mem.u32(SIGNALS) + RETIRE_EMITTER;
        let target = crate::ngp::Call::Retire.address();
        self.host.call(target, emitter, &[code_at]);
    }

    fn assist(&mut self, car: u32, a: u32, b: u32, c: u32) {
        assist_at(self.host, &mut self.mem, 0, car, [a, b, c]);
    }
}

const LANE_TEST: u32 = 0x4E7910;
/// The retire and outside-assistance emitters' offsets on the signal global (the asm's `add ecx, ...`).
const RETIRE_EMITTER: u32 = 0xABB0;
const OUTSIDE_EMITTER: u32 = 0xD378;

/// thiscall: `CCar::Test7910`, the lane test (`layout::lane_test`); eax is the verdict. The original's `LerpAt` traps past
/// the table's count, so the port declines there and the original runs.
pub(super) fn lane_test(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let car = r.ecx;
    let mut calls = car_calls(ctx, r);
    let image = ctx.mem.clone();
    let verdict = on_stage(ctx.host, &image, |line| {
        on_car(ctx, |m| car::layout::lane_test(m, line, car, |v| ProbeCalls::to_world(&mut calls, car, v)))
    });
    match verdict {
        Some(eax) => Ok(Return::void(eax, 0)),
        None => Err(Decline::Unsupported("LerpAt's count check (the original traps past the count)")),
    }
}

/// `CCar::Assist8DF0` (0x4e8df0, thiscall `(a, b, c)`) on `car`, as the host's call at `sp`: the outside-assistance worker
/// (`layout::outside_assistance`).
pub(super) fn assist_at(host: &dyn Host, mem: &mut Mem, sp: u32, car: u32, words: [u32; 3]) {
    let esp = enter(host, mem, sp, &words);
    let frame = Frame::new(host, esp);
    let mut calls = car_calls_at(&frame, mem, esp);
    let [a, b, c] = words;
    super::on_car_in(&frame, mem, |m| car::layout::outside_assistance(&mut calls, m, car, esp, a, b, c));
}

/// thiscall `()`: `CCar::Vf158x` (slot 86), the car's slot 0 then the tail-call into `CSolid::E43Vf158`.
pub(super) fn vf158x(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    struct Calls<'a> {
        host: &'a dyn Host,
    }
    impl car::layout::CarSoftenCalls for Calls<'_> {
        fn entity_name(&mut self, _mem: &Mem, target: u32, sp: u32, car: u32) {
            self.host.call_at(sp, target, car, &[]);
        }

        fn solid_soften(&mut self, _mem: &mut Mem, sp: u32, car: u32) -> u32 {
            self.host.call_at(sp, 0x52c030, car, &[]).eax
        }
    }
    let mut calls = Calls { host: ctx.host };
    let eax = car::layout::car_soften(&mut calls, &mut ctx.mem, r.ecx, r.esp);
    Ok(Return::void(eax, 0))
}

/// fastcall on the car (ecx), no args, `ret`: `E34_RetireA`, the retire announcer (`layout::retire_announce`).
pub(super) fn retire_a(ctx: &mut Ctx, r: &Regs) -> Result<Return, Decline> {
    let mut calls = car_calls(ctx, r);
    car::layout::retire_announce(&mut calls, &mut ctx.mem, r.ecx, r.esp);
    Ok(Return::void(0, 0))
}
