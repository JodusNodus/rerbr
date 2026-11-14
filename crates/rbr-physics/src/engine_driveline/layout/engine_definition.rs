//! The engine's definition (`CEngine::CDef`, the engine's `+4`) as a step reads it, held read only with the step's
//! definitions (`store::defs`): [`EngineDefinition`], every word the engine's ports read but the exhaust table's, at
//! their offsets, which the engine's view (`EngineView::with_defs`) serves at their addresses. The ports read the
//! scalars as named records ([`TurboDef`], [`InletDef`], [`HeatDef`] and the single words) and the tables at the
//! offsets named here. A word's name is its NGP tweak key (`data/ngp-lsp-keys.md`: a key's member is a `CReal` whose
//! value is at member + 0xc) where it has one, else named by use.
//!
//! Not held, read on the image: the exhaust table (256 rows of eight words from `+0x1fc`, read at the crank's keys:
//! lifting its 8 KB each step costs more than the step's reads of it) and the turbo's two blend tables (the heap blocks
//! behind `+0xa0` and `+0xa8`). The poison build watches the whole span the ports read ([`ENGINE_DEF_BYTES`]).

use std::ops::Range;

use crate::mem::Mem;
use crate::store::Raw;

/// The bytes of the definition the ports read: from the turbo's inertia (`+0x18`) to the friction threshold's end.
pub const ENGINE_DEF_BYTES: Range<u32> = 0x18..0x2870;

/// The knots of a curve.
pub const CURVE_KNOTS: usize = 16;
/// The bytes between two knots of a curve (a `CRealPair`: the key's `CReal` value at +0xc, the value's at +0x1c).
const KNOT_STRIDE: u32 = 0x20;
/// The bytes between two cylinders' words (the phase and the oil gallery's three).
const CYLINDER_STRIDE: u32 = 0x40;

