//! The engine's definition (`CEngine::CDef`, the engine's `+4`) as a step reads it, held read only with the step's
//! definitions (`store::defs`): [`EngineDefinition`], the words the engine's ports read (`+0x18..+0x2870`, from the
//! turbo's inertia to the friction threshold), which the engine's view (`EngineView::with_defs`) serves at their
//! addresses. The ports read the scalars as named records ([`TurboDef`], [`InletDef`], [`HeatDef`] and the single
//! words), the tables at the offsets named here. A word's name is its NGP tweak key (`data/ngp-lsp-keys.md`: a key's
//! member is a `CReal` whose value is at member + 0xc) where it has one, else named by use. The turbo's two blend
//! tables (the heap blocks behind `+0xa0` and `+0xa8`) stay on the image.

use std::ops::Range;

use crate::mem::Mem;
use crate::store::Raw;

/// The bytes of the definition the ports read: from the turbo's inertia (`+0x18`) to the friction threshold's end.
pub const ENGINE_DEF_BYTES: Range<u32> = 0x18..0x2870;
const WORDS: usize = ((ENGINE_DEF_BYTES.end - ENGINE_DEF_BYTES.start) / 4) as usize;

/// The cylinders' crank phases (`+0x108`, 0x40 bytes apart; 0, 0x40, 0x80, 0xc0 at construction) and their count
/// (`+0x2698`), named by use (`Sim8E20`'s and `Sim8D90`'s entry keys).
const CYLINDER_PHASE: u32 = 0x108;
const CYLINDER_PHASE_STRIDE: u32 = 0x40;
const CYLINDER_COUNT: u32 = 0x2698;
/// `Turbo` (`EngineType_NGP`, whose bit 0 NGP's car init leaves): set for a turbocharged engine.
const TURBO: u32 = 0x26f8;
/// `IdleSpeedThrottle_NGP`: the pedal's floor in the cosine response.
const IDLE_SPEED_THROTTLE: u32 = 0x2708;
/// `OilCapacity_NGP`: what the fuel and air terms' total drains the first gate by.
const OIL_CAPACITY: u32 = 0x281c;
/// `TorqueCut_NGP`: NGP's engine wrapper's scale during the shift cooldown.
const TORQUE_CUT: u32 = 0x283c;

/// The volumetric efficiency over the engine speed (`VE00..VE15`; NGP's `Tq_xx_RPM_NGP` and `Tq_xx_NGP`), 16 knots.
pub const VE_CURVE: u32 = 0x2244;
/// The turbine's back pressure over the engine speed (`TurboTurbineBackPressure_xx_AV_NGP` and `_xx_NGP`), 16 knots.
pub const BACK_PRESSURE_CURVE: u32 = 0x2444;
/// The knots of either curve.
pub const CURVE_KNOTS: usize = 16;
/// The intake flow table over the flow ratio (17 values from `+0x2654`), named by use.
pub const FLOW_TABLE: u32 = 0x2654;
pub const FLOW_TABLE_LEN: usize = 17;
/// The turbo's two blend tables (`SimE060`'s on the boost block, `SimE1F0`'s on the band's), named by use: their
/// counts and record pointers (`+0x9c`/`+0xa0`, `+0xa4`/`+0xa8`).
pub const BOOST_TABLE: u32 = 0x9c;
pub const BAND_TABLE: u32 = 0xa4;
/// The engine's heat capacity (mass, specific heat: NGP's car init copies `EngineMass_NGP` into its mass).
pub const ENGINE_HEAT_CAPACITY: u32 = 0x271c;
/// The coolant's heat block (the heat scale word and the clamp parameters both heat states are clamped by).
pub const COOLANT_HEAT: u32 = 0x2748;

/// The definition's words a step reads, as the image holds them at a step's start.
#[derive(Clone, PartialEq)]
pub struct EngineDefinition {
    words: Box<[u32]>,
}

impl std::fmt::Debug for EngineDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EngineDefinition({} words)", self.words.len())
    }
}

impl EngineDefinition {
    /// The definition at `def`, from the image (one block read).
    pub fn lift(mem: &Mem, def: u32) -> Self {
        let bytes = mem.read(def + ENGINE_DEF_BYTES.start, 4 * WORDS);
        let words = bytes.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        EngineDefinition { words }
    }

    /// The word at `offset` from the definition's start (aligned), where it holds it.
    #[inline(always)]
    pub fn word(&self, offset: u32) -> Option<u32> {
        let rel = offset.wrapping_sub(ENGINE_DEF_BYTES.start);
        if rel & 3 == 0 {
            self.words.get((rel / 4) as usize).copied()
        } else {
            None
        }
    }

