//! The tyre's typed state on the image: where a tyre's state lives ([`TyreImage`]) with its lift and lower, the car's
//! tyres in typed storage ([`CarTyres`]) and their store ([`TyreView`]), the per-call path ([`with_tyre`]) and the round
//! trip. The setup (`m_def`), the pointers and the wheel's contact angle stay in the image, read per call.

use std::ops::Range;

use crate::mem::{Block, Mem};
use crate::store::defs::{self, Defs};

use super::frame::{settle_material, Outside, Table, WORDS};
use super::frame_types::{ContactFrame, Frame, Indentation, Scratch, Segment, Setup, StateWords};
use super::kernels::{word_at, RECORD, RECORD_CONTACT_ANGLE, WEAR_TABLE};
use super::{
    CONTACT_FRAME, CONTACT_VELOCITY, DEF, DEF_CARCASS_RADIUS, DEF_CARCASS_STIFFNESS_LATERAL,
    DEF_CARCASS_STIFFNESS_TORSIONAL, DEF_CARCASS_WIDTH, DEF_DEBUG_ID, DEF_REFERENCE_FRICTION, DEF_TREAD_COVERAGE,
    DEF_TREAD_DEPTH, DEF_TREAD_WIDTH, LONG_SLIP_VELOCITY, SPEED, SPEED_SQ, SPIN, SPIN_SLIP, STATE,
    STATE_EFFECTIVE_RADIUS, STATE_INNER_AIR_TEMPERATURE, STATE_TREAD_COVERAGE,
};
use crate::tyre::store::TyreStore;
use crate::tyre::surface::SurfaceInput;
use crate::tyre::tyres::{Tyre, TyreId, Tyres};

/// The scratch words a tyre's typed state owns (besides the material block's copied words): the contact point and
/// frame, the patch, the forces and slips, the noise signal and the outputs, the heat words and the soil smoothing.
const OWNED_SCRATCH: [(Range<u32>, &str); 8] = [
    (0x160..0x1A0, "tyre contact"),
    (0x300..0x320, "tyre patch"),
    (0x324..0x408, "tyre forces"),
    (0x420..0x488, "tyre noise signal"),
    (0x490..0x4D4, "tyre outputs"),
    (0x4E0..0x4F0, "tyre heat"),
    (0x4F8..0x500, "tyre heat losses"),
    (0x504..0x510, "tyre soil smoothing"),
];
/// The state words it owns: the active flag, the relaxation counter, the start flag, and from the last point to the
/// sector (the words between are tweak headers and words nothing reads).
const OWNED_STATE: [(Range<u32>, &str); 5] = [
    (0x0C..0x10, "tyre active"),
    (0x18..0x20, "tyre relax count"),
    (0x28..0x78, "tyre state"),
    (0x80..0xCC, "tyre searches"),
    (0xD0..0x138, "tyre temperatures"),
];
/// The material block (`this+0x1a0`, a `CTyreMatData`).
pub const MATERIAL: u32 = 0x1A0;

/// The material block's copied words as byte ranges of the scratch: runs of consecutive words.
fn material_ranges() -> Vec<Range<u32>> {
    let mut words = WORDS;
    words.sort_unstable();
    let mut out: Vec<Range<u32>> = Vec::new();
    for w in words {
        let at = MATERIAL + 4 * w;
        match out.last_mut() {
            Some(last) if last.end == at => last.end = at + 4,
            _ => out.push(at..at + 4),
        }
    }
    out
}

/// Where a tyre's typed state lives in the image: its scratch, the setup, state, contact record and wear table it
/// points at. A wear pointer of 0 (before the wheel sets it, `CScratch::Set414`) holds no wear: it lifts as zeros and
/// lowers nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TyreImage {
    pub scratch: u32,
    pub def: u32,
    pub state: u32,
    pub record: u32,
    pub wear: u32,
}

impl TyreImage {
    /// The tyre whose scratch is at `scratch`, its objects as its pointers give them.
    pub fn of(mem: &Mem, scratch: u32) -> Self {
        TyreImage {
            scratch,
            def: mem.u32(scratch + DEF),
            state: mem.u32(scratch + STATE),
            record: mem.u32(scratch + RECORD),
            wear: mem.u32(scratch + WEAR_TABLE),
        }
    }

