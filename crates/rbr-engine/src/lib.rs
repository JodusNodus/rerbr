//! Engine models (ours, experimental): a model gives the net crank torque in place of NGP's torque formula
//! (`rbr_physics::ngp::engine::torque`) in NGP's engine wrapper (`rbr_hybrid::ngp::engine`). [`model`] builds one by
//! name for a car's [`Params`]; the host builds a fresh one at each start, and steps it once per physics step with
//! that step's [`Inputs`].

pub mod bench;
pub mod calibrate;
pub mod class;
pub mod curve;
pub mod ecu;
pub mod engine;
pub mod friction;
pub mod gas;
pub mod realistic;
pub mod samples;
pub mod solve;
pub mod turbo;

/// The car's engine in physical units, as NGP holds it after its car init, and what the car's metadata tells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params {
    /// The full-throttle torque curve's knots: engine speed (rad/s) and torque as a fraction of [`Self::max_torque`]
    /// (`Tq_xx_RPM_NGP`, `Tq_xx_NGP`: the first and those with a positive value, without repeated speeds).
    pub torque_knots: Vec<(f32, f32)>,
    /// `MaxTorque_NGP`, N·m.
    pub max_torque: f32,
    /// The control unit's `RPMLimit`, rad/s.
    pub rev_limit: f32,
    /// `IdleRPM_NGP`, rad/s.
    pub idle_speed: f32,
    /// `BrakeCoefficient_NGP`, a fraction of [`Self::max_torque`]: NGP's closed-throttle braking at the rev limit.
    pub brake_coefficient: f32,
    /// `EngineType_NGP` bit 0.
    pub turbo: bool,
    /// `CylinderVolume_NGP`, m³: a quarter of the displacement on every car, whatever its cylinder count.
    pub cylinder_volume: f32,
    /// `InletPipeArea_NGP`, m²: the throttle's total area.
    pub inlet_area: f32,
    /// `TorqueCut_NGP`: the share of torque NGP keeps during the control unit's shift cooldown.
    pub torque_cut: f32,
    /// What the crank accelerates in neutral (`EngineInertia` + `GearBoxInertiaIn`), kg·m².
    pub inertia: f32,
    /// The car's identity and metadata.
    pub car: Car,
}

/// The car's identity and metadata (RallySimFans' `cars.json` and `cars_data.json`), when the car has them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Car {
    /// The car's folder: the RSF pack's (`Ford_Fiesta_Rally2_ngp7`), else the slot's.
    pub folder: String,
    /// The RSF base group's name (`cargroups.json`), e.g. `Group A8`, `Rally 2`, `WRC 2.0`.
    pub group: Option<String>,
    /// The car's year (`cars_data.json` `year`).
    pub year: Option<u32>,
}

/// One physics step's inputs, as NGP's engine wrapper reads them while the engine runs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Inputs {
    /// The step, s (1/720).
    pub dt: f32,
    /// The crank's angular speed, rad/s.
    pub speed: f32,
    /// The wheels' rim speeds (|ωR|), m/s; the start mode's signal.
    pub wheels: [f32; 4],
    /// The car's speed along its forward direction, m/s; the start mode's signal.
    pub body_speed: f32,
    /// The throttle the control unit passes on (engine +0x20), 0..=1: the pedal, or the control unit's blip or cut.
    pub throttle: f32,
    /// The control unit's shift cooldown is running (NGP scales its torque by [`Params::torque_cut`] then).
    pub shift_cut: bool,
    /// NGP's damage factor, 1 for an intact engine.
    pub damage: f32,
    /// The air at the car: pressure (Pa) and temperature (K).
    pub pressure: f32,
    pub temperature: f32,
    /// The 1.02 engine sim's friction torque (state +0x270, N·m): oil starvation, overheating, damage.
    pub friction: f32,
    /// NGP's stall history: no speed above its stall speed within its 0.15 s ring, and below it now.
    pub stalled: bool,
    /// The 1.02 engine sim's engine temperature (state +0x23c), K.
    pub engine_temperature: f32,
}

/// What a model shows of itself after a step, for curves, checks and presentation (sound, gauges). Never read back
/// into the physics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Telemetry {
    /// Intake manifold pressure, Pa.
    pub manifold_pressure: f32,
    /// Boost (charge pipe) pressure before the throttle, Pa; the air's pressure without a turbo.
    pub boost_pressure: f32,
    /// Turbo shaft speed, rad/s; 0 without a turbo.
    pub turbo_speed: f32,
    /// Throttle plate lift, 0..=1.
    pub plate: f32,
    /// The share of firing events cut this step (limiter, shift, overrun, anti-lag), 0..=1.
    pub cut: f32,
    /// Anti-lag is running.
    pub anti_lag: bool,
    /// The blow-off valve's opening, 0..=1 (0 without a turbo): what vents the boost pipe while the throttle is shut.
    pub blow_off: f32,
    /// The air's pressure at the car, Pa (the step's [`Inputs::pressure`]); 0 before the first step.
    pub air_pressure: f32,
}

/// An engine model: one per car and run.
pub trait Model: Send {
    /// This step's net crank torque, N·m, positive driving the crank: indicated torque less friction and pumping,
    /// what the drivetrain takes whole (NGP's own formula goes through its wrapper's /1.05).
    fn torque(&mut self, input: &Inputs) -> f32;

    /// Back to a new model's state, keeping what it built for the car: the stage's restart.
    fn restart(&mut self);

    /// The state after the last step.
    fn telemetry(&self) -> Telemetry {
        Telemetry::default()
    }
}

/// The names [`model`] takes; `ngp` is NGP's own formula.
pub const NAMES: [&str; 2] = ["ngp", "realistic"];

/// The engine model `name` for a car with these params: `None` for `ngp` (NGP's formula runs as it is).
pub fn model(name: &str, params: &Params) -> Result<Option<Box<dyn Model>>, String> {
    match name {
        "ngp" => Ok(None),
        "realistic" => Ok(Some(Box::new(realistic::Realistic::new(params)))),
        _ => Err(format!("engine model {name}: one of {}", NAMES.join(", "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_pick_their_models() {
        let params = Params::default();
        assert!(model("ngp", &params).unwrap().is_none());
        assert_eq!(model("fast", &params).err().unwrap(), "engine model fast: one of ngp, realistic");
    }
}
