//! `--tyre-probe FILE` (with `--autopilot`): read-only dumps of each wheel's tyre
//! `Frame` at intervals of the drive, as `name: values` lines. Research tooling
//! for the NGP tyre characterisation: reads physics memory after the step,
//! never writes it.

use std::io::Write;
use std::path::Path;

use rbr_hybrid::pure::CAR_PTR;
use rbr_physics::mem::Mem;
use rbr_physics::tyre::layout::{Table, WORDS};
use rbr_physics::tyre::store::TyreStore;

/// The wheel pointers in the car (`car+0x470`, `ngp::layout::car_init::WHEELS`).
const WHEELS: u32 = 0x470;
/// The middle ray block (`o+0xc20`, `probe::PROBE_BLOCK_2`): its weight (`+0x34`) and tags (`+0x38`, `+0x3c`).
const BLOCK_2: u32 = 0xC20;
/// The physics definition global (`g_1659b6c`); the material manager is at its `+0x18`.
const CDEF_GLOBAL: u32 = 0x0165_9b6c;
const DEF_MATERIALS: u32 = 0x18;
/// A material entry's size (`CMaterial`, 0x160); the first is at the manager's `+0xc`.
const MAT_SIZE: u32 = 0x160;
const MAT_FIRST: u32 = 0x0c;
/// The tyre scratch in the wheel (`this+0xf00`).
const SCRATCH: u32 = 0xF00;
/// Dump every this many steps, plus the final step.
const EVERY: u32 = 1000;

/// A material value's byte offset in the material block, from `WORDS`.
fn mat_word(material: &[u32; 39], offset: u32) -> f32 {
    for (i, w) in WORDS.iter().enumerate() {
        if 4 * w == offset {
            return f32::from_bits(material[i]);
        }
    }
    f32::NAN
}

fn table_line(out: &mut impl Write, name: &str, t: &Table) -> std::io::Result<()> {
    write!(out, "{name}: {}", t.count())?;
    for i in 0..t.count() {
        let v = t.at(i as i32).unwrap_or(f32::NAN);
        write!(out, " {v:.6}")?;
    }
    writeln!(out)
}

/// An open probe dump.
pub struct TyreProbe {
    file: std::io::BufWriter<std::fs::File>,
    path: String,
}

impl TyreProbe {
    /// Open `path` for the dump.
    pub fn create(path: &Path) -> Result<Self, String> {
        let file =
            std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
        Ok(TyreProbe { file, path: path.display().to_string() })
    }

    /// Dump after the step at `step` when it is due (`EVERY`), or always when `last`.
    pub fn step(&mut self, mem: &Mem, step: u32, last: bool) -> Result<(), String> {
        if !last && !step.is_multiple_of(EVERY) {
            return Ok(());
        }
        self.dump(mem, step)
    }