    /// The tyre's typed state, read from the image (the scratch words 0x160..0x510 and the state words 0x0c..0x138 each
    /// read at once).
    pub fn lift(&self, mem: &Mem) -> Tyre {
        let (this, state) = (self.scratch, self.state);
        let s = Block::<{ (0x510 - 0x160) / 4 }>::read(mem, this + 0x160);
        let f = |off: u32| s.f32(this + off);
        let scratch = Scratch {
            contact_point: s.vec4(this + 0x160),
            contact_frame: ContactFrame {
                lateral: s.vec4(this + CONTACT_FRAME),
                rolling: s.vec4(this + CONTACT_FRAME + 0x10),
                normal: s.vec4(this + CONTACT_FRAME + 0x20),
            },
            material: std::array::from_fn(|i| s.u32(this + MATERIAL + 4 * WORDS[i])),
            static_friction_dirty: s.u32(this + 0x1E4),
            static_friction: f(0x1E8),
            kinetic_friction_dirty: s.u32(this + 0x1F4),
            kinetic_friction: f(0x1F8),
            static_friction_soil: f(0x208),
            kinetic_friction_soil: f(0x218),
            sink_factor: f(0x228),
            soil_thickness: f(0x258),
            water_factor: f(0x288),
            rolling_resistance: f(0x298),
            lateral_deflection_ratio: f(0x300),
            camber_radius: f(0x304),
            tread_contact_area: f(0x308),
            patch_length: f(0x30C),
            contact_pressure: f(0x310),
            normal_force: f(0x314),
            carcass_force: f(0x318),
            tread_force: f(0x31C),
            total_force_ratio: f(0x324),
            tread_force_share: f(0x328),
            indentation: Indentation::from_words(s.f32s(this + 0x32C)),
            soil_drag: f(0x348),
            lateral_soil_force: f(0x34C),
            longitudinal_soil_force: f(0x350),
            lateral_soil_flow: f(0x354),
            soil_flow_aligned: f(0x358),
            soil_flow_opposed: f(0x35C),
            speed: f(SPEED),
            speed_sq: f(SPEED_SQ),
            contact_velocity: s.f32s(this + CONTACT_VELOCITY),
            long_slip_velocity: f(LONG_SLIP_VELOCITY),
            spin: f(SPIN),
            spin_slip: f(SPIN_SLIP),
            low_speed_a: f(0x380),
            low_speed_b: f(0x384),
            slip_phase: f(0x388),
            cornering_stiffness: f(0x38C),
            traction_stiffness: f(0x390),
            lateral_stiffness: f(0x394),
            torsional_stiffness: f(0x398),
            lateral_force: f(0x39C),
            longitudinal_force: f(0x3A0),
            torsion_torque: f(0x3A4),
            hub_height: f(0x3A8),
            inclination_sin: f(0x3AC),
            inclination_cos: f(0x3B0),
            stiffness_factor: f(0x3B4),
            pneumatic_trail: f(0x3B8),
            tread_remaining: f(0x3BC),
            sector: s.u32(this + 0x3C0),
            wet_term: f(0x3C4),
            rolling_torque: f(0x3C8),
            extra_force: f(0x3CC),
            extra_axle_torque: f(0x3D0),
            extra_term: f(0x3D4),
            slip_area_scale: f(0x3D8),
            sliding_weight: f(0x3DC),
            lateral_phase: f(0x3E0),
            longitudinal_phase: f(0x3E4),
            sliding_deflection: f(0x3E8),
            sliding_torsion: f(0x3EC),
            sticking_weight: f(0x3F0),
            stick_phase_a: f(0x3F4),
            stick_phase_b: f(0x3F8),
            sticking_deflection: f(0x3FC),
            sticking_torsion: f(0x400),
            abrasion_rate: f(0x404),
            wear: if self.wear == 0 { [0.0; 8] } else { mem.f32s(self.wear) },
            signal: std::array::from_fn(|k| s.u32(this + 0x420 + 4 * k as u32)),
            in_contact: s.u32(this + 0x480),
            sector_copy: s.u32(this + 0x484),
            contact_point_copy: s.vec4(this + 0x490),
            motion_copy: s.vec4(this + 0x4A0),
            soil_flow_copy: s.u32(this + 0x4B0),
            long_slip_velocity_copy: s.u32(this + 0x4B4),
            longitudinal_velocity_copy: s.u32(this + 0x4B8),
            temperature_grip: f(0x4BC),
            lateral_friction_force: f(0x4C0),
            longitudinal_friction_force: f(0x4C4),
            debug_words: std::array::from_fn(|k| s.u32(this + 0x4C8 + 4 * k as u32)),
            surface_heat_loss: f(0x4E0),
            frictional_power: f(0x4E4),
            wet_damping: f(0x4EC),
            ngp_heat_power: f(0x4E8),
            tread_heat_loss: f(0x4F8),
            sidewall_heat_loss: f(0x4FC),
            smoothed_soil_drag: f(0x504),
            smoothed_drag_weight: f(0x508),
            smoothed_furrow_ratio: f(0x50C),
        };
        let t = Block::<{ (0x138 - 0x0C) / 4 }>::read(mem, state + 0x0C);
        let st = |off: u32| t.f32(state + off);
        let entry = |k: u32, word: u32| t.f32(state + 0xD0 + 12 * k + 4 * word);
        let segment = |at: u32| Segment { lo: t.u32(at), hi: t.u32(at + 4), frac: t.f32(at + 8) };
        let surface_temperature = std::array::from_fn(|k| entry(k as u32, 0));
        // The entries' second and third words in order; the last third word is the tread temperature.
        let sector_spare = std::array::from_fn(|i| entry(i as u32 / 2, 1 + i as u32 % 2));
        let state_words = StateWords {
            active: t.u32(state + 0x0C),
            relax_dirty: t.u32(state + 0x18),
            relax_count: t.u32(state + 0x1C),
            started_dirty: t.u32(state + 0x28),
            started: t.u32(state + 0x2C),
            last_point: t.vec4(state + 0x30),
            lateral_deflection: st(0x40),
            torsion_angle: st(0x44),
            penetration_depth: st(0x48),
            effective_radius: st(STATE_EFFECTIVE_RADIUS),
            vertical_stiffness: st(0x50),
            tread_elasticity: st(0x54),
            tread_coverage: st(STATE_TREAD_COVERAGE),
            grip_clamped: t.u32(state + 0x5C),
            inner_air_mass: st(0x60),
            inner_air_temperature: st(STATE_INNER_AIR_TEMPERATURE),
            air_pressure: st(0x68),
            inner_air_volume: st(0x6C),
            tread_engaged: t.u32(state + 0x70),
            carcass_engaged: t.u32(state + 0x74),
            hold_direction: t.vec4(state + 0x80),
            pressure_segment: segment(state + 0x90),
            load_segment: segment(state + 0x9C),
            surface_temperature_segment: segment(state + 0xA8),
            tread_temperature_segment: segment(state + 0xB4),
            contact_pressure_segment: segment(state + 0xC0),
            surface_temperature,
            sector_spare,
            tread_temperature: entry(7, 2),
            carcass_temperature: st(0x130),
            sector: t.u32(state + 0x134),
        };
        Tyre { scratch, state: state_words }
    }