    /// Every word against the image's at `def`: the bytes compared, or the first that differs.
    pub fn differential(&self, mem: &Mem, def: u32) -> Result<usize, String> {
        for (k, &word) in self.words.iter().enumerate() {
            let at = def + ENGINE_DEF_BYTES.start + 4 * k as u32;
            if word != mem.u32(at) {
                return Err(format!("engine def at {def:#010x}: +{:#x} differs from the image's", at - def));
            }
        }
        Ok(4 * WORDS)
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
    mem.u32(def.wrapping_add(CYLINDER_PHASE).wrapping_add(CYLINDER_PHASE_STRIDE.wrapping_mul(k)))
}

/// The pedal's floor in the cosine response.
pub fn idle_speed_throttle<S: Raw>(mem: &S, def: u32) -> f32 {
    mem.f32(def + IDLE_SPEED_THROTTLE)
}

/// The oil capacity.
pub fn oil_capacity<S: Raw>(mem: &S, def: u32) -> f32 {
    mem.f32(def + OIL_CAPACITY)
}

/// NGP's torque cut during the shift cooldown.
pub fn torque_cut<S: Raw>(mem: &S, def: u32) -> f32 {
    mem.f32(def + TORQUE_CUT)
}

/// The turbo's words the turbo step reads, by their NGP keys.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TurboDef {
    /// `TurboInertia_NGP` (`+0x18`).
    pub inertia: f32,
    /// `TurboMinBlowOffPressure_NGP`, `TurboMaxBlowOffPressure_NGP`, `TurboBlowOffCoefficient_NGP` (`+0x48`, `+0x58`,
    /// `+0x68`).
    pub min_blow_off_pressure: f32,
    pub max_blow_off_pressure: f32,
    pub blow_off_coefficient: f32,
    /// `TurboWasteGateCoefficient_NGP` (`+0x98`).
    pub waste_gate_coefficient: f32,
    /// `TurboLinearFrictionFactor_NGP`, `TurboSquareFrictionFactor_NGP` (`+0xb8`, `+0xc8`).
    pub linear_friction: f32,
    pub square_friction: f32,
    /// `TurboHosingDamageLossCoefficient_NGP` (`+0x2718`).
    pub hosing_loss: f32,
}

impl TurboDef {
    pub fn load<S: Raw>(mem: &S, def: u32) -> Self {
        let f = |off: u32| mem.f32(def + off);
        TurboDef {
            inertia: f(0x18),
            min_blow_off_pressure: f(0x48),
            max_blow_off_pressure: f(0x58),
            blow_off_coefficient: f(0x68),
            waste_gate_coefficient: f(0x98),
            linear_friction: f(0xb8),
            square_friction: f(0xc8),
            hosing_loss: f(0x2718),
        }
    }
}

/// The inlet's words the intake reset reads, by their NGP keys.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InletDef {
    pub turbo: bool,
    /// `CylinderVolume_NGP` (`+0x2650`).
    pub cylinder_volume: f32,
    /// `InletPipeAirFlowCoefficient_NGP`, `InletPipeArea_NGP` (`+0x26b8`, `+0x26d8`).
    pub inlet_flow_coefficient: f32,
    pub inlet_area: f32,
}

impl InletDef {
    pub fn load<S: Raw>(mem: &S, def: u32) -> Self {
        InletDef {
            turbo: turbo(mem, def),
            cylinder_volume: mem.f32(def + 0x2650),
            inlet_flow_coefficient: mem.f32(def + 0x26b8),
            inlet_area: mem.f32(def + 0x26d8),
        }
    }
}

/// The cooling's words the heat and overheat steps read, by their NGP keys.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeatDef {
    /// `EngineHeatTransferCoefficient_NGP`, `EngineCoolantDuctArea_NGP` (`+0x2734`, `+0x2744`).
    pub heat_transfer: f32,
    pub coolant_duct_area: f32,
    /// `EngineCoolantHeatStateMassUpperBound_NGP` (`+0x2764`).
    pub coolant_mass_max: f32,
    /// `ThermostatOpeningTemperature_NGP` (`+0x2774`), K.
    pub thermostat_opening: f32,
    /// `WaterPumpMassFlowRate_NGP` (`+0x2784`).
    pub water_pump_flow: f32,
    /// `RadiatorCoolantHeatStateMassUpperBound_NGP`, `RadiatorCoolantMassLowerBound_NGP`,
    /// `RadiatorCoolantMassUpperBound_NGP` (`+0x279c`, `+0x27ac`, `+0x27bc`).
    pub radiator_heat_mass_max: f32,
    pub radiator_mass_min: f32,
    pub radiator_mass_max: f32,
    /// `RadiatorArea_NGP`, `BaseAirCooling_NGP`, `VelocityAirCooling_NGP` (`+0x27cc`, `+0x27dc`, `+0x27ec`).
    pub radiator_area: f32,
    pub base_air_cooling: f32,
    pub velocity_air_cooling: f32,
    /// The axis the airflow over the radiator is taken along (`+0x2804`, `+0x2808`, `+0x2800`), named by use.
    pub airflow_axis: [f32; 3],
}

impl HeatDef {
    pub fn load<S: Raw>(mem: &S, def: u32) -> Self {
        let f = |off: u32| mem.f32(def + off);
        HeatDef {
            heat_transfer: f(0x2734),
            coolant_duct_area: f(0x2744),
            coolant_mass_max: f(0x2764),
            thermostat_opening: f(0x2774),
            water_pump_flow: f(0x2784),
            radiator_heat_mass_max: f(0x279c),
            radiator_mass_min: f(0x27ac),
            radiator_mass_max: f(0x27bc),
            radiator_area: f(0x27cc),
            base_air_cooling: f(0x27dc),
            velocity_air_cooling: f(0x27ec),
            airflow_axis: [f(0x2804), f(0x2808), f(0x2800)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_words_are_the_images() {
        let mut mem = Mem::new();
        let def = 0x4000;
        let bytes: Vec<u8> = (0..0x2880u32).map(|k| (k as u8).wrapping_mul(31) ^ 0x5a).collect();
        mem.map(def, &bytes);
        let held = EngineDefinition::lift(&mem, def);
        for off in (0..0x2880).step_by(4) {
            let want = ENGINE_DEF_BYTES.contains(&off).then(|| mem.u32(def + off));
            assert_eq!(held.word(off), want, "+{off:#x}");
        }
        assert_eq!(held.word(0x1a), None);
        assert_eq!(held.differential(&mem, def), Ok(0x2858));
        mem.set_u32(def + 0x2000, 1);
        assert!(held.differential(&mem, def).is_err());
    }
}