// The turbo's: `TurboInertia_NGP`, the blow-off's `TurboMinBlowOffPressure_NGP`, `TurboMaxBlowOffPressure_NGP` and
// `TurboBlowOffCoefficient_NGP`, `TurboWasteGateCoefficient_NGP`, the two blend tables' counts and record pointers
// (named by use), `TurboLinearFrictionFactor_NGP`, `TurboSquareFrictionFactor_NGP`, `TurboMaxOilFlowPressure_NGP` and
// `TurboMinOilFlowPressure_NGP` (`EngineDef`'s fifth stage).
const TURBO_INERTIA: u32 = 0x18;
const MIN_BLOW_OFF_PRESSURE: u32 = 0x48;
const MAX_BLOW_OFF_PRESSURE: u32 = 0x58;
const BLOW_OFF_COEFFICIENT: u32 = 0x68;
const WASTE_GATE_COEFFICIENT: u32 = 0x98;
pub const BOOST_TABLE: u32 = 0x9c;
pub const BAND_TABLE: u32 = 0xa4;
const LINEAR_FRICTION: u32 = 0xb8;
const SQUARE_FRICTION: u32 = 0xc8;
const OIL_FLOW_PRESSURES: u32 = 0xd8;
/// Per cylinder (`CYLINDER_STRIDE` apart): its crank phase in crank ticks (named by use; 0, 0x40, 0x80, 0xc0 at
/// construction) and its oil gallery's `MaxOilGalleryFlowPressure_xx_NGP`, `MinOilGalleryFlowPressure_xx_NGP` and
/// `MinOilFlowRatePercentage_xx_NGP` (`EngineDef`'s stages, `LoadDef`'s denominators).
const CYLINDER_PHASE: u32 = 0x108;
const CYLINDERS: u32 = 4;
/// The exhaust table's first row (`SimEC40`'s rows, eight words 0x20 bytes apart), on the image.
pub(super) const EXHAUST_ROWS: u32 = 0x1fc;
// The torque curve's scales and coefficients (`SimED70`, `EngineDef`; names not known).
const TORQUE_SCALES: u32 = 0x21dc;
const TORQUE_B: u32 = 0x21fc;
const TORQUE_A: u32 = 0x2214;
const TORQUE_C: u32 = 0x222c;
// The volumetric efficiency over the engine speed (`VE00..VE15`; NGP's `Tq_xx_RPM_NGP` and `Tq_xx_NGP`) and the
// turbine's back pressure over it (`TurboTurbineBackPressure_xx_AV_NGP` and `_xx_NGP`).
pub const VE_CURVE: u32 = 0x2244;
pub const BACK_PRESSURE_CURVE: u32 = 0x2444;
// `CylinderVolume_NGP`, the intake flow table (named by use), the cylinder count (named by use),
// `InletPipeAirFlowCoefficient_NGP`, `InletPipeArea_NGP`, `Turbo` (`EngineType_NGP`, whose bit 0 NGP's car init
// leaves), `IdleSpeedThrottle_NGP`, `TurboHosingDamageLossCoefficient_NGP`.
const CYLINDER_VOLUME: u32 = 0x2650;
pub const FLOW_TABLE: u32 = 0x2654;
pub const FLOW_TABLE_LEN: usize = 17;
const CYLINDER_COUNT: u32 = 0x2698;
const INLET_FLOW_COEFFICIENT: u32 = 0x26b8;
const INLET_AREA: u32 = 0x26d8;
const TURBO: u32 = 0x26f8;
const IDLE_SPEED_THROTTLE: u32 = 0x2708;
const HOSING_LOSS: u32 = 0x2718;
// The cooling's: the engine's heat capacity (mass, specific heat; NGP's car init copies `EngineMass_NGP` into the
// mass), `EngineHeatTransferCoefficient_NGP`, `EngineCoolantDuctArea_NGP`, the coolant's heat block (the heat scale
// word and the clamp parameters), `EngineCoolantHeatStateMassUpperBound_NGP`, `ThermostatOpeningTemperature_NGP`,
// `WaterPumpMassFlowRate_NGP`, the radiator's `RadiatorCoolantHeatStateMassUpperBound_NGP`,
// `RadiatorCoolantMassLowerBound_NGP`, `RadiatorCoolantMassUpperBound_NGP`, `RadiatorArea_NGP`,
// `BaseAirCooling_NGP`, `VelocityAirCooling_NGP`, and the axis the airflow is taken along (named by use).
pub const ENGINE_HEAT_CAPACITY: u32 = 0x271c;
const HEAT_TRANSFER: u32 = 0x2734;
const COOLANT_DUCT_AREA: u32 = 0x2744;
pub const COOLANT_HEAT: u32 = 0x2748;
const COOLANT_MASS_MAX: u32 = 0x2764;
const THERMOSTAT_OPENING: u32 = 0x2774;
const WATER_PUMP_FLOW: u32 = 0x2784;
const RADIATOR_HEAT_MASS_MAX: u32 = 0x279c;
const RADIATOR_MASS_MIN: u32 = 0x27ac;
const RADIATOR_MASS_MAX: u32 = 0x27bc;
const RADIATOR_AREA: u32 = 0x27cc;
const BASE_AIR_COOLING: u32 = 0x27dc;
const VELOCITY_AIR_COOLING: u32 = 0x27ec;
const AIRFLOW_AXIS: u32 = 0x2800;
// The oil's `OilCapacity_NGP` and `OilPumpOutput_NGP` (`EngineDef`'s abs scale), `TorqueCut_NGP` (NGP's engine
// wrapper's scale in the shift cooldown), the friction's `CylinderOilFrictionTorqueCoefficient_NGP`,
// `HighTemperatureFrictionLossTorqueCoefficient_NGP` and `FrictionLossTemperatureThreshold_NGP` (`LoadDef`'s).
const OIL_CAPACITY: u32 = 0x281c;
const OIL_PUMP_OUTPUT: u32 = 0x282c;
const TORQUE_CUT: u32 = 0x283c;
const FRICTION: u32 = 0x284c;