    /// Dump all four wheels' frames after the step at `step`.
    pub fn dump(&mut self, mem: &Mem, step: u32) -> Result<(), String> {
        let failed = |e: std::io::Error| format!("{}: {e}", self.path);
        let car = mem.u32(CAR_PTR);
        writeln!(self.file, "# step {step} car {car:#010x}").map_err(failed)?;
        for k in 0..4 {
            let wheel = mem.u32(car + WHEELS + 4 * k);
            let scratch_at = wheel + SCRATCH;
            let def = mem.u32(scratch_at + 0x10);
            let frame = mem.load_frame(scratch_at);
            let (s, st, d) = (&frame.scratch, &frame.state, &frame.setup);
            writeln!(self.file, "wheel: {k} scratch {scratch_at:#010x} def {def:#010x}").map_err(failed)?;
            let (tag1, tag2) = (mem.u32(wheel + BLOCK_2 + 0x38), mem.u32(wheel + BLOCK_2 + 0x3C));
            writeln!(self.file, "block_tags: {tag1} {tag2} {:.6}", mem.f32(wheel + BLOCK_2 + 0x34),).map_err(failed)?;
            let manager = mem.u32(CDEF_GLOBAL) + DEF_MATERIALS;
            for tag in [tag1, tag2, 0] {
                let entry = manager + MAT_FIRST + tag * MAT_SIZE;
                writeln!(
                    self.file,
                    "mat_entry {tag}: {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:#010x} {:#010x} flag:{:#04x}",
                    mem.f32(entry + 0x48),
                    mem.f32(entry + 0x58),
                    mem.f32(entry + 0x68),
                    mem.f32(entry + 0x78),
                    mem.f32(entry + 0xE8),
                    mem.f32(entry + 0xF8),
                    mem.u32(entry + 0x144),
                    mem.u32(entry + 0x148),
                    mem.u8(manager + tag * MAT_SIZE + 0x164),
                )
                .map_err(failed)?;
            }
            writeln!(self.file, "type_word: {:#010x}", mem.u32(def + 0xC)).map_err(failed)?;
            writeln!(self.file, "reference_friction: {:.6}", d.reference_friction).map_err(failed)?;
            writeln!(
                self.file,
                "carcass: {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
                d.carcass_stiffness_lateral,
                d.carcass_stiffness_torsional,
                d.carcass_radius,
                d.tread_depth,
                d.carcass_thickness,
                d.tread_width,
                d.tread_coverage,
                d.carcass_width,
            )
            .map_err(failed)?;
            writeln!(
                self.file,
                "misc_setup: {:.6} {:.6} {:.6} {:.6}",
                d.sidewall_height, d.pressure_ratio, d.wet_wear_factor, d.inclination_heat_scale
            )
            .map_err(failed)?;
            table_line(&mut self.file, "pressure_axis", &d.pressure_axis).map_err(failed)?;
            table_line(&mut self.file, "stiffness_factor", &d.stiffness_factor_by_pressure).map_err(failed)?;
            table_line(&mut self.file, "temperature_axis", &d.temperature_axis).map_err(failed)?;
            table_line(&mut self.file, "rubber_friction", &d.rubber_friction_by_temperature).map_err(failed)?;
            table_line(&mut self.file, "load_axis", &d.load_axis).map_err(failed)?;
            table_line(&mut self.file, "cornering_stiffness", &d.cornering_stiffness_by_load).map_err(failed)?;
            table_line(&mut self.file, "traction_stiffness", &d.traction_stiffness_by_load).map_err(failed)?;
            table_line(&mut self.file, "cornering_friction", &d.cornering_friction_by_load).map_err(failed)?;
            table_line(&mut self.file, "traction_friction", &d.traction_friction_by_load).map_err(failed)?;
            table_line(&mut self.file, "cornering_peak_slip", &d.cornering_peak_slip_by_load).map_err(failed)?;
            table_line(&mut self.file, "traction_peak_slip", &d.traction_peak_slip_by_load).map_err(failed)?;
            // The tyre type's per-surface-class tables (`load_surface_input`): base friction, wet slope and
            // wear factor (`def+0x2c4/0x2e0/0x2f8`, 6 classes) and the design wetness (`def+0x2dc`).
            write!(self.file, "tyre_classes:").map_err(failed)?;
            for k in 0..6 {
                write!(
                    self.file,
                    " {:.6} {:.6} {:.6}",
                    mem.f32(def + 0x2C4 + 4 * k),
                    mem.f32(def + 0x2E0 + 4 * k),
                    mem.f32(def + 0x2F8 + 4 * k),
                )
                .map_err(failed)?;
            }
            writeln!(self.file).map_err(failed)?;
            writeln!(self.file, "design_wetness: {:.6}", mem.f32(def + 0x2DC)).map_err(failed)?;
            writeln!(
                self.file,
                "surface: {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
                s.static_friction,
                s.kinetic_friction,
                s.static_friction_soil,
                s.kinetic_friction_soil,
                s.sink_factor,
                s.soil_thickness,
                s.water_factor,
                s.rolling_resistance,
                s.temperature_grip,
            )
            .map_err(failed)?;
            writeln!(
                self.file,
                "material: {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
                mat_word(&s.material, 0x48),
                mat_word(&s.material, 0x58),
                mat_word(&s.material, 0x68),
                mat_word(&s.material, 0x78),
                mat_word(&s.material, 0x88),
                mat_word(&s.material, 0x98),
                mat_word(&s.material, 0xB8),
                mat_word(&s.material, 0xE8),
                mat_word(&s.material, 0xF8),
            )
            .map_err(failed)?;
            writeln!(
                self.file,
                "op: {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
                s.normal_force,
                s.patch_length,
                st.effective_radius,
                st.air_pressure,
                s.stiffness_factor,
                s.cornering_stiffness,
                s.traction_stiffness,
                s.carcass_force,
                s.tread_force,
            )
            .map_err(failed)?;
            writeln!(
                self.file,
                "slip_op: {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
                s.speed,
                s.contact_velocity[0],
                s.contact_velocity[1],
                s.spin,
                s.spin_slip,
                s.sliding_weight,
                s.sticking_weight,
                s.slip_phase,
                s.inclination_sin,
                s.hub_height,
            )
            .map_err(failed)?;
            writeln!(
                self.file,
                "force_op: {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
                s.lateral_force,
                s.longitudinal_force,
                s.lateral_friction_force,
                s.longitudinal_friction_force,
                s.pneumatic_trail,
                s.torsion_torque,
            )
            .map_err(failed)?;
            write!(self.file, "wear:").map_err(failed)?;
            for w in s.wear {
                write!(self.file, " {w:.6}").map_err(failed)?;
            }
            writeln!(self.file).map_err(failed)?;
            writeln!(
                self.file,
                "soil_op: {:.6} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
                s.indentation.penetration,
                s.indentation.sink_depth,
                s.indentation.carcass_compression,
                s.contact_pressure,
                s.total_force_ratio,
                s.soil_drag,
                s.lateral_soil_force,
            )
            .map_err(failed)?;
            write!(self.file, "surface_temp:").map_err(failed)?;
            for t in st.surface_temperature {
                write!(self.file, " {t:.3}").map_err(failed)?;
            }
            writeln!(self.file).map_err(failed)?;
            writeln!(
                self.file,
                "temps: {:.3} {:.3} {:.6} {:.6} {:.6} {:.6} {:.6}",
                st.tread_temperature,
                st.carcass_temperature,
                st.load_segment.lo,
                st.load_segment.hi,
                st.load_segment.frac,
                st.pressure_segment.frac,
                st.tread_temperature_segment.frac,
            )
            .map_err(failed)?;
        }
        self.file.flush().map_err(failed)
    }
}
