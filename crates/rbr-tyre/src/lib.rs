//! Tyre models (ours, experimental): a model gives the contact patch's forces in place of NGP's sliding model's
//! (`rbr_physics::tyre::model`) in NGP's tyre step (`rbr_hybrid::ngp::tyre_model`). [`model`] builds one by name for
//! a car's [`Wheel`]s; the host builds a fresh one at each start and asks it at each wheel's step.

pub mod brush;
pub mod calibrate;
pub mod car;
pub mod fit;
pub mod loose;
pub mod tuning;

use rbr_physics::tyre::layout::{Setup, Table};
pub use rbr_physics::tyre::model::{ForceInput, ForceOutput};
pub use tuning::Tuning;

/// One wheel's tyre as NGP's car init leaves it: the tables of its type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wheel {
    /// The tyre's address (its `CScratch`, the step's `this`), which names it at each step.
    pub tyre: u32,
    /// `ReferenceFriction`.
    pub reference_friction: f32,
    /// The load axis, N, and the tables over it: `CrnMyFct_L`, `TrctMyFct_L` (friction), `SlpPkCrn_L`, `SlpPkTrct_L`
    /// (peak slip), `CrnStf_L`, `TrctStf_L` (stiffness).
    pub load_axis: Vec<f32>,
    pub cornering_friction: Vec<f32>,
    pub traction_friction: Vec<f32>,
    pub cornering_peak_slip: Vec<f32>,
    pub traction_peak_slip: Vec<f32>,
    pub cornering_stiffness: Vec<f32>,
    pub traction_stiffness: Vec<f32>,
    /// The temperature axis, K, and `RubMyFct_T` over it.
    pub temperature_axis: Vec<f32>,
    pub rubber_friction: Vec<f32>,
    /// The type's friction per surface class (`load_surface_input`): base (`SFric`, `def+0x2c4`) and wet slope
    /// (`WetSlope`, `+0x2e0`) of classes 0-5, and its design wetness (`WetnessDesign`, `+0x2dc`). Zero unless the host
    /// reads them.
    pub class_friction: [f32; 6],
    pub class_wet_slope: [f32; 6],
    pub design_wetness: f32,
}

impl Wheel {
    /// The tyre at `tyre` with the type `setup`.
    pub fn new(tyre: u32, setup: &Setup) -> Wheel {
        let entries = |t: &Table| (0..t.count() as i32).filter_map(|i| t.at(i)).collect();
        Wheel {
            tyre,
            reference_friction: setup.reference_friction,
            load_axis: entries(&setup.load_axis),
            cornering_friction: entries(&setup.cornering_friction_by_load),
            traction_friction: entries(&setup.traction_friction_by_load),
            cornering_peak_slip: entries(&setup.cornering_peak_slip_by_load),
            traction_peak_slip: entries(&setup.traction_peak_slip_by_load),
            cornering_stiffness: entries(&setup.cornering_stiffness_by_load),
            traction_stiffness: entries(&setup.traction_stiffness_by_load),
            temperature_axis: entries(&setup.temperature_axis),
            rubber_friction: entries(&setup.rubber_friction_by_temperature),
            ..Wheel::default()
        }
    }

    /// Whether it is NGP's damp tarmac tyre (kind 1): design wetness 0.5 and tarmac its best class (the damp gravel
    /// tyre, kind 4, has design wetness 0.5 and gravel best).
    pub fn is_damp_tarmac(&self) -> bool {
        let tarmac = self.class_friction[brush::TARMAC_CLASS as usize];
        self.design_wetness == 0.5 && self.class_friction.iter().all(|&f| f <= tarmac)
    }
}

/// A tyre model: one per car and run.
pub trait Model: Send {
    /// The contact patch's forces of the tyre at `tyre` at this step (`None`: NGP's).
    fn forces(&mut self, tyre: u32, x: &ForceInput) -> Option<ForceOutput>;

    /// Back to a new model's state, keeping what it built for the car: the stage's restart.
    fn restart(&mut self);
}

/// The names [`model`] takes.
pub const NAMES: [&str; 2] = ["ngp", "brush"];

/// The tyre model `name` for a car with these wheels, the brush tuned by `tuning`: `None` for `ngp` (NGP's sliding
/// model runs as it is).
pub fn model(name: &str, wheels: &[Wheel], tuning: &Tuning) -> Result<Option<Box<dyn Model>>, String> {
    match name {
        "ngp" => Ok(None),
        "brush" => Ok(Some(Box::new(car::Brush::new(wheels, tuning)?))),
        _ => Err(format!("tyre model {name}: one of {}", NAMES.join(", "))),
    }
}