    /// Writes the tyre's typed state back: every owned word, nothing else.
    pub fn lower(&self, tyre: &Tyre, mem: &mut Mem) {
        let (this, state) = (self.scratch, self.state);
        let s = &tyre.scratch;
        // The material block first: ten of its words are scalars below, which hold the latest values.
        for (i, word) in s.material.iter().enumerate() {
            mem.set_u32(this + MATERIAL + 4 * WORDS[i], *word);
        }
        let mut set = |off: u32, value: f32| mem.set_f32(this + off, value);
        set(0x1E8, s.static_friction);
        set(0x1F8, s.kinetic_friction);
        set(0x208, s.static_friction_soil);
        set(0x218, s.kinetic_friction_soil);
        set(0x228, s.sink_factor);
        set(0x258, s.soil_thickness);
        set(0x288, s.water_factor);
        set(0x298, s.rolling_resistance);
        set(0x300, s.lateral_deflection_ratio);
        set(0x304, s.camber_radius);
        set(0x308, s.tread_contact_area);
        set(0x30C, s.patch_length);
        set(0x310, s.contact_pressure);
        set(0x314, s.normal_force);
        set(0x318, s.carcass_force);
        set(0x31C, s.tread_force);
        set(0x324, s.total_force_ratio);
        set(0x328, s.tread_force_share);
        for (k, v) in s.indentation.to_words().into_iter().enumerate() {
            set(0x32C + 4 * k as u32, v);
        }
        set(0x348, s.soil_drag);
        set(0x34C, s.lateral_soil_force);
        set(0x350, s.longitudinal_soil_force);
        set(0x354, s.lateral_soil_flow);
        set(0x358, s.soil_flow_aligned);
        set(0x35C, s.soil_flow_opposed);
        set(SPEED, s.speed);
        set(SPEED_SQ, s.speed_sq);
        for (k, v) in s.contact_velocity.into_iter().enumerate() {
            set(CONTACT_VELOCITY + 4 * k as u32, v);
        }
        set(LONG_SLIP_VELOCITY, s.long_slip_velocity);
        set(SPIN, s.spin);
        set(SPIN_SLIP, s.spin_slip);
        set(0x380, s.low_speed_a);
        set(0x384, s.low_speed_b);
        set(0x388, s.slip_phase);
        set(0x38C, s.cornering_stiffness);
        set(0x390, s.traction_stiffness);
        set(0x394, s.lateral_stiffness);
        set(0x398, s.torsional_stiffness);
        set(0x39C, s.lateral_force);
        set(0x3A0, s.longitudinal_force);
        set(0x3A4, s.torsion_torque);
        set(0x3A8, s.hub_height);
        set(0x3AC, s.inclination_sin);
        set(0x3B0, s.inclination_cos);
        set(0x3B4, s.stiffness_factor);
        set(0x3B8, s.pneumatic_trail);
        set(0x3BC, s.tread_remaining);
        set(0x3C4, s.wet_term);
        set(0x3C8, s.rolling_torque);
        set(0x3CC, s.extra_force);
        set(0x3D0, s.extra_axle_torque);
        set(0x3D4, s.extra_term);
        set(0x3D8, s.slip_area_scale);
        set(0x3DC, s.sliding_weight);
        set(0x3E0, s.lateral_phase);
        set(0x3E4, s.longitudinal_phase);
        set(0x3E8, s.sliding_deflection);
        set(0x3EC, s.sliding_torsion);
        set(0x3F0, s.sticking_weight);
        set(0x3F4, s.stick_phase_a);
        set(0x3F8, s.stick_phase_b);
        set(0x3FC, s.sticking_deflection);
        set(0x400, s.sticking_torsion);
        set(0x404, s.abrasion_rate);
        set(0x4BC, s.temperature_grip);
        set(0x4C0, s.lateral_friction_force);
        set(0x4C4, s.longitudinal_friction_force);
        set(0x4E0, s.surface_heat_loss);
        set(0x4E4, s.frictional_power);
        set(0x4EC, s.wet_damping);
        set(0x4E8, s.ngp_heat_power);
        set(0x4F8, s.tread_heat_loss);
        set(0x4FC, s.sidewall_heat_loss);
        set(0x504, s.smoothed_soil_drag);
        set(0x508, s.smoothed_drag_weight);
        set(0x50C, s.smoothed_furrow_ratio);
        if self.wear != 0 {
            for (k, v) in s.wear.into_iter().enumerate() {
                mem.set_f32(self.wear + 4 * k as u32, v);
            }
        }
        for (k, word) in s.signal.iter().enumerate() {
            mem.set_u32(this + 0x420 + 4 * k as u32, *word);
        }
        mem.set_u32(this + 0x480, s.in_contact);
        mem.set_u32(this + 0x484, s.sector_copy);
        s.contact_point_copy.store(mem, this + 0x490);
        s.motion_copy.store(mem, this + 0x4A0);
        mem.set_u32(this + 0x4B0, s.soil_flow_copy);
        mem.set_u32(this + 0x4B4, s.long_slip_velocity_copy);
        mem.set_u32(this + 0x4B8, s.longitudinal_velocity_copy);
        for (k, word) in s.debug_words.iter().enumerate() {
            mem.set_u32(this + 0x4C8 + 4 * k as u32, *word);
        }
        s.contact_point.store(mem, this + 0x160);
        s.contact_frame.lateral.store(mem, this + CONTACT_FRAME);
        s.contact_frame.rolling.store(mem, this + CONTACT_FRAME + 0x10);
        s.contact_frame.normal.store(mem, this + CONTACT_FRAME + 0x20);
        mem.set_u32(this + 0x1E4, s.static_friction_dirty);
        mem.set_u32(this + 0x1F4, s.kinetic_friction_dirty);
        mem.set_u32(this + 0x3C0, s.sector);

        let st = &tyre.state;
        mem.set_u32(state + 0x0C, st.active);
        mem.set_u32(state + 0x18, st.relax_dirty);
        mem.set_u32(state + 0x1C, st.relax_count);
        mem.set_u32(state + 0x28, st.started_dirty);
        mem.set_u32(state + 0x2C, st.started);
        st.last_point.store(mem, state + 0x30);
        let words = [
            st.lateral_deflection,
            st.torsion_angle,
            st.penetration_depth,
            st.effective_radius,
            st.vertical_stiffness,
            st.tread_elasticity,
            st.tread_coverage,
        ];
        for (k, v) in words.into_iter().enumerate() {
            mem.set_f32(state + 0x40 + 4 * k as u32, v);
        }
        mem.set_u32(state + 0x5C, st.grip_clamped);
        let air = [st.inner_air_mass, st.inner_air_temperature, st.air_pressure, st.inner_air_volume];
        for (k, v) in air.into_iter().enumerate() {
            mem.set_f32(state + 0x60 + 4 * k as u32, v);
        }
        mem.set_u32(state + 0x70, st.tread_engaged);
        mem.set_u32(state + 0x74, st.carcass_engaged);
        st.hold_direction.store(mem, state + 0x80);
        let segments = [
            (0x90, st.pressure_segment),
            (0x9C, st.load_segment),
            (0xA8, st.surface_temperature_segment),
            (0xB4, st.tread_temperature_segment),
            (0xC0, st.contact_pressure_segment),
        ];
        for (at, seg) in segments {
            mem.set_u32(state + at, seg.lo);
            mem.set_u32(state + at + 4, seg.hi);
            mem.set_f32(state + at + 8, seg.frac);
        }
        for (k, temperature) in st.surface_temperature.iter().enumerate() {
            mem.set_f32(state + 0xD0 + 12 * k as u32, *temperature);
        }
        for (i, word) in st.sector_spare.iter().enumerate() {
            mem.set_f32(state + 0xD0 + 12 * (i as u32 / 2) + 4 + 4 * (i as u32 % 2), *word);
        }
        mem.set_f32(state + 0x12C, st.tread_temperature);
        mem.set_f32(state + 0x130, st.carcass_temperature);
        mem.set_u32(state + 0x134, st.sector);
    }

