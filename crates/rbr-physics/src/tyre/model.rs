//! The seam of a tyre model (ours, experimental, `rbr_tyre`): given one, the tyre step takes the contact patch's forces
//! from it in place of NGP's sliding model's, blended by the weight it answers with, and `forces::solve_forces` takes
//! the weight to leave the model's force undamped below 2 m/s; without one (the default) the step is NGP's. A model
//! that answers with temperatures ([`ForceOutput::temperatures`]) takes the tread's surface and the tread from NGP's
//! heat step ([`apply_temperatures`]).

use std::f32::consts::TAU;

use super::layout::Frame;
use crate::ngp::heat::TyreThermal;

/// What a tyre model reads at one step of a wheel in contact, after the temperature grip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForceInput {
    /// The time step, s.
    pub dt: f32,
    /// Whether the wheel was in contact at its last step (if not, its deflections start from zero).
    pub touching: bool,
    /// The contact point's speed over the ground, its lateral and longitudinal velocities, m/s (forward motion is a
    /// negative longitudinal velocity), the wheel's spin, rad/s (positive rolling forward), and its effective radius, m.
    pub speed: f32,
    pub lateral_velocity: f32,
    pub longitudinal_velocity: f32,
    pub spin: f32,
    pub effective_radius: f32,
    /// The normal force, N, and the contact patch's length, m.
    pub normal_force: f32,
    pub patch_length: f32,
    /// The grip NGP's peak takes: the static friction (the surface's, wet or dry, times the temperature grip) times the
    /// inclination's scale of the friction tables.
    pub grip: f32,
    /// The inclination's scale of the stiffness tables (1 unless the camber is large), and the stiffness factor at the
    /// air pressure.
    pub stiffness_scale: f32,
    pub stiffness_factor: f32,
    /// The tread temperature, K, and the tread left (1 new).
    pub tread_temperature: f32,
    pub tread_left: f32,
    /// The soil's thickness under the tyre, m: 0 on a hard surface.
    pub soil_thickness: f32,
    /// The surface's wetness, 0 dry to 1 wet (`WaterFactor`).
    pub water_factor: f32,
    /// The sine of the tyre's inclination to the ground, in the wheel's frame: negative with its top leaning towards
    /// the car (negative camber), whose thrust then points inwards (NGP's sliding model takes `0.1` of it off the
    /// lateral slip).
    pub inclination_sin: f32,
    /// The surface classes of the two materials the wheel's surface blends (each material's `+0x148`, the index of the
    /// tyre type's per-class base friction: 1 gravel, 3 snow) and the second one's weight, 0 to 1. NGP's step does not
    /// read them: [`input`] leaves them unknown ([`UNKNOWN_CLASS`]), the host fills them in.
    pub surface_classes: [u32; 2],
    pub surface_blend: f32,
    /// The carcass's lateral stiffness times the inclination's cosine, N/m, and its torsional stiffness, N·m/rad, as
    /// NGP's sliding model takes them (the torsional one reduced at low speed).
    pub lateral_stiffness: f32,
    pub torsional_stiffness: f32,
    /// What a temperature model reads ([`input`] fills it in; `None` holds the temperatures as they are).
    pub thermal: Option<ThermalInput>,
}

/// What a tyre temperature model reads at the step: NGP's temperatures as they stand, the tyre's heat capacities and
/// areas, and the heat flows NGP's own terms give.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThermalInput {
    /// NGP's rubber grip at its temperatures (`RubMyFct` at the contact sector's surface temperature, 0.6, and at the
    /// tread's, 0.4), before the wet wear: `grip` holds it.
    pub rubber_grip: f32,
    /// The mean of the tread sectors' surface temperatures, the carcass temperature and the air's, K.
    pub surface_temperature: f32,
    pub carcass_temperature: f32,
    pub air_temperature: f32,
    /// The heat capacities of the whole tread's surface layer (every sector's) and of the tread under it, J/K.
    pub surface_capacity: f32,
    pub tread_capacity: f32,
    /// The tread's depth and width and the tyre's outer radius, m; the tread's surface and the contact's area, m².
    pub tread_depth: f32,
    pub tread_width: f32,
    pub outer_radius: f32,
    pub tread_area: f32,
    pub contact_area: f32,
    /// The conductance between the tread and the carcass, W/K (`relax`'s band conduction).
    pub carcass_conductance: f32,
    /// The heat step's contact conductance factor at the contact pressure.
    pub contact_conductance: f32,
    /// The rolling resistance torque's power, W.
    pub rolling_power: f32,
}

/// A surface class [`input`] does not know.
pub const UNKNOWN_CLASS: u32 = u32::MAX;

/// A tyre model's forces at the step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ForceOutput {
    /// Their weight against NGP's sliding model, 0 to 1 (1: the model's alone).
    pub weight: f32,
    /// The patch's lateral and longitudinal forces, N, in NGP's signs (they oppose the sliding).
    pub lateral_force: f32,
    pub longitudinal_force: f32,
    /// The pneumatic trail, m.
    pub pneumatic_trail: f32,
    /// The slip phase in NGP's sense: 0 to 1 up to the peak, 1 to 2 past it, 2 sliding.
    pub phase: f32,
    /// The model's surface and tread temperatures after the step, K (`None`: NGP's heat step's).
    pub temperatures: Option<[f32; 2]>,
}

