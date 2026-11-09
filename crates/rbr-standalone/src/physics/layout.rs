//! Adapters for the backend's original memory layout and recorded control payload.

use rbr_gameplay::control::CarControl;
use rbr_gameplay::stage::{State, LOAD_SUBSTEPS};
use rbr_hybrid::pure::{self, Playback, CAR_PTR};
use rbr_physics::mem::Mem;
use rbr_physics::tyre::store::TyreStore;
use rbr_recording::sound::{TyreMax, CAR_WHEELS, WHEEL_TYRE_SCRATCH};
use rbr_recording::{read_f32, CONTROL_BYTES};

use super::CarState;

pub const DRIVELINE_OFFSET: usize = 0x17c;
const VELOCITY_OFFSET: usize = 0x1c0;
const POSITION_OFFSET: usize = 0x140;
const NOTIFY_A_SPEED_OFFSET: usize = 0x15f8;
/// Keep the same full read footprint as the recording adapter, including its mapped-byte checks.
const SAMPLE_BYTES: usize = NOTIFY_A_SPEED_OFFSET + 4;

pub(super) fn recorded_car(car: &[u8]) -> CarState {
    CarState {
        driveline_parameter: read_f32(car, DRIVELINE_OFFSET).expect("car +0x17c"),
        velocity: std::array::from_fn(|lane| read_f32(car, VELOCITY_OFFSET + 4 * lane).expect("car +0x1c0")),
        position: std::array::from_fn(|lane| read_f32(car, POSITION_OFFSET + 4 * lane).expect("car +0x140")),
        // Vf6cx writes -MB560[1] to +0x15f8; NotifyA reads that scalar from p[0].
        notify_a_speed: read_f32(car, NOTIFY_A_SPEED_OFFSET).expect("car +0x15f8"),
    }
}

pub(super) fn car_state(playback: &Playback) -> CarState {
    let mem = &playback.pure.mem;
    recorded_car(&mem.read(mem.u32(CAR_PTR), SAMPLE_BYTES))
}

pub(super) fn accumulate_tyre_terms(playback: &Playback, tyre_max: &mut TyreMax) {
    let mem = &playback.pure.mem;
    let wheels: [u32; 4] = std::array::from_fn(|k| mem.u32(mem.u32(CAR_PTR) + CAR_WHEELS + 4 * k as u32));
    for (scratch, term) in playback.pure.take_tyre_terms() {
        if let Some(k) = wheels.iter().position(|&w| w != 0 && w + WHEEL_TYRE_SCRATCH == scratch) {
            tyre_max.add(k, term);
        }
    }
}

/// The car's wheels' tyres, for a tyre model: each one's scratch with its type's tables, as NGP's car init left them.
pub(super) fn tyre_wheels(playback: &Playback) -> Vec<rbr_tyre::Wheel> {
    let mem = &playback.pure.mem;
    let car = mem.u32(CAR_PTR);
    let wheels = (0..4).map(|k| mem.u32(car + CAR_WHEELS + 4 * k)).filter(|&w| w != 0);
    wheels
        .map(|w| w + WHEEL_TYRE_SCRATCH)
        .map(|t| {
            // The type's surface friction words (`load_surface_input`): base and wet slope per class, design wetness.
            let def = mem.u32(t + rbr_physics::tyre::layout::DEF);
            rbr_tyre::Wheel {
                class_friction: std::array::from_fn(|k| mem.f32(def + 0x2C4 + 4 * k as u32)),
                class_wet_slope: std::array::from_fn(|k| mem.f32(def + 0x2E0 + 4 * k as u32)),
                design_wetness: mem.f32(def + 0x2DC),
                ..rbr_tyre::Wheel::new(t, &mem.load_frame(t).setup)
            }
        })
        .collect()
}

/// `CDriveLine::Sample` (0x50b180) on the physics' drive line (the object 0x1659184 points to) at `distance`: its
/// xyz.
pub(super) fn drive_line_point(playback: &Playback, distance: f32) -> [f32; 3] {
    let mem = &playback.pure.mem;
    let p = rbr_physics::engine_driveline::layout::sample(mem, mem.u32(DRIVE_LINE), distance);
    [p.x, p.y, p.z]
}

/// The global holding the drive line object (`CDriveLine`) the physics and the school's `SchoolIncident` sample
/// (`CCar::Test7910` reads it the same way).
const DRIVE_LINE: u32 = 0x165_9184;

/// The drive state's +0x570 (car +0x4b0, its +8), compared by NGP's live gear handler.
pub(super) fn drive_gear(playback: &Playback) -> u32 {
    let mem = &playback.pure.mem;
    mem.u32(mem.u32(mem.u32(CAR_PTR) + 0x4b0 + 8) + 0x570)
}