    /// The tyre's setup, read from the image (the words 0x1c..0x314 at once).
    pub fn setup(&self, mem: &Mem) -> Setup {
        setup_at(mem, self.def)
    }

    /// The frame of the tyre whose state is `tyre`: its setup and the contact angle from the image.
    pub fn frame(&self, mem: &Mem, tyre: &Tyre) -> Frame {
        self.frame_with(mem, tyre, self.setup(mem))
    }

    /// The frame of the tyre whose state is `tyre` with `setup`, the contact angle from the image.
    fn frame_with(&self, mem: &Mem, tyre: &Tyre, setup: Setup) -> Frame {
        Frame {
            scratch: tyre.scratch,
            state: tyre.state,
            setup,
            contact_angle: mem.f32(self.record + RECORD_CONTACT_ANGLE),
        }
    }

    /// The surface friction's inputs (see [`TyreStore::load_surface_input`]): the setup's tables at `+0x2c4` (base),
    /// `+0x2e0` (wet slope) and `+0x2f8` (wear factor) at the class indices, which are words (so the address wraps as
    /// the original's), and its design wetness `+0x2dc`; the wear of the sector `tyre` names.
    /// `held` is the setup's typed definition, if the step holds it: a word it holds is its, any other the image's.
    fn surface_input(
        &self,
        mem: &Mem,
        held: Option<&TyreDef>,
        tyre: &Tyre,
        classes: [u32; 2],
        wetness: [f32; 2],
    ) -> Result<SurfaceInput, Outside> {
        let def = self.def;
        let [first, second] = classes;
        let wear = tyre.scratch.wear;
        let sector = tyre.state.sector;
        let sector_wear = *wear.get(sector as usize).ok_or(Outside { index: sector as i32, len: wear.len() })?;
        let read = |table: u32, k: u32| {
            let image = || mem.u32(word_at(def + table, k));
            f32::from_bits(match held.and_then(|d| d.surface(table, k)) {
                Some(v) => defs::checked(v.to_bits(), image),
                None => image(),
            })
        };
        Ok(SurfaceInput {
            base_first: read(BASE_FRICTION, first),
            base_second: read(BASE_FRICTION, second),
            wet_slope_first: read(WET_SLOPE, first),
            wet_slope_second: read(WET_SLOPE, second),
            wear_factor_first: read(WEAR_FACTOR, first),
            wear_factor_second: read(WEAR_FACTOR, second),
            design_wetness: read(DESIGN_WETNESS, 0),
            wetness_first: wetness[0],
            wetness_second: wetness[1],
            sector_wear,
        })
    }