/// A tyre model on one wheel's step: `None` leaves it NGP's.
pub type Model<'a> = &'a mut dyn FnMut(&ForceInput) -> Option<ForceOutput>;

/// The model's input at the step on `f`, after the temperature grip `rubber_grip` (before its wet wear), with the
/// inclination's scales of the friction (`grip_scale`) and the stiffness (`stiffness_scale`) tables, under NGP's
/// thermal state `ngp`.
pub fn input(
    f: &Frame,
    dt: f32,
    grip_scale: f32,
    stiffness_scale: f32,
    rubber_grip: f32,
    ngp: &TyreThermal,
) -> ForceInput {
    let (s, st, d) = (&f.scratch, &f.state, &f.setup);
    let depth = d.tread_depth;
    let sectors = st.surface_temperature.len() as f32;
    let pressure = st.contact_pressure_segment;
    let conductance = |i: u32| d.contact_conductance.at(i as i32);
    let thermal = conductance(pressure.lo).zip(conductance(pressure.hi)).map(|(lo, hi)| ThermalInput {
        rubber_grip,
        surface_temperature: st.surface_temperature.iter().sum::<f32>() / sectors,
        carcass_temperature: st.carcass_temperature,
        air_temperature: ngp.air_temperature,
        surface_capacity: d.surface_capacity[0] * d.surface_capacity[1] * sectors,
        tread_capacity: d.tread_capacity[0] * d.tread_capacity[1],
        tread_depth: depth,
        tread_width: d.tread_width,
        outer_radius: d.carcass_radius + depth,
        tread_area: (d.carcass_radius + depth) * TAU * d.tread_width,
        contact_area: s.tread_contact_area,
        carcass_conductance: ngp.relax_rate * d.carcass_conductivity * (d.tread_width * d.carcass_radius * TAU),
        contact_conductance: (1.0 - pressure.frac) * lo + hi * pressure.frac,
        rolling_power: (s.patch_length * s.rolling_resistance * 0.5 * s.normal_force * s.sliding_weight * s.spin).abs(),
    });
    ForceInput {
        dt,
        touching: st.active != 0,
        speed: s.speed,
        lateral_velocity: s.contact_velocity[0],
        longitudinal_velocity: s.contact_velocity[1],
        spin: s.spin,
        effective_radius: st.effective_radius,
        normal_force: s.normal_force,
        patch_length: s.patch_length,
        grip: s.static_friction * grip_scale,
        stiffness_scale,
        stiffness_factor: s.stiffness_factor,
        tread_temperature: st.tread_temperature,
        tread_left: if depth > 0.0 { s.tread_remaining / depth } else { 1.0 },
        soil_thickness: s.soil_thickness,
        water_factor: s.water_factor,
        inclination_sin: s.inclination_sin,
        surface_classes: [UNKNOWN_CLASS; 2],
        surface_blend: 0.0,
        lateral_stiffness: s.inclination_cos * s.lateral_stiffness,
        torsional_stiffness: s.torsional_stiffness,
        thermal,
    }
}

/// Blends the model's forces `o` into NGP's sliding model's on `f` by their weight: the carcass deflection and torsion
/// that hold them (with the soil's forces, as NGP's limit takes them), the trail, the phases and the friction forces.
pub fn apply(f: &mut Frame, o: &ForceOutput) {
    let mix = |ngp: f32, model: f32| ngp + (model - ngp) * o.weight;
    let s = &mut f.scratch;
    let deflection = -(s.lateral_soil_force + o.lateral_force) / (s.inclination_cos * s.lateral_stiffness);
    let torsion =
        -(f.state.effective_radius * o.longitudinal_force + s.longitudinal_soil_force) / s.torsional_stiffness;
    f.state.lateral_deflection = mix(f.state.lateral_deflection, deflection);
    f.state.torsion_angle = mix(f.state.torsion_angle, torsion);
    s.pneumatic_trail = mix(s.pneumatic_trail, o.pneumatic_trail);
    s.lateral_phase = mix(s.lateral_phase, o.phase);
    s.longitudinal_phase = mix(s.longitudinal_phase, o.phase);
    s.slip_phase = mix(s.slip_phase, o.phase);
    s.lateral_friction_force = mix(s.lateral_friction_force, o.lateral_force);
    s.longitudinal_friction_force = mix(s.longitudinal_friction_force, o.longitudinal_force);
}

/// A model's temperatures `t` (surface, tread) in place of NGP's heat step's on `f`: every sector's surface and the
/// tread take them, and the carcass goes back to `carcass`, its temperature before the heat step (the model's tread
/// conducts to it, and `relax` conducts the same flow into it). The heat step's wear and power words stay.
pub fn apply_temperatures(f: &mut Frame, t: [f32; 2], carcass: f32) {
    f.state.surface_temperature = [t[0]; 8];
    f.state.tread_temperature = t[1];
    f.state.carcass_temperature = carcass;
}