/// The held words' runs: offset and length in words, the curves' knots and the cylinders' words added by [`offsets`].
const RUNS: &[(u32, u32)] = &[
    (TURBO_INERTIA, 1),
    (MIN_BLOW_OFF_PRESSURE, 1),
    (MAX_BLOW_OFF_PRESSURE, 1),
    (BLOW_OFF_COEFFICIENT, 1),
    (WASTE_GATE_COEFFICIENT, 1),
    (BOOST_TABLE, 2),
    (BAND_TABLE, 2),
    (LINEAR_FRICTION, 1),
    (SQUARE_FRICTION, 1),
    (OIL_FLOW_PRESSURES, 1),
    (OIL_FLOW_PRESSURES + 0x10, 1),
    (TORQUE_SCALES, 3),
    (TORQUE_B, 3),
    (TORQUE_A, 3),
    (TORQUE_C, 3),
    (CYLINDER_VOLUME, 1),
    (FLOW_TABLE, FLOW_TABLE_LEN as u32),
    (CYLINDER_COUNT, 1),
    (INLET_FLOW_COEFFICIENT, 1),
    (INLET_AREA, 1),
    (TURBO, 1),
    (IDLE_SPEED_THROTTLE, 1),
    (HOSING_LOSS, 1),
    (ENGINE_HEAT_CAPACITY, 2),
    (HEAT_TRANSFER, 1),
    (COOLANT_DUCT_AREA, 1),
    (COOLANT_HEAT, 4),
    (COOLANT_MASS_MAX, 1),
    (THERMOSTAT_OPENING, 1),
    (WATER_PUMP_FLOW, 1),
    (RADIATOR_HEAT_MASS_MAX, 1),
    (RADIATOR_MASS_MIN, 1),
    (RADIATOR_MASS_MAX, 1),
    (RADIATOR_AREA, 1),
    (BASE_AIR_COOLING, 1),
    (VELOCITY_AIR_COOLING, 1),
    (AIRFLOW_AXIS, 3),
    (OIL_CAPACITY, 1),
    (OIL_PUMP_OUTPUT, 1),
    (TORQUE_CUT, 1),
    (FRICTION, 1),
    (FRICTION + 0x10, 1),
    (FRICTION + 0x20, 1),
];

/// The words of the definition's span ([`ENGINE_DEF_BYTES`] from 0).
const SPAN_WORDS: usize = (ENGINE_DEF_BYTES.end / 4) as usize;
/// The count of the held words.
const HELD: usize = offsets().1;
/// Each held word's offset, in slot order.
const OFFSETS: [u32; HELD] = {
    let (all, n) = offsets();
    let mut out = [0; HELD];
    let mut k = 0;
    while k < n {
        out[k] = all[k];
        k += 1;
    }
    out
};
/// Each word's slot in the held words, by offset / 4 (`u16::MAX`: not held).
const SLOTS: [u16; SPAN_WORDS] = {
    let mut slots = [u16::MAX; SPAN_WORDS];
    let mut k = 0;
    while k < HELD {
        let w = (OFFSETS[k] / 4) as usize;
        assert!(slots[w] == u16::MAX, "a word held twice");
        slots[w] = k as u16;
        k += 1;
    }
    slots
};

/// The held words' offsets (the runs, the cylinders' words, the curves' knots) and their count.
const fn offsets() -> ([u32; SPAN_WORDS], usize) {
    let mut out = [0; SPAN_WORDS];
    let mut n = 0;
    let mut r = 0;
    while r < RUNS.len() {
        let (at, len) = RUNS[r];
        let mut k = 0;
        while k < len {
            out[n] = at + 4 * k;
            n += 1;
            k += 1;
        }
        r += 1;
    }
    let mut c = 0;
    while c < CYLINDERS {
        let mut k = 0;
        while k < 4 {
            out[n] = CYLINDER_PHASE + CYLINDER_STRIDE * c + 0x10 * k;
            n += 1;
            k += 1;
        }
        c += 1;
    }
    let curves = [VE_CURVE, BACK_PRESSURE_CURVE];
    let mut v = 0;
    while v < curves.len() {
        let mut k = 0;
        while k < CURVE_KNOTS as u32 {
            out[n] = curves[v] + KNOT_STRIDE * k + 0xc;
            out[n + 1] = curves[v] + KNOT_STRIDE * k + 0x1c;
            n += 2;
            k += 1;
        }
        v += 1;
    }
    (out, n)
}