    /// The owned byte ranges with their names.
    pub fn owned(&self) -> impl Iterator<Item = (Range<u32>, &'static str)> {
        let at = |base: u32| move |(r, name): (Range<u32>, &'static str)| (base + r.start..base + r.end, name);
        let material = material_ranges().into_iter().map(|r| (r, "tyre material"));
        OWNED_SCRATCH
            .into_iter()
            .chain(material)
            .map(at(self.scratch))
            .chain(OWNED_STATE.into_iter().map(at(self.state)))
            .chain((self.wear != 0).then_some((self.wear..self.wear + 0x20, "tyre wear")))
    }
}

/// The setup at `def`, read from the image (the words 0x1c..0x314 at once).
fn setup_at(mem: &Mem, def: u32) -> Setup {
    let d = Block::<{ (0x314 - DEF_DEBUG_ID as usize) / 4 }>::read(mem, def + DEF_DEBUG_ID);
    let table = |at: u32, len: usize| Table {
        below: d.f32(at - 4),
        len,
        entries: std::array::from_fn(|k| if k < len { d.f32(at + 4 * k as u32) } else { 0.0 }),
    };
    Setup {
        debug_id: d.u32(def + DEF_DEBUG_ID),
        pressure_ratio: d.f32(def + 0x2C),
        reference_friction: d.f32(def + DEF_REFERENCE_FRICTION),
        carcass_stiffness_lateral: d.f32(def + DEF_CARCASS_STIFFNESS_LATERAL),
        carcass_stiffness_torsional: d.f32(def + DEF_CARCASS_STIFFNESS_TORSIONAL),
        carcass_radius: d.f32(def + DEF_CARCASS_RADIUS),
        tread_depth: d.f32(def + DEF_TREAD_DEPTH),
        carcass_thickness: d.f32(def + 0x9C),
        tread_width: d.f32(def + DEF_TREAD_WIDTH),
        tread_coverage: d.f32(def + DEF_TREAD_COVERAGE),
        carcass_width: d.f32(def + DEF_CARCASS_WIDTH),
        sidewall_height: d.f32(def + 0xEC),
        wet_wear_factor: d.f32(def + 0x310),
        inclination_heat_scale: d.f32(def + 0x10C),
        surface_capacity: d.f32s(def + 0x110),
        tread_capacity: d.f32s(def + 0x11C),
        carcass_capacity: d.f32s(def + 0x128),
        tread_conductivity: d.f32(def + 0x118),
        carcass_conductivity: d.f32(def + 0x124),
        pressure_axis: table(def + 0x134, 4),
        stiffness_factor_by_pressure: table(def + 0x144, 4),
        contact_pressure_axis: table(def + 0x154, 6),
        contact_conductance: table(def + 0x16C, 6),
        temperature_axis: table(def + 0x184, 8),
        rubber_friction_by_temperature: table(def + 0x1A4, 8),
        rubber_wear_by_temperature: table(def + 0x1C4, 8),
        load_axis: table(def + 0x1E4, 8),
        cornering_stiffness_by_load: table(def + 0x204, 8),
        traction_stiffness_by_load: table(def + 0x224, 8),
        cornering_friction_by_load: table(def + 0x244, 8),
        traction_friction_by_load: table(def + 0x264, 8),
        cornering_peak_slip_by_load: table(def + 0x284, 8),
        traction_peak_slip_by_load: table(def + 0x2A4, 8),
    }
}

/// The setup's surface tables, by their offset: the base frictions, the design wetness, the wet slopes and the wear
/// factors (six surface classes each).
const BASE_FRICTION: u32 = 0x2C4;
const DESIGN_WETNESS: u32 = 0x2DC;
const WET_SLOPE: u32 = 0x2E0;
const WEAR_FACTOR: u32 = 0x2F8;
const CLASSES: usize = 6;
/// The four by-load tables (stiffnesses and frictions) NGP's step wrapper scales by the surface around `Step` and
/// restores after it (`rbr_hybrid::ngp::steps::step_wrapper`): a step writes them, so they stay on the image.
const SCALED: Range<u32> = 0x204..0x284;
/// The setup's bytes [`TyreDef`] holds: what a step reads (the frame's setup, the hub offset, the surface tables) but
/// [`SCALED`].
pub const DEF_BYTES: [Range<u32>; 2] = [DEF_DEBUG_ID..SCALED.start, SCALED.end..0x314];

/// The by-load tables at `def` ([`SCALED`]), from the image, as [`setup_at`] reads them.
fn scaled_tables(mem: &Mem, def: u32) -> [Table; 4] {
    let words: [f32; 0x21] = mem.f32s(def + SCALED.start - 4);
    std::array::from_fn(|k| Table {
        below: words[8 * k],
        len: 8,
        entries: std::array::from_fn(|i| words[8 * k + 1 + i]),
    })
}

/// A tyre's setup (`m_def`) as a step reads it, held read only with the step's definitions (`store::defs`): the
/// frame's setup, the `HubOffset` the wheel reads (`+0x6c`) and the surface tables. The setup's by-load tables are
/// read on the image ([`TyreDef::setup`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TyreDef {
    setup: Setup,
    pub hub_offset: f32,
    pub base_friction: [f32; CLASSES],
    pub design_wetness: f32,
    pub wet_slope: [f32; CLASSES],
    pub wear_factor: [f32; CLASSES],
}

