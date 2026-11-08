//! `--tyre-probe FILE` (with `--autopilot`): read-only dumps of each wheel's tyre
//! `Frame` at intervals of the drive, as `name: values` lines. Research tooling
//! for the NGP tyre characterisation: reads physics memory after the step,
//! never writes it.

use std::io::Write;
use std::path::Path;

use rbr_hybrid::pure::CAR_PTR;
use rbr_physics::mem::Mem;
use rbr_physics::physics_core::layout::material;
use rbr_physics::tyre::layout::{Table, WORDS};
use rbr_physics::tyre::store::TyreStore;
use rbr_physics::wheel::layout::{BLOCK_TAG_1, BLOCK_TAG_2, BLOCK_WEIGHT, CAR, DEF_MATERIALS, TYRE_SCRATCH};

/// The wheel pointers in the car (`car+0x470`, `ngp::layout::car_init::WHEELS`).
const WHEELS: u32 = 0x470;
/// The middle ray block (`o+0xc20`, `wheel::layout::probe`'s `PROBE_BLOCK_2`): its weight and tags.
const BLOCK_2: u32 = 0xC20;
/// The physics definition global (`g_1659b6c`); the material manager is at its `DEF_MATERIALS`.
const CDEF_GLOBAL: u32 = 0x0165_9b6c;
/// A material's flags byte past its entry (the `CBitField` value at `+0x14c + 0xc`).
const MAT_FLAGS: u32 = 0x158;
/// The scratch's footprint the lab maps back (`load_frame` reads to `+0x50c`; the record's angle
/// lives at `+0x5ac`), and the state, setup, wear-table and record sizes it maps with it. NGP's
/// `.data` words come from `NGP_DATA` whole, as `boot::dump` maps them.
const SCRATCH_LEN: u32 = 0x600;
const STATE_LEN: u32 = 0x200;
const DEF_LEN: u32 = 0x400;
const WEAR_LEN: usize = 32;
const RECORD_LEN: u32 = 0x600;
const NGP_FILE_BASE: u32 = 0x1000_0000;
/// NGP's `.data` in this run's memory (`NGP_DATA` file words at `runtime::BASE`, `boot::NGP_DATA`).
const NGP_DATA: (u32, usize) = (0x1017_1000 - NGP_FILE_BASE + rbr_physics::ngp::layout::runtime::BASE, 0x7000);
/// NGP's per-car thermal words in its `.data` (`rbr_hybrid::ngp::tuning`), as running addresses.
const NGP_AIR: u32 = 0x1017_1adc - NGP_FILE_BASE + rbr_physics::ngp::layout::runtime::BASE;
const NGP_RELAX: u32 = 0x1017_1478 - NGP_FILE_BASE + rbr_physics::ngp::layout::runtime::BASE;
const NGP_FACTOR0: u32 = 0x1017_1470 - NGP_FILE_BASE + rbr_physics::ngp::layout::runtime::BASE;
const NGP_FACTOR1: u32 = 0x1017_1474 - NGP_FILE_BASE + rbr_physics::ngp::layout::runtime::BASE;
/// The wheel's spin (`+0xea0`), the car's speed (`+0x15f8` of the wheel's car) and the scratch's
/// convection boost (`+0x1c8`, read when in contact).
const WHEEL_SPIN: u32 = 0xEA0;
const CAR_SPEED: u32 = 0x15F8;
const SCRATCH_BOOST: u32 = 0x1C8;
/// Dump every this many steps (unless `--tyre-probe-every`), plus the final step.
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
    every: u32,
}

impl TyreProbe {
    /// Open `path` for the dump, one every `every` steps ([`EVERY`] if `None`).
    pub fn create(path: &Path, every: Option<u32>) -> Result<Self, String> {
        let file =
            std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
        Ok(TyreProbe { file, path: path.display().to_string(), every: every.unwrap_or(EVERY) })
    }

