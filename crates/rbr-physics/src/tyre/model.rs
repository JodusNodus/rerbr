//! The seam of a tyre model (ours, experimental, `rbr_tyre`): given one, the tyre step takes the contact patch's forces
//! from it in place of NGP's sliding model's, blended by the weight it answers with, and `forces::solve_forces` takes
//! the weight to leave the model's force undamped below 2 m/s; without one (the default) the step is NGP's.

use super::layout::Frame;

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
    /// The surface classes of the two materials the wheel's surface blends (each material's `+0x148`, the index of the
    /// tyre type's per-class base friction: 1 gravel, 3 snow) and the second one's weight, 0 to 1. NGP's step does not
    /// read them: [`input`] leaves them unknown ([`UNKNOWN_CLASS`]), the host fills them in.
    pub surface_classes: [u32; 2],
    pub surface_blend: f32,
    /// The carcass's lateral stiffness times the inclination's cosine, N/m, and its torsional stiffness, N·m/rad, as
    /// NGP's sliding model takes them (the torsional one reduced at low speed).
    pub lateral_stiffness: f32,
    pub torsional_stiffness: f32,
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
}

/// A tyre model on one wheel's step: `None` leaves it NGP's.
pub type Model<'a> = &'a mut dyn FnMut(&ForceInput) -> Option<ForceOutput>;

/// The model's input at the step on `f`, after the temperature grip, with the inclination's scales of the friction
/// (`grip_scale`) and the stiffness (`stiffness_scale`) tables.
pub fn input(f: &Frame, dt: f32, grip_scale: f32, stiffness_scale: f32) -> ForceInput {
    let (s, st) = (&f.scratch, &f.state);
    let depth = f.setup.tread_depth;
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
        surface_classes: [UNKNOWN_CLASS; 2],
        surface_blend: 0.0,
        lateral_stiffness: s.inclination_cos * s.lateral_stiffness,
        torsional_stiffness: s.torsional_stiffness,
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