impl TyreDef {
    /// The setup at `def`, from the image.
    pub fn lift(mem: &Mem, def: u32) -> Self {
        TyreDef {
            setup: setup_at(mem, def),
            hub_offset: mem.f32(def + crate::wheel::layout::TYRE_DEF_HUB_OFFSET),
            base_friction: mem.f32s(def + BASE_FRICTION),
            design_wetness: mem.f32(def + DESIGN_WETNESS),
            wet_slope: mem.f32s(def + WET_SLOPE),
            wear_factor: mem.f32s(def + WEAR_FACTOR),
        }
    }

    /// The setup at `def` (this one's address): the held words, the by-load tables from `mem`.
    pub fn setup(&self, mem: &Mem, def: u32) -> Setup {
        let [cornering_stiffness, traction_stiffness, cornering_friction, traction_friction] = scaled_tables(mem, def);
        Setup {
            cornering_stiffness_by_load: cornering_stiffness,
            traction_stiffness_by_load: traction_stiffness,
            cornering_friction_by_load: cornering_friction,
            traction_friction_by_load: traction_friction,
            ..self.setup
        }
    }

    /// The setup's `CarcassWidth`.
    pub fn carcass_width(&self) -> f32 {
        self.setup.carcass_width
    }

    /// Entry `k` of the surface table at `table` (`k` 0 of the design wetness), `None` past it.
    fn surface(&self, table: u32, k: u32) -> Option<f32> {
        let k = k as usize;
        match table {
            BASE_FRICTION => self.base_friction.get(k).copied(),
            DESIGN_WETNESS => (k == 0).then_some(self.design_wetness),
            WET_SLOPE => self.wet_slope.get(k).copied(),
            WEAR_FACTOR => self.wear_factor.get(k).copied(),
            _ => None,
        }
    }

    /// Every answer of the setup at `def` against the image's: the setup, the hub offset and each table entry. The
    /// bytes compared, or the first answer that differs.
    pub fn differential(&self, mem: &Mem, def: u32) -> Result<usize, String> {
        if self.setup(mem, def) != setup_at(mem, def) {
            return Err(format!("tyre setup at {def:#010x}: the typed setup differs from the image's"));
        }
        let mut bytes = DEF_BYTES.iter().map(|r| (r.end - r.start) as usize).sum::<usize>();
        let mut words = vec![(crate::wheel::layout::TYRE_DEF_HUB_OFFSET, self.hub_offset)];
        for table in [BASE_FRICTION, WET_SLOPE, WEAR_FACTOR] {
            words.extend((0..CLASSES as u32).map(|k| (table + 4 * k, self.surface(table, k).unwrap())));
        }
        words.push((DESIGN_WETNESS, self.design_wetness));
        for (off, typed) in words {
            if typed.to_bits() != mem.u32(def + off) {
                return Err(format!("tyre setup at {def:#010x} +{off:#x}: typed {:#x}", typed.to_bits()));
            }
            bytes += 4;
        }
        Ok(bytes)
    }
}