    /// Dump after the step at `step` when it is due (every `every` steps), or always when `last`.
    pub fn step(&mut self, mem: &Mem, step: u32, last: bool) -> Result<(), String> {
        if !last && !step.is_multiple_of(self.every) {
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
            let scratch_at = wheel + TYRE_SCRATCH;
            let def = mem.u32(scratch_at + 0x10);
            let frame = mem.load_frame(scratch_at);
            let (s, st, d) = (&frame.scratch, &frame.state, &frame.setup);
            writeln!(self.file, "wheel: {k} scratch {scratch_at:#010x} def {def:#010x}").map_err(failed)?;
            let block = wheel + BLOCK_2;
            let (tag1, tag2) = (mem.u32(block + BLOCK_TAG_1), mem.u32(block + BLOCK_TAG_2));
            writeln!(self.file, "block_tags: {tag1} {tag2} {:.6}", mem.f32(block + BLOCK_WEIGHT)).map_err(failed)?;
            let manager = mem.u32(CDEF_GLOBAL) + DEF_MATERIALS;
            for tag in [tag1, tag2, 0] {
                let entry = material::entry(manager, tag);
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
                    mem.u8(entry + MAT_FLAGS),
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

/// A `u32` read that yields `None` when any byte is unmapped (never a panic on a dev capture).
fn try_u32(mem: &Mem, at: u32) -> Option<u32> {
    mem.try_read(at, 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)
}

/// An `f32` read that yields `None` when any byte is unmapped.
fn try_f32(mem: &Mem, at: u32) -> Option<f32> {
    try_u32(mem, at).map(f32::from_bits)
}

/// `--tyre-image FILE` (with `--autopilot`): a memory image of the four wheels' tyre footprints at `step`,
/// for the lab (`crates/rbr-tyre-lab/examples/ngp_transient.rs`). `FILE` holds records `base size bytes`
/// (u32 LE each, as `boot::dump` writes them) for each wheel's scratch, state, setup, wear table and contact
/// record, plus NGP's `.data`; `FILE` with the extension `txt` is a text sidecar with the wheels' addresses
/// and the thermal scalars the lab's `TyreThermal` needs. Read-only: nothing is written to physics memory.
/// The image stays out of the repository (the lab reads it from `/tmp/tyre-lab`).
pub fn write_image(mem: &Mem, step: u32, path: &Path) -> Result<(), String> {
    let mut out = Vec::new();
    let mut meta = format!("# step {step}\n");
    let car = try_u32(mem, CAR_PTR).ok_or("the car's pointer is unmapped")?;
    meta.push_str(&format!("car: {car:#010x}\n"));
    for k in 0..4 {
        let wheel = try_u32(mem, car + WHEELS + 4 * k).ok_or(format!("wheel {k}: unmapped"))?;
        let scratch_at = wheel + TYRE_SCRATCH;
        let (state, def, record, wear) = (
            try_u32(mem, scratch_at + 0x14).ok_or(format!("wheel {k}: state unmapped"))?,
            try_u32(mem, scratch_at + 0x10).ok_or(format!("wheel {k}: setup unmapped"))?,
            try_u32(mem, scratch_at + 0x410).ok_or(format!("wheel {k}: record unmapped"))?,
            try_u32(mem, scratch_at + 0x414).ok_or(format!("wheel {k}: wear table unmapped"))?,
        );
        for (base, len) in [
            (scratch_at, SCRATCH_LEN),
            (state, STATE_LEN),
            (def, DEF_LEN),
            (wear, WEAR_LEN as u32),
            (record, RECORD_LEN),
        ] {
            let bytes = mem.try_read(base, len as usize).ok_or(format!("wheel {k}: {base:#010x}+{len:#x} unmapped"))?;
            out.extend(base.to_le_bytes());
            out.extend(len.to_le_bytes());
            out.extend(bytes);
        }
        let missing = || f32::NAN;
        let in_contact = try_u32(mem, scratch_at + 0x480).unwrap_or(0);
        let boost = (in_contact != 0).then(|| try_f32(mem, scratch_at + SCRATCH_BOOST)).flatten();
        let speed = try_f32(mem, scratch_at + 0x360).unwrap_or_else(missing);
        let fz = try_f32(mem, scratch_at + 0x314).unwrap_or_else(missing);
        let spin = try_f32(mem, wheel + WHEEL_SPIN).unwrap_or_else(missing);
        let car_speed = try_u32(mem, wheel + CAR).and_then(|c| try_f32(mem, c + CAR_SPEED)).unwrap_or_else(missing);
        let kind = try_u32(mem, def + 0x1C).unwrap_or(0);
        meta.push_str(&format!(
            "wheel {k}: wheel {wheel:#010x} scratch {scratch_at:#010x} state {state:#010x} \
             def {def:#010x} wear {wear:#010x} record {record:#010x} kind {kind} \
             spin {spin:.6} car_speed {car_speed:.6} boost {} in_contact {in_contact} fz {fz:.3} speed {speed:.6}\n",
            boost.map_or("nan".into(), |b| format!("{b:.6}")),
        ));
    }
    let bytes = mem.try_read(NGP_DATA.0, NGP_DATA.1).ok_or("NGP's .data is unmapped (not an NGP run)")?;
    out.extend(NGP_DATA.0.to_le_bytes());
    out.extend((NGP_DATA.1 as u32).to_le_bytes());
    out.extend(bytes);
    let num = |at| try_f32(mem, at).unwrap_or(f32::NAN);
    meta.push_str(&format!(
        "ngp: air {:.3} relax_rate {:.6} factors {:.6} {:.6}\n",
        num(NGP_AIR),
        num(NGP_RELAX),
        num(NGP_FACTOR0),
        num(NGP_FACTOR1),
    ));
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))?;
    let sidecar = path.with_extension("txt");
    std::fs::write(&sidecar, meta).map_err(|e| format!("{}: {e}", sidecar.display()))?;
    Ok(())
}