/// The car's engine (`CEngine`, car +0x690): its definition at +4.
const CAR_ENGINE_DEF: u32 = 0x694;
/// `CEngine::CDef` members after NGP's car init (`rbr_physics::ngp::layout::car_init`; key + 0xc,
/// `data/ngp-lsp-keys.md`): the torque curve's 16 knots (`Tq_xx_RPM_NGP` +0x2250 + 0x20 k, their `Tq_xx_NGP` 0x10 on).
const DEF_TORQUE_KNOTS: u32 = 0x2250;
const DEF_CYLINDER_VOLUME: u32 = 0x2650;
const DEF_INLET_PIPE_AREA: u32 = 0x26d8;
/// `EngineType_NGP`, of which the init leaves bit 0 (the turbo).
const DEF_ENGINE_TYPE: u32 = 0x26f8;
/// `TorqueCut_NGP`, the wrapper's scale during the shift cooldown (0.5 when not above 0).
const DEF_TORQUE_CUT: u32 = 0x283c;
/// The drive's definition (car +0x4b0, its +4): `EngineInertia` and `GearBoxInertiaIn` (`CReal`s at +0x658 and +0x668,
/// `data/physics-lsp-fields.md`), what the crank's speed divides by (`drive_worker`).
const CAR_DRIVE_DEF: u32 = 0x4b4;
const DRIVE_ENGINE_INERTIA: u32 = 0x664;
const DRIVE_GEARBOX_INERTIA_IN: u32 = 0x674;
/// The `CCompleteCarSetupOptions` pointer, and its engine options' `IdleRPM_NGP`, `MaxTorque_NGP` and
/// `BrakeCoefficient_NGP` (u32; the last in percent).
const SETUP_OPTIONS: u32 = 0x7e_cd04;
const OPTIONS_IDLE_RPM: u32 = 0x13c0;
const OPTIONS_MAX_TORQUE: u32 = 0x13d0;
const OPTIONS_BRAKE_COEFFICIENT: u32 = 0x13e0;
/// The VCU's definition (car +0x850) +0x188: `RPMLimit`, rpm.
const CAR_VCU_DEF: u32 = 0x850;
const VCU_RPM_LIMIT: u32 = 0x188;
/// NGP's engine mode word (`0x101767d4`, the whole `EngineType_NGP`): with bit 1 the init turned the knots' rpm to
/// rad/s.
const NGP_ENGINE_MODES: u32 = 0x1017_67d4;

/// The car's engine as NGP's car init left it, in physical units, for `car`.
pub(super) fn engine_params(playback: &Playback, car: rbr_engine::Car) -> rbr_engine::Params {
    use rbr_physics::ngp::layout::car_init::Data;
    let mem = &playback.pure.mem;
    let car_at = mem.u32(CAR_PTR);
    let def = mem.u32(car_at + CAR_ENGINE_DEF);
    let drive_def = mem.u32(car_at + CAR_DRIVE_DEF);
    let options = mem.u32(SETUP_OPTIONS);
    let rad_s = |rpm: f32| rpm * std::f32::consts::PI / 30.0;
    let converted = mem.u32(Data { base: playback.pure.ngp_base }.at(NGP_ENGINE_MODES)) & 2 != 0;
    // The first knot and those with a positive value, skipping a speed within 1e-9 of the last, as the init's idle
    // table takes them.
    let mut torque_knots: Vec<(f32, f32)> = Vec::new();
    for k in 0..16 {
        let at = def + DEF_TORQUE_KNOTS + 0x20 * k;
        let (x, y) = (mem.f32(at), mem.f32(at + 0x10));
        let x = if converted { x } else { rad_s(x) };
        if (k == 0 || y > 0.0)
            && torque_knots.last().is_none_or(|&(last, _)| (f64::from(x) - f64::from(last)).abs() >= 1e-9)
        {
            torque_knots.push((x, y));
        }
    }
    let torque_cut = mem.f32(def + DEF_TORQUE_CUT);
    rbr_engine::Params {
        torque_knots,
        max_torque: mem.u32(options + OPTIONS_MAX_TORQUE) as f32,
        rev_limit: rad_s(mem.f32(mem.u32(car_at + CAR_VCU_DEF) + VCU_RPM_LIMIT)),
        idle_speed: rad_s(mem.u32(options + OPTIONS_IDLE_RPM) as f32),
        brake_coefficient: mem.u32(options + OPTIONS_BRAKE_COEFFICIENT) as f32 / 100.0,
        turbo: mem.u32(def + DEF_ENGINE_TYPE) & 1 != 0,
        cylinder_volume: mem.f32(def + DEF_CYLINDER_VOLUME),
        inlet_area: mem.f32(def + DEF_INLET_PIPE_AREA),
        torque_cut: if torque_cut > 0.0 { torque_cut } else { 0.5 },
        inertia: mem.f32(drive_def + DRIVE_ENGINE_INERTIA) + mem.f32(drive_def + DRIVE_GEARBOX_INERTIA_IN),
        car,
    }
}