/// The frame of the tyre whose scratch is at `scratch`, read from the image: for the readers between steps.
pub fn frame_of(mem: &Mem, scratch: u32) -> Frame {
    let image = TyreImage::of(mem, scratch);
    image.frame(mem, &image.lift(mem))
}

/// Tyres in typed storage: where each lives in the image and its state, by slot ([`TyreId`]). A car's are by wheel
/// ([`CarTyres::lift`]); the per-call path holds one ([`with_tyre`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CarTyres {
    pub images: [Option<TyreImage>; 4],
    pub tyres: Tyres,
}

impl CarTyres {
    /// The tyres of the car at `car`, wheel by wheel, lifted from the image.
    pub fn lift(mem: &Mem, car: u32) -> Self {
        let wheels = crate::car_body::store::CarStore::wheel_pointers(mem, car);
        Self::lift_scratches(
            mem,
            wheels.map(|wheel| (wheel != 0).then_some(wheel + crate::wheel::layout::TYRE_SCRATCH)),
        )
    }

    /// The tyres whose scratches are at `scratches`, slot by slot, lifted from the image.
    pub fn lift_scratches(mem: &Mem, scratches: [Option<u32>; 4]) -> Self {
        let images = scratches.map(|s| s.map(|s| TyreImage::of(mem, s)));
        CarTyres { images, tyres: Tyres(images.map(|image| image.map(|image| image.lift(mem)))) }
    }

    /// Writes every tyre's state back to the image.
    pub fn lower(&self, mem: &mut Mem) {
        for (image, tyre) in self.images.iter().zip(&self.tyres.0) {
            if let (Some(image), Some(tyre)) = (image, tyre) {
                image.lower(tyre, mem);
            }
        }
    }

    /// Writes the tyre in slot `id` back to the image, for a call whose callees read its words at their addresses.
    pub fn lower_tyre(&self, id: TyreId, mem: &mut Mem) {
        self.images[id.0].expect("a tyre in its slot").lower(self.tyres.get(id), mem);
    }

    /// Lifts the tyre in slot `id` from the image again, after such a call.
    pub fn lift_tyre(&mut self, id: TyreId, mem: &Mem) {
        *self.tyres.get_mut(id) = self.images[id.0].expect("a tyre in its slot").lift(mem);
    }

    /// The slot of the tyre whose scratch is at `scratch`, if it is one of these.
    pub fn id_of(&self, scratch: u32) -> Option<TyreId> {
        self.images.iter().position(|image| image.is_some_and(|image| image.scratch == scratch)).map(TyreId)
    }

    /// The tyre in slot `id`'s frame.
    pub fn frame(&self, mem: &Mem, id: TyreId) -> Frame {
        self.images[id.0].expect("a tyre in its slot").frame(mem, self.tyres.get(id))
    }

    /// The tyres as a store: their state here, their setups, pointers and contact angles in `mem`.
    pub fn view<'a>(&'a mut self, mem: &'a Mem) -> TyreView<'a> {
        TyreView { mem, images: &self.images, tyres: &mut self.tyres, defs: None }
    }

    /// Every owned byte range, named by its wheel slot and part.
    pub fn owned(&self) -> Vec<(Range<u32>, String)> {
        let slots = self.images.iter().enumerate().filter_map(|(k, image)| image.map(|image| (k, image)));
        slots.flat_map(|(k, image)| image.owned().map(move |(r, name)| (r, format!("wheel{k} {name}")))).collect()
    }
}

/// Runs `run` on the tyre whose scratch is at `scratch`, lifted into a store of its own and lowered after it: the
/// per-call path, on which the image holds the tyre between calls. What `run`'s caller writes through its pointers goes
/// to the image after the lower, so a pointer into the tyre's own words keeps its write.
pub fn with_tyre<T>(mem: &mut Mem, scratch: u32, run: impl FnOnce(&mut TyreView, TyreId) -> T) -> T {
    let mut one = CarTyres::lift_scratches(mem, [Some(scratch), None, None, None]);
    let out = run(&mut one.view(mem), TyreId(0));
    one.lower(mem);
    out
}

/// `lower(lift(image))` of the car's tyres into an empty image, compared with `mem` on every owned byte: their count, or
/// the first byte that differs.
pub fn round_trip(mem: &Mem, car: u32) -> Result<usize, String> {
    let tyres = CarTyres::lift(mem, car);
    let mut out = Mem::new();
    tyres.lower(&mut out);
    let mut bytes = 0;
    for (range, name) in tyres.owned() {
        let len = (range.end - range.start) as usize;
        let (want, got) = (mem.read(range.start, len), out.read(range.start, len));
        if let Some(k) = want.iter().zip(&got).position(|(a, b)| a != b) {
            return Err(format!("{name} at {:#010x}: +{k:#x} differs after lower(lift)", range.start));
        }
        bytes += len;
    }
    Ok(bytes)
}

/// Tyres in typed storage as a [`TyreStore`]: the state in [`CarTyres`], the setups the step's typed definitions hold
/// ([`TyreView::with_defs`]); the other setups, the pointers and the wheels' contact angles read from the image.
pub struct TyreView<'a> {
    mem: &'a Mem,
    images: &'a [Option<TyreImage>; 4],
    tyres: &'a mut Tyres,
    defs: Option<&'a Defs>,
}