/// The words of an engine's definition a step reads (the exhaust table's on the image), as the image holds them at a
/// step's start: one per held offset.
#[derive(Clone, PartialEq)]
pub struct EngineDefinition {
    words: [u32; HELD],
}

impl std::fmt::Debug for EngineDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EngineDefinition({HELD} words)")
    }
}

impl EngineDefinition {
    /// The definition at `def`, from the image.
    pub fn lift(mem: &Mem, def: u32) -> Self {
        EngineDefinition { words: OFFSETS.map(|off| mem.u32(def + off)) }
    }

    /// The word at `offset` from the definition's start (aligned), where it holds it.
    #[inline(always)]
    pub fn word(&self, offset: u32) -> Option<u32> {
        let slot = *SLOTS.get((offset / 4) as usize)?;
        self.words.get(usize::from(slot)).copied()
    }

    /// Every held word against the image's at `def`: the bytes compared, or the first that differs.
    pub fn differential(&self, mem: &Mem, def: u32) -> Result<usize, String> {
        for (&off, &word) in OFFSETS.iter().zip(&self.words) {
            if word != mem.u32(def + off) {
                return Err(format!("engine def at {def:#010x}: +{off:#x} differs from the image's"));
            }
        }
        Ok(4 * HELD)
    }
}

/// Whether the engine of the definition at `def` is turbocharged.
pub fn turbo<S: Raw>(mem: &S, def: u32) -> bool {
    mem.u32(def + TURBO) != 0
}

/// The count of the cylinders (the intake entries the engine step runs).
pub fn cylinder_count<S: Raw>(mem: &S, def: u32) -> u32 {
    mem.u32(def + CYLINDER_COUNT)
}

/// Cylinder `k`'s crank phase, in crank ticks (any `k`, read at its offset as the original does).
pub fn cylinder_phase<S: Raw>(mem: &S, def: u32, k: u32) -> u32 {
    mem.u32(def.wrapping_add(CYLINDER_PHASE).wrapping_add(CYLINDER_STRIDE.wrapping_mul(k)))
}

/// The pedal's floor in the cosine response.
pub fn idle_speed_throttle<S: Raw>(mem: &S, def: u32) -> f32 {
    mem.f32(def + IDLE_SPEED_THROTTLE)
}

/// The oil capacity, what the fuel and air terms' total drains the first gate by.
pub fn oil_capacity<S: Raw>(mem: &S, def: u32) -> f32 {
    mem.f32(def + OIL_CAPACITY)
}

/// NGP's torque cut during the shift cooldown.
pub fn torque_cut<S: Raw>(mem: &S, def: u32) -> f32 {
    mem.f32(def + TORQUE_CUT)
}

/// The turbo's words the turbo step reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TurboDef {
    pub inertia: f32,
    pub min_blow_off_pressure: f32,
    pub max_blow_off_pressure: f32,
    pub blow_off_coefficient: f32,
    pub waste_gate_coefficient: f32,
    pub linear_friction: f32,
    pub square_friction: f32,
    pub hosing_loss: f32,
}

impl TurboDef {
    pub fn load<S: Raw>(mem: &S, def: u32) -> Self {
        let f = |field: u32| mem.f32(def + field);
        TurboDef {
            inertia: f(TURBO_INERTIA),
            min_blow_off_pressure: f(MIN_BLOW_OFF_PRESSURE),
            max_blow_off_pressure: f(MAX_BLOW_OFF_PRESSURE),
            blow_off_coefficient: f(BLOW_OFF_COEFFICIENT),
            waste_gate_coefficient: f(WASTE_GATE_COEFFICIENT),
            linear_friction: f(LINEAR_FRICTION),
            square_friction: f(SQUARE_FRICTION),
            hosing_loss: f(HOSING_LOSS),
        }
    }
}