/// The drive's ratio ring's newest entry: the engine wrapper's last answer as the drive step took it
/// (`drive_step::ratio_input`).
#[cfg(test)]
pub(super) fn ratio_ring_newest(playback: &Playback) -> f32 {
    use rbr_physics::engine_driveline::layout::{ratio_ring_at, RatioRing};
    let mem = &playback.pure.mem;
    let ring = RatioRing::load(mem, ratio_ring_at(mem.u32(mem.u32(CAR_PTR) + 0x4b0 + 8)));
    ring.values[ring.index as usize]
}

/// The state goes where the control emit reads its slot (S2).
pub(super) fn set_app_state(playback: &Playback, state: State) {
    pure::set_app_state(&mut playback.pure.mem.clone(), state.value());
}

/// The 0x20-byte CSigCarControl payload, preserving word 6 when no gear is selected.
pub fn payload(control: &CarControl, word6: u32) -> [u8; CONTROL_BYTES] {
    let mut out = [0; CONTROL_BYTES];
    for (k, w) in control.words().iter().enumerate() {
        out[4 * k..4 * k + 4].copy_from_slice(&w.unwrap_or(word6).to_le_bytes());
    }
    out
}

/// The NGP boot's load budget, or the countdown captured in a validation snapshot's instruction at 0x482bc0.
pub fn load_substeps(mem: &Mem) -> u32 {
    if mem.ngp_runtime().is_some() {
        return rbr_gameplay::stage::NGP_LOAD_SUBSTEPS;
    }
    mem.try_read(0x48_2bc0, 4).map_or(LOAD_SUBSTEPS, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_uses_the_ngp_load_budget_without_executable_code() {
        use rbr_physics::ngp::layout::runtime::Runtime;
        let mut mem = Mem::new();
        assert_eq!(load_substeps(&mem), LOAD_SUBSTEPS);
        mem.set_u32(0x482bc0, rbr_gameplay::stage::NGP_LOAD_SUBSTEPS);
        assert_eq!(load_substeps(&mem), 1500);
        let mut boot = Mem::new();
        boot.set_ngp_runtime(Runtime::new([0; 3], 1));
        assert_eq!(load_substeps(&boot), load_substeps(&mem));
    }

    #[test]
    fn the_payload_takes_the_recorded_word_6_unless_a_gear_is_selected() {
        let control = CarControl { throttle: 1.0, flags: 0x10, ..CarControl::default() };
        let words = |bytes: [u8; CONTROL_BYTES]| -> Vec<u32> {
            bytes.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
        };
        assert_eq!(words(payload(&control, 0xdead)), [1.0f32.to_bits(), 0, 0, 0, 0, 0x10, 0xdead, 0]);
        let select = CarControl { flags: 0x04, gear: Some(3), ..control };
        assert_eq!(words(payload(&select, 0xdead))[6], 3);
    }

    #[test]
    fn recorded_state_preserves_signed_zero_and_nan_payloads() {
        let mut bytes = [0u8; SAMPLE_BYTES];
        let values = [
            (DRIVELINE_OFFSET, 0x8000_0000u32),
            (NOTIFY_A_SPEED_OFFSET, 0x7fc0_1234),
            (VELOCITY_OFFSET, 3.0f32.to_bits()),
            (VELOCITY_OFFSET + 4, 4.0f32.to_bits()),
            (POSITION_OFFSET, 0x8000_0000),
            (POSITION_OFFSET + 4, 0xffc0_5678),
            (POSITION_OFFSET + 8, 42.0f32.to_bits()),
        ];
        for (at, word) in values {
            bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        let sample = super::super::sample(&bytes, &[]);
        assert_eq!(sample.driveline_parameter.to_bits(), 0x8000_0000);
        assert_eq!(sample.notify_a_speed.to_bits(), 0x7fc0_1234);
        assert_eq!(sample.position.map(f32::to_bits), [0x8000_0000, 0xffc0_5678, 42.0f32.to_bits()]);
        assert_eq!(sample.speed, 5.0);
        assert!(sample.outside_assistance.is_none());
        assert!(sample.retire.is_none());
    }
}