impl<'a> TyreView<'a> {
    /// The view with the step's typed definitions (`None`: every setup on the image).
    pub fn with_defs(self, defs: Option<&'a Defs>) -> Self {
        TyreView { defs, ..self }
    }
}

impl TyreView<'_> {
    fn image(&self, tyre: TyreId) -> &TyreImage {
        self.images[tyre.0].as_ref().expect("a tyre in its slot")
    }

    /// The typed setup of the tyre `tyre`, where the step holds it.
    fn held(&self, tyre: TyreId) -> Option<&TyreDef> {
        self.defs?.tyre(self.image(tyre).def)
    }
}

impl TyreStore for TyreView<'_> {
    type Tyre = TyreId;

    fn load_frame(&self, tyre: TyreId) -> Frame {
        let image = self.image(tyre);
        let setup = match self.held(tyre) {
            Some(def) => defs::checked(def.setup(self.mem, image.def), || image.setup(self.mem)),
            None => image.setup(self.mem),
        };
        image.frame_with(self.mem, self.tyres.get(tyre), setup)
    }

    fn store_frame(&mut self, tyre: TyreId, frame: &Frame) {
        let mut scratch = frame.scratch;
        // The scalars into the material block's words, as a lower and a lift leave them.
        settle_material(&mut scratch);
        *self.tyres.get_mut(tyre) = Tyre { scratch, state: frame.state };
    }

    fn load_surface_input(&self, tyre: TyreId, classes: [u32; 2], wetness: [f32; 2]) -> Result<SurfaceInput, Outside> {
        self.image(tyre).surface_input(self.mem, self.held(tyre), self.tyres.get(tyre), classes, wetness)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRATCH: u32 = 0x10000;
    const STATE: u32 = 0x20000;
    const DEFINITION: u32 = 0x30000;
    const RECORD_AT: u32 = 0x40000;
    const WEAR: u32 = 0x50000;

    /// The tyre's objects mapped with `pattern`-derived bytes, the scratch's pointers set.
    fn image(pattern: u8) -> Mem {
        let mut mem = Mem::new();
        let bytes = |n: u32, salt: u8| (0..n).map(|k| (k as u8).wrapping_mul(29) ^ salt ^ pattern).collect::<Vec<_>>();
        mem.map(SCRATCH, &bytes(0x520, 1));
        mem.map(STATE, &bytes(0x140, 2));
        mem.map(DEFINITION, &bytes(0x320, 3));
        mem.map(RECORD_AT + 0x5A0, &bytes(0x20, 4));
        mem.map(WEAR - 0x10, &bytes(0x40, 5));
        for (at, value) in [(DEF, DEFINITION), (super::STATE, STATE), (RECORD, RECORD_AT), (WEAR_TABLE, WEAR)] {
            mem.set_u32(SCRATCH + at, value);
        }
        mem
    }

    #[test]
    fn owned_ranges_are_ordered_and_apart() {
        let image = TyreImage::of(&image(0), SCRATCH);
        let mut ranges: Vec<_> = image.owned().map(|(r, _)| r).collect();
        ranges.sort_by_key(|r| r.start);
        assert!(ranges.windows(2).all(|w| w[0].end <= w[1].start), "{ranges:x?}");
        // The pointers are not owned.
        for at in [DEF, super::STATE, RECORD, WEAR_TABLE] {
            assert!(!ranges.iter().any(|r| r.contains(&(SCRATCH + at))), "{at:#x}");
        }
    }

    #[test]
    fn lower_writes_the_lifted_owned_words_and_nothing_else() {
        let a = image(0);
        let image_a = TyreImage::of(&a, SCRATCH);
        let tyre = image_a.lift(&a);
        let mut b = image(0x5a);
        image_a.lower(&tyre, &mut b);
        let owned: Vec<_> = image_a.owned().map(|(r, _)| r).collect();
        let blocks =
            [(SCRATCH, 0x520), (STATE, 0x140), (DEFINITION, 0x320), (RECORD_AT + 0x5A0, 0x20), (WEAR - 0x10, 0x40)];
        for (base, len) in blocks {
            for at in base..base + len {
                let want = if owned.iter().any(|r| r.contains(&at)) { &a } else { &image(0x5a) };
                assert_eq!(b.u8(at), want.u8(at), "{at:#x}");
            }
        }
        assert_eq!(TyreImage::of(&b, SCRATCH).lift(&b), tyre);
    }

    #[test]
    fn the_view_stores_the_frame_as_lower_and_lift_leave_it() {
        let mut mem = image(0);
        let mut tyres = CarTyres::lift_scratches(&mem, [Some(SCRATCH), None, None, None]);
        let mut view = tyres.view(&mem);
        let mut frame = view.load_frame(TyreId(0));
        frame.scratch.static_friction = 1.25;
        frame.state.relax_count = 7;
        view.store_frame(TyreId(0), &frame);
        tyres.lower(&mut mem);
        assert_eq!(CarTyres::lift_scratches(&mem, [Some(SCRATCH), None, None, None]), tyres);
    }
}