/// The inlet's words the intake reset reads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InletDef {
    pub turbo: bool,
    pub cylinder_volume: f32,
    pub inlet_flow_coefficient: f32,
    pub inlet_area: f32,
}

impl InletDef {
    pub fn load<S: Raw>(mem: &S, def: u32) -> Self {
        InletDef {
            turbo: turbo(mem, def),
            cylinder_volume: mem.f32(def + CYLINDER_VOLUME),
            inlet_flow_coefficient: mem.f32(def + INLET_FLOW_COEFFICIENT),
            inlet_area: mem.f32(def + INLET_AREA),
        }
    }
}

/// The cooling's words the heat and overheat steps read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeatDef {
    pub heat_transfer: f32,
    pub coolant_duct_area: f32,
    pub coolant_mass_max: f32,
    /// K.
    pub thermostat_opening: f32,
    pub water_pump_flow: f32,
    pub radiator_heat_mass_max: f32,
    pub radiator_mass_min: f32,
    pub radiator_mass_max: f32,
    pub radiator_area: f32,
    pub base_air_cooling: f32,
    pub velocity_air_cooling: f32,
    /// The axis's words in the order the airflow term takes them (`+0x2804`, `+0x2808`, `+0x2800`).
    pub airflow_axis: [f32; 3],
}

impl HeatDef {
    pub fn load<S: Raw>(mem: &S, def: u32) -> Self {
        let f = |field: u32| mem.f32(def + field);
        HeatDef {
            heat_transfer: f(HEAT_TRANSFER),
            coolant_duct_area: f(COOLANT_DUCT_AREA),
            coolant_mass_max: f(COOLANT_MASS_MAX),
            thermostat_opening: f(THERMOSTAT_OPENING),
            water_pump_flow: f(WATER_PUMP_FLOW),
            radiator_heat_mass_max: f(RADIATOR_HEAT_MASS_MAX),
            radiator_mass_min: f(RADIATOR_MASS_MIN),
            radiator_mass_max: f(RADIATOR_MASS_MAX),
            radiator_area: f(RADIATOR_AREA),
            base_air_cooling: f(BASE_AIR_COOLING),
            velocity_air_cooling: f(VELOCITY_AIR_COOLING),
            airflow_axis: [f(AIRFLOW_AXIS + 4), f(AIRFLOW_AXIS + 8), f(AIRFLOW_AXIS)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_words_are_the_images_and_nothing_else() {
        let mut mem = Mem::new();
        let def = 0x4000;
        let bytes: Vec<u8> = (0..0x2880u32).map(|k| (k as u8).wrapping_mul(31) ^ 0x5a).collect();
        mem.map(def, &bytes);
        let held = EngineDefinition::lift(&mem, def);
        let mut count = 0;
        for off in (0..0x2880).step_by(4) {
            if let Some(word) = held.word(off) {
                assert!(ENGINE_DEF_BYTES.contains(&off), "+{off:#x}");
                assert_eq!(word, mem.u32(def + off), "+{off:#x}");
                count += 1;
            }
        }
        assert_eq!(count, HELD);
        assert_eq!(held.differential(&mem, def), Ok(4 * HELD));
        // A curve holds its knots' key and value, not their headers; the exhaust table is not held.
        assert!(held.word(VE_CURVE + 0xc).is_some() && held.word(BACK_PRESSURE_CURVE + 0x1fc).is_some());
        assert_eq!(held.word(VE_CURVE + 0x10), None);
        assert_eq!(held.word(EXHAUST_ROWS + 0x20 * 7), None);
        assert!(held.word(TURBO).is_some() && held.word(FRICTION + 0x20).is_some());
        mem.set_u32(def + TURBO, 1);
        assert!(held.differential(&mem, def).is_err());
    }
}
