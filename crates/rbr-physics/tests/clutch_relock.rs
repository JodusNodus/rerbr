//! The clutch worker's relock under an engine model (ours, experimental): a slipping clutch relocks only when it can
//! hold, else it keeps slipping in the real direction. With no model (`GearHost` default) NGP's worker is unchanged.

use rbr_physics::engine_driveline::drive_worker::{clutch_torque, GearHost};
use rbr_physics::engine_driveline::layout::with_gear;
use rbr_physics::mem::Mem;

const THIS: u32 = 0x1000_0000;
const DEF: u32 = 0x2000_0000;
const STATE: u32 = 0x3000_0000;
const OBJ: u32 = 0x4000_0000;
const WORLD: u32 = 0x5000_0000;
const OPTIONS: u32 = 0x6000_0000;

const SLIP_GAIN_A: f32 = 0.131;
const D: f32 = 1.0 / 720.0;
const C: f32 = 8.821427;

struct Host(bool);

impl GearHost for Host {
    fn car_signal_word(&mut self, _: u32) -> u32 {
        panic!("the slipping branch makes no calls")
    }
    fn emit_gear_change(&mut self, _: [u32; 4]) {
        panic!("the slipping branch makes no calls")
    }
    fn clutch_holds_only_with_capacity(&self) -> bool {
        self.0
    }
}

/// What the worker returns and leaves: the value written, the engine speed, the sign word and the trigger.
struct After {
    out: Option<f32>,
    engine: f32,
    word: i32,
    trigger: u32,
}

/// One call in the slipping branch: the sign word -1 (the engine's torque `a` negative), the engine at `engine`, the
/// clutch-engaged speed `locked`, the clutch torque limit `b`.
fn call(model: bool, engine: f32, locked: f32, a: f32, b: f32) -> After {
    let mut mem = Mem::new();
    for (at, len) in [(THIS, 0x400), (DEF, 0x2000), (STATE, 0x2000), (OBJ, 0x400), (WORLD, 0x100), (OPTIONS, 0x16100)] {
        mem.map(at, &vec![0; len]);
    }
    mem.map(0x0165_9b70, &WORLD.to_le_bytes());
    mem.set_u32(WORLD + 0x24, OPTIONS);
    mem.set_u32(THIS + 4, DEF);
    mem.set_u32(THIS + 8, STATE);
    mem.set_u32(THIS + 0x170, OBJ);
    mem.set_f32(DEF + 0x694, 1.8095238);
    mem.set_u32(STATE + 0x55c, 1);
    mem.set_u32(STATE + 0x368, (-1i32) as u32);
    mem.set_u32(STATE + 0x578, 0);
    mem.set_f32(STATE + 0x45c, engine);
    mem.set_f32(STATE + 0x384, locked / C);
    mem.set_f32(DEF + 0x664, SLIP_GAIN_A);
    mem.set_f32(DEF + 0x674, 0.017);
    let out = with_gear(&mut mem, THIS, |w, g| clutch_torque(w, g, &mut Host(model), a, [b, C, D]));
    After { out, engine: mem.f32(STATE + 0x45c), word: mem.u32(STATE + 0x368) as i32, trigger: mem.u32(STATE + 0x578) }
}

#[test]
fn an_open_clutch_does_not_relock_to_a_falling_drivetrain() {
    // Engine at 88.78, wheels' side 87.92, net torque -6 N·m, no clutch capacity.
    let free = 88.78 + -6.0 / SLIP_GAIN_A * D;
    let held = call(true, 88.78, 87.92, -6.0, 0.0);
    assert!((held.engine - free).abs() < 1e-3, "{}", held.engine);
    assert_eq!((held.word, held.trigger, held.out), (1, 0, Some(0.0)));
    // NGP's worker: the engine goes to the locked speed.
    let ngp = call(false, 88.78, 87.92, -6.0, 0.0);
    assert!((ngp.engine - 87.92).abs() < 1e-3, "{}", ngp.engine);
    assert_eq!((ngp.word, ngp.trigger), (0, 1));
}

#[test]
fn a_clutch_with_capacity_relocks_as_before() {
    let (model, ngp) = (call(true, 88.78, 88.75, -6.0, 600.0), call(false, 88.78, 88.75, -6.0, 600.0));
    assert!((model.engine - 88.75).abs() < 1e-3, "{}", model.engine);
    assert_eq!((model.engine.to_bits(), model.word, model.trigger), (ngp.engine.to_bits(), ngp.word, ngp.trigger));
    assert_eq!((model.word, model.trigger), (0, 1));
}
