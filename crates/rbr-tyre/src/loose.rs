//! Loose surfaces, loose-v1 (ours, experimental): the brush's patch ([`brush::patch`]) on soil (gravel and snow), with
//! a flat friction and a ploughing berm in place of the hard surface's friction against sliding speed. The game takes
//! it on soil wheels under the brush ([`crate::car`], `--physics experimental-brush` only); NGP's soil lateral force
//! stays on top (`rbr_physics::tyre::model::apply`), on gravel and snow held at its value at [`GRAVEL_SOIL_SPEED`]
//! and [`SNOW_SOIL_SPEED`] ([`gravel_soil_share`], [`snow_soil_share`]).
//!
//! - Level: NGP's, on gravel times [`gravel_depth_level`], along the wheel times [`GRAVEL_LONGITUDINAL`] and across it
//!   times [`GRAVEL_LATERAL`], on snow times [`SNOW_LONGITUDINAL`] and [`SNOW_LATERAL`]. The free
//!   rolling peaks μ_y and μ_x at each load of the wheel's axis are NGP's own, swept from its
//!   tables on a unit surface ([`crate::fit::loose`]), interpolated over the load and scaled by NGP's grip (static
//!   friction times the temperature grip). With a flat friction the brush's peak is μ Fz exactly, so no solve is needed.
//!   Damp gravel takes NGP's dry level back ([`gravel_damp_level`]); wet keeps NGP's.
//! - Shape below the level, gravel: Janosi-Hanamoto's shear law of soil (τ = τ_max (1 - exp(-j/K)); Wong, Theory of
//!   Ground Vehicles, not read: secondary via Senatore & Sandu, J. Terramech. 48, 2011) with the shear displacement
//!   taken as the theoretical slip times a fixed length: μ Fz (1 - exp(-u)) along the slip, u the slip over
//!   tan [`GRAVEL_ALPHA0_DEG`] ([`shear`]). Coutermarsh & Shoop (SAE 2010-01-0374, CRREL CIV, LT235/75R15, 241 kPa,
//!   6.7 kN, 1-1.5 m/s, Fig. 15, fig ±0.004): pea stone over its maximum 0.33/0.55/0.80/0.91 at 2.5/5/10/15°, still
//!   rising to 38°, fits it with α0 6.28° (rms 0.0012; the brush's cubic 0.0165, the law integrated over a uniform
//!   patch 0.0098); wet gravel fits 9.7° (0.23/0.40/0.65/0.79). That is NGP's deeper gravel (25-30 mm); a shallower
//!   loose layer builds force faster, dry α0 3.4° at 9 mm and 4.8° at 17.5 mm ([`GRAVEL_DEPTH_ALPHA0_DEG`]), and
//!   NGP's WaterFactor scales it by up to 9.7 / 6.3, linear (ours; [`gravel_alpha0_deg`]). No peak, no fall; the
//!   level is approached (0.995 at 30°, 0.973 wet). At stage speed the one curve, Croft-White's WRC telemetry on wet
//!   loose gravel (PhD, Cranfield 2006, Figs 4.32-4.33, digitised: fig ±0.05, inferred), saturates by 9-22° (90 %) and
//!   does not fall to 32° (0.96-1.06 of the 10-24° maximum; his tarmac curves fall to 0.71-0.83 by the same method, the
//!   bench data's 0.73-0.90), so the law takes no speed. The slope follows
//!   the level, μ Fz / tan α0 (6.0 per rad at 6.7 kN against NGP's 6.6), not NGP's stiffness. The longitudinal side
//!   takes the same law with the brush's Cx/Cy ([`KX_OVER_KY`]) at the axes' levels, so the braking force keeps
//!   rising past NGP's peak slip (peak κ 0.58 on the 0-0.8 grid). Only NGP's gravel materials (surface class
//!   [`GRAVEL_CLASS`], `Gravel_*`), by their share in the wheel's material blend ([`class_share`]).
//! - Shape, snow and the other soils: the brush's patch with a flat friction: the brush's peak is the level times Fz
//!   exactly. Packed snow rises faster (Coutermarsh Fig. 8: 90 % by 7-9°; α0 3.6° at a slope NGP's level doubles) and
//!   the brush at NGP's slope gives it (α90 9°). The cornering slope stays NGP's (about twice that road tyre's 3-5 per
//!   rad): the stiffness is solved for it under the flat friction ([`calibrate`]).
//! - Snow along the wheel builds its force in two stages ([`SNOW_HELD_LEVEL`]): the tread on the snow, and the snow
//!   held in the tread shearing on the road's, softer. Two patches side by side, each with its share of the load, so
//!   the side force alone stays the one patch's.
//! - Longitudinal: past the free rolling peak the braking force keeps rising, to [`PLOUGH`] times the peak at lock: the
//!   material builds up ahead of a locked tyre (a berm) and stops it harder than rolling (NHTSA DOT HS 808 875 /
//!   SAE 1999-01-1287, 9 cars, 56 km/h, 5 cm crushed limestone: locked over ABS decel 1.25 laden, 1.30 light, per car
//!   1.12-1.42; Macnabb ESV 98 1.21-1.59; Coutermarsh: locked fronts on pea stone Fx/Fz 0.85 against a free rolling
//!   lateral peak 0.54-0.57 and a traction coefficient 0.58-0.68). The berm grows with the braking slip
//!   ([`braking_slip`]) from [`BERM_FROM`] (or the patch's full slide, if later) to lock, a smoothstep; between the
//!   full slide and there the force stays at the peak. No intermediate points are published; NHTSA's mechanism is the
//!   locked tyre's berm, and a berm that grows from the full slide holds the wheels off lock at the Fiesta's full-pedal
//!   brake torque (note, "Loose surfaces: loose-v1"). Driving
//!   slip builds no berm here: the data are braked, and the motion resistance of a spinning tyre's sinkage is not.
//! - Snow (surface class [`SNOW_CLASS`], by its share in the blend): no berm ([`SNOW_PLOUGH`] in place of PLOUGH).
//!   Ichihara & Mizoguchi (HRB SR 115, 1969-70, Fig. 3, fig ±0.02): studded snow tyres on normal compacted snow at 20
//!   km/h hold 0.36-0.42 at 15-20 % slip, peak 0.42-0.445 at 40-60 % and lock at 0.40-0.43: locked over the 15-20 %
//!   slip 1.02-1.08, over the peak 0.95-0.97 ("small" differences, their text); snow tyres 0.74-0.95 of the 20 % value.
//!   NHTSA's berm is graded gravel; compacted snow shears off rather than heaping. A rally tyre on snow is studded.
//! - Combined: the berm acts against the sliding velocity, with the friction (Svendenius's collinear slide), so a
//!   locked tyre's side force is PLOUGH μ sin α, linear in the slip angle and below free rolling at every angle
//!   (Coutermarsh, Fig. 16, fig ±0.02: 0.72 sin α against a free rolling 0.55, 1.31 sin α of it; here 1.3 sin α).
//!
//! - The berm on gravel by the loose layer's depth ([`gravel_berm`]): the measured layers are 5 cm of graded gravel,
//!   NGP's gravels 9-30 mm, and a thin layer on a firm bed leaves less to heap up. Linear from none to PLOUGH at
//!   [`BERM_DEPTH`]: 1.09 at 15 mm, where Finland 2021's braking past 50 % wheel slip is 1.07-1.14 of its force at
//!   22-32 % (0.72-0.89 against 0.67-0.78 g; note, "Straight braking"). Other soils keep PLOUGH.
//!
//! Ours: the smoothstep and its start at a braking slip of 0.8, no berm when driving, PLOUGH the published cars'
//! 1.25-1.30, the berm linear in gravel's depth (two points: the 5 cm cars and Finland's road) and independent of
//! speed (no speed trend in the locked-wheel data, 40-80 km/h).

use crate::brush::{self, Calibration, EvalInput, EvalOutput, KX_OVER_KY, M_CX, M_CY, V_MIN};

/// The braking force of a locked tyre over its free rolling peak on loose surfaces (NHTSA: 1.25 laden, 1.30 light).
pub const PLOUGH: f64 = 1.3;
/// The braking force of a locked tyre over its level on snow (surface class [`SNOW_CLASS`]): the level, no berm. On
/// hard packed snow a locked tyre holds 0.93-1.00 of its peak: VTI's trailer (BV12, 30 km/h) 0.93-0.94 (Baliga &
/// Hiregoudar, Chalmers MSc 2018:16, Figs 6.2-6.3, fig ±0.01), 0.93 and 1.00 (Hjort et al., AVEC'24, Fig. 2b, fig
/// ±0.01); studded snow tyres 0.95-0.97 (Ichihara & Mizoguchi, HRB SR 115, Fig. 3, fig ±0.02). The 2017-21 World Rally
/// Cars' braking is level from 32-50 to 50-80 % of wheel slip on four snow rallies (0.91-1.01 of it). 1.05 before
/// was SR 115's locked over its 15-20 % slip value (1.02-1.08), on the one patch, at its level by 13 % slip; the two
/// stages ([`SNOW_HELD_LEVEL`]) hold 0.93-0.98 of the level at 15-20 % (4 kN), so that ratio is theirs without a berm.
pub const SNOW_PLOUGH: f64 = 1.0;
/// The loose layer's depth the published locked stops were measured on, m (NHTSA: 5 cm of crushed limestone).
pub const BERM_DEPTH: f64 = 0.05;
/// The share of the berm a gravel layer of `depth` (m) builds: linear to the whole at [`BERM_DEPTH`] (see the module).
pub fn gravel_berm(depth: f64) -> f64 {
    (depth / BERM_DEPTH).clamp(0.0, 1.0)
}
/// The braking slip the berm starts to grow from (ours: near lock, where the tread stops rolling over the material).
pub const BERM_FROM: f64 = 0.8;

/// NGP's free rolling peak friction on a unit surface at each load: lateral and longitudinal (driving).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Levels {
    /// The loads, N, rising.
    pub load: Vec<f64>,
    pub lateral: Vec<f64>,
    pub longitudinal: Vec<f64>,
}

impl Levels {
    /// The lateral and longitudinal peak friction at `fz`: linear between the loads, the ends held.
    pub fn at(&self, fz: f64) -> (f64, f64) {
        let (l, y, x) = (&self.load, &self.lateral, &self.longitudinal);
        let n = l.len();
        if n == 0 {
            return (0.0, 0.0);
        }
        if fz <= l[0] {
            return (y[0], x[0]);
        }
        if fz >= l[n - 1] {
            return (y[n - 1], x[n - 1]);
        }
        let k = (0..n - 1).find(|&k| fz <= l[k + 1]).unwrap_or(n - 2);
        let f = (fz - l[k]) / (l[k + 1] - l[k]);
        (y[k] + (y[k + 1] - y[k]) * f, x[k] + (x[k + 1] - x[k]) * f)
    }
}

/// One wheel's loose-surface tyre: the brush's stiffness and factors (its friction levels unused) and NGP's levels.
#[derive(Clone, Debug)]
pub struct Loose {
    pub cal: Calibration,
    pub levels: Levels,
    /// The factor on gravel's shear law's stiffness ([`shear`]): 1, or the tuning's ([`crate::tuning::Tuning`]).
    pub shear_stiffness: f64,
}

/// The braking slip, 0 to 1: the share of the contact point's longitudinal speed the patch slides along it (1 locked
/// or turning backwards), 0 when the slip drives. The speed is floored at [`V_MIN`], so it fades to 0 at standstill.
pub fn braking_slip(vsx: f64, vx: f64) -> f64 {
    if vsx * vx > 0.0 {
        (vsx.abs() / vx.abs().max(V_MIN)).min(1.0)
    } else {
        0.0
    }
}

/// The driving slip, 0 to 1: the share of the rolling speed `vr` (floored, [`brush::slip_velocities`]) the patch
/// slides back along it (1 spinning at standstill), 0 when the slip brakes.
pub fn driving_slip(vsx: f64, vx: f64, vr: f64) -> f64 {
    if vsx * vx < 0.0 || (vx == 0.0 && vsx != 0.0) {
        (vsx.abs() / vr).min(1.0)
    } else {
        0.0
    }
}

/// DRIVE TEST ONLY (off unless set): `RERBR_TEST_GRAVEL_SPIN=F` lets gravel's driven force fall past
/// [`SPIN_FROM`] to F times the level at full spin. Read once; one stderr line when set.
fn test_gravel_spin() -> Option<f64> {
    static F: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *F.get_or_init(|| {
        let f = std::env::var("RERBR_TEST_GRAVEL_SPIN").ok()?.parse::<f64>().ok()?.clamp(0.0, 1.0);
        eprintln!("rbr-tyre: RERBR_TEST_GRAVEL_SPIN={f}: gravel's driven force falls to {f} of the level at full spin");
        Some(f)
    })
}

/// The driving slip the drive test's fall starts from: dense sand peaks at ~35 % slip (CRREL 95-1 Fig. 18), a sand
/// rig at 15-25 % (WES TR 3-666); gravel's shear law is at 0.97 of its level by 0.3. A smoothstep to full spin puts
/// 0.88 of the way at 80 % (dense sand 0.87 of its peak there, at F 0.85).
pub const SPIN_FROM: f64 = 0.3;

/// Gravel's shear law reaches 1 - 1/e of the friction at this slip angle on dry deep gravel, degrees ([`shear`]):
/// Coutermarsh & Shoop's pea stone (Fig. 15; 0.64 cm stone on recycled asphalt, the loose depth under the test not
/// stated, hard turns built it up to 10 cm, TR-10-11 p. 18) fitted: 6.28° (rms 0.0012 μ/max over 1-38°), rounded.
/// NGP's deeper gravel (25-30 mm) takes it ([`GRAVEL_DEPTH_ALPHA0_DEG`]).
pub const GRAVEL_ALPHA0_DEG: f64 = 6.3;
/// The same on wet gravel (NGP's WaterFactor 1): Coutermarsh & Shoop's wet gravel (Fig. 15) fitted: 9.7° (rms 0.0015).
/// Its ratio to [`GRAVEL_ALPHA0_DEG`] scales every depth's dry α0.
pub const GRAVEL_WET_ALPHA0_DEG: f64 = 9.7;
/// Dry gravel's α0, degrees, at NGP's loose layer depths, m (`SoilThickness`; shallow 9-15, deep 17.5-22.5, deeper
/// 25-30 mm): linear between, the ends held. A thin layer on a packed base builds force faster than a deep one (ours,
/// the size; the direction: the initial slope rises with the soil's strength, Gee-Clough & Sommer 1981 via El-Razaz
/// 1988; gravel's exponential rate 2.7 times a ploughed field's, Metz 1993; a firm gravel lot over asphalt at α90
/// 2.6-3.3°, Hindiyeh 2013). The anchors are fitted to the α90 at 4 kN, 15 m/s, dry (`force_sweep` on RSF 345's
/// gravel probe): 9 mm 8° (between the firm lot and Croft-White's WRC on a stage road, ~12°), 17.5 mm 11° (that
/// road), 25 mm 14.25° (pea stone).
pub const GRAVEL_DEPTH_ALPHA0_DEG: [(f64, f64); 3] = [(0.009, 3.4), (0.0175, 4.8), (0.025, GRAVEL_ALPHA0_DEG)];

/// Gravel's α0, degrees ([`shear`]), at the loose layer's `depth` (m, NGP's blended `SoilThickness`,
/// [`GRAVEL_DEPTH_ALPHA0_DEG`]) and the surface's `wetness` (NGP's WaterFactor, 0 dry, 0.25 damp, 1 wet): the depth's
/// dry α0 times 1 to [`GRAVEL_WET_ALPHA0_DEG`] / [`GRAVEL_ALPHA0_DEG`], linear in the wetness (ours: the damp point is
/// unmeasured).
pub fn gravel_alpha0_deg(depth: f64, wetness: f64) -> f64 {
    let a = &GRAVEL_DEPTH_ALPHA0_DEG;
    let dry = match a.iter().position(|&(d, _)| depth < d) {
        Some(0) => a[0].1,
        Some(k) => a[k - 1].1 + (a[k].1 - a[k - 1].1) * (depth - a[k - 1].0) / (a[k].0 - a[k - 1].0),
        None => a[a.len() - 1].1,
    };
    dry * (1.0 + (GRAVEL_WET_ALPHA0_DEG / GRAVEL_ALPHA0_DEG - 1.0) * wetness.clamp(0.0, 1.0))
}

/// Gravel's level at damp over NGP's: NGP's damp gravel materials (WaterFactor 0.25) carry the dry friction × 0.925
/// (static and soil static, every `Gravel_*` id, `materials` example), and the brush takes that loss back out, so damp
/// gravel grips as dry; wet keeps NGP's × 0.845-0.8525. A user decision (2026-11-09) on this evidence: the one gravel
/// road measured dry and sprinkled skids locked at 1.06-1.18 of dry (Moyer 1933), nothing measures a damp gravel loss,
/// and wet ordinary gravel roads hold 0.76-0.90 of dry (González 2015, Paige-Green).
pub const GRAVEL_DAMP_LEVEL: f64 = 1.0 / 0.925;

/// The factor on the loose level at the wetness `w` for a `gravel` share of the soil ([`class_share`]): 1 +
/// ([`GRAVEL_DAMP_LEVEL`] - 1) times the damp tent ([`brush::damp_weight`]: 1 at damp, 0 dry and wet, linear in W
/// between) times the share; exactly 1 dry, wet and off gravel.
pub fn gravel_damp_level(w: f64, gravel: f64) -> f64 {
    1.0 + (GRAVEL_DAMP_LEVEL - 1.0) * brush::damp_weight(w) * gravel.clamp(0.0, 1.0)
}

/// Gravel's longitudinal level over NGP's, on the gravel share of the soil ([`class_share`]; grass, rock and snow keep
/// NGP's). A user decision (2026-11-11; `notes/tyre-model.md`, "Straight braking, the level"): on the pad NGP's peak
/// braking is 5-25 % over the saturated braking of the 2017-21 World Rally Cars in Finland, the grippiest of the six
/// gravel rallies; 0.85 puts the tyres' force on Finland's, and 0.9 the car's deceleration, the real cars having
/// 0.08 g more air drag at 115 km/h than the files give. By hand on Finland's road (RSF 524 against the eight cars of
/// 2021's SS7, the same braking zones) a sim driver peaks 6 % over the real crews on NGP's tyre and 8-17 % under them
/// at 0.85, where the same drives' apex speeds are 11-12 % under: 0.9 leaves a driver under the crews in both.
pub const GRAVEL_LONGITUDINAL: f64 = 0.9;

/// Gravel's lateral level over NGP's, on the gravel share of the soil. At NGP's own level the car cornered under
/// NGP's car (i20 Coupe WRC 2020, material 49, the profiles' aero): the ramp steer's peak at 80 km/h 0.871 g against
/// 0.905, the circles of 65 / 100 m 0.89 g at 89 / 109 km/h against 0.92 / 0.93. 1.06 gave 0.912 g and 0.92 / 0.93,
/// after the user found the gravel "quite a bit less grippy in the corners" by hand on RSF 524 (2026-11-11). 1.10, a
/// user decision (2026-11-12) on the whole g the 2017-21 World Rally Cars hold in a slide in Finland (0.97 g from 15
/// deg of sideslip at 60-100 km/h and 1.01 at 100-160, [`gravel_soil_share`]; the car at 1.06 0.94 and 0.98): 0.96
/// and 1.00, the ramp steer's peak 0.942 g, the circles of 65 / 100 / 200 m 0.94 / 0.97 / 0.99 g (were 0.92 / 0.93 /
/// 0.97); the straight braking does not move. Their p90 through corners of 40 / 100 m is 1.17 / 1.06 g on a cambered
/// road.
pub const GRAVEL_LATERAL: f64 = 1.10;

/// The loose layer's depth, m, to which gravel keeps its level (RSF 524's material, Finland's road), the depth of
/// NGP's loosest gravel, and the level there over NGP's ([`gravel_depth_level`]).
pub const GRAVEL_FIRM_DEPTH: f64 = 0.015;
pub const GRAVEL_LOOSE_DEPTH: f64 = 0.030;
pub const GRAVEL_LOOSE_LEVEL: f64 = 0.96;

/// Gravel's level over NGP's at the loose layer's `depth` (m), both axes: 1 to [`GRAVEL_FIRM_DEPTH`], linear to
/// [`GRAVEL_LOOSE_LEVEL`] at [`GRAVEL_LOOSE_DEPTH`]. Under NGP's own values the i20 Coupe WRC brakes and corners 6 %
/// lower on its loosest gravel than on the firm one (static friction 0.8925 to 0.865, and the slower rise); the six
/// dry gravel rallies' saturated braking falls 5 to 14 % from Finland's (Estonia, Portugal, Sardegna, Acropolis,
/// Safari). With this the loosest gravel is 10 % under the firm one, a user decision (2026-11-11): no more, to keep
/// the stages near their authors' design. Ours: that a deeper loose layer is the lower road.
pub fn gravel_depth_level(depth: f64) -> f64 {
    let t = ((depth - GRAVEL_FIRM_DEPTH) / (GRAVEL_LOOSE_DEPTH - GRAVEL_FIRM_DEPTH)).clamp(0.0, 1.0);
    1.0 + (GRAVEL_LOOSE_LEVEL - 1.0) * t
}

/// The sideways speed of the contact point above which NGP's soil lateral force stops growing on gravel, m/s
/// ([`gravel_soil_share`]): 27 deg of slip at 80 km/h, 14 at 150.
pub const GRAVEL_SOIL_SPEED: f64 = 10.0;

/// The share of NGP's soil lateral force (∝ v_lat²) gravel keeps at the contact point's sideways speed `v_lat` (m/s):
/// the whole to [`GRAVEL_SOIL_SPEED`], its value there above. NGP's force is 0.03 g of the i20 Coupe WRC at 6 m/s,
/// 0.08 g at 10, 0.16 g at 14 and 0.3 g at 19 (30 deg at 100 and 140 km/h), and the car's whole g rises with it:
/// held past the limit on the pad (material 49, circles of 65 / 100 / 200 m, the p90 of 2 s windows from 15 / 30 /
/// 40 deg of sideslip up) NGP's tyre reads 1.01 / 1.02 / 1.02 g at 60-100 km/h and 1.07 at 100-160. The 2017-21
/// World Rally Cars in Finland read 0.97 / 1.00 / 0.90 and 1.01 / 0.92 / 0.83 on their accelerometers (1.04 / 1.06 /
/// 1.05 and 1.06 / 1.04 / 0.98 along their path, the road cambered). Held from 10 m/s, with [`GRAVEL_LATERAL`] at
/// 1.06: 0.94 / 0.92 / 0.89 and 0.98 from 15 deg at 100-160, and the 200 m circle's limit 0.98 g at 159 km/h (NGP's
/// tyre 0.99, Finland's p90 0.99); at 1.10 0.96 / 0.92 / 0.90 and 1.00. From 6 m/s (the value on 2026-11-11, a user
/// decision on the real cars' whole g, then driven: "quite a bit less grippy in the corners") 0.91 / 0.90 / 0.87,
/// 0.94 and 0.93 g at 156 km/h; from 14 m/s 0.99 / 0.99 / 0.95 and 1.03, over the real cars'. Not measured: 30 deg
/// up above 100 km/h, where the held force is level and the real cars' whole g falls. Ours: the form and the value.
pub fn gravel_soil_share(v_lat: f64) -> f64 {
    let v = v_lat.abs();
    if v > GRAVEL_SOIL_SPEED {
        (GRAVEL_SOIL_SPEED / v).powi(2)
    } else {
        1.0
    }
}

/// Snow's level along the wheel over NGP's, on the snow share of the soil ([`class_share`]). On the material of RSF 531
/// Aittajarvi (117, SnowOnIce_Shallow) the i20 Coupe WRC 2020 brakes at a slip of 0.3 with 1.16 / 1.07 / 1.00 / 0.96 g
/// over 160-130 / 130-100 / 100-70 / 70-40 km/h at NGP's level, where the 2017-21 World Rally Cars of Arctic Rally
/// Finland 2021, the grippiest of the four snow rallies of 2017-21 (Sweden 2017-18 read 6-7 % under it, 2019 level with
/// it; 2020 ran on frozen gravel), give 0.92 / 0.79-0.82 / 0.73 / 0.67 (their tyres' force with the wheels 22 % or more
/// under the car, plus the drag of 1.74 m2; the accelerometers at 22-50 % of slip 0.80-0.82 g at 115 km/h). With this
/// 0.88 / 0.80 / 0.74 / 0.70: 5 % over them at most (user 2026-11-13: closer to the Arctic's stages, 5 % above at
/// most), 4 % under at the top, the car gaining less with speed than they do (0.75: 0.90 / 0.82 / 0.76 / 0.72; 0.78:
/// 0.93 / 0.85 / 0.79 / 0.75; `notes/tyre-model.md`, "Real stage telemetry (WRC, snow)").
pub const SNOW_LONGITUDINAL: f64 = 0.73;

/// Snow's level across the wheel over NGP's, on the snow share of the soil: 5 % at most over the whole g the same cars
/// hold in a slide in the Arctic (user 2026-11-13, as [`SNOW_LONGITUDINAL`]), 0.82 g from 15 deg of sideslip at 60-100
/// km/h and 0.84 at 100-160 on their accelerometers (0.85 / 0.88 along their path; Finland's gravel 0.97 / 1.01). The
/// i20 held past the limit on material 117's circles gives 1.08 / 1.14 at NGP's level and 0.74 / 0.78, 0.77 / 0.82,
/// 0.80 / 0.87, 0.80 / 0.88, 0.83 / 0.90, 0.87 / 0.95 at 0.70, 0.75, 0.80, 0.82, 0.85, 0.90 with [`SNOW_SOIL_SPEED`].
/// Corner by corner on RSF 531 against Aittajarvi's two passes the car at 0.80 / 0.85 / 0.90 holds 0.81-0.87 /
/// 0.84-0.90 / 0.88-0.94 of the crews' g in the corners that turn 30 deg or more, gravel's 0.92 by the same reading
/// (`notes/tyre-model.md`, "Real stage telemetry (WRC, snow)").
pub const SNOW_LATERAL: f64 = 0.82;

/// The sideways speed of the contact point above which NGP's soil lateral force stops growing on snow, m/s
/// ([`snow_soil_share`]), gravel's ([`GRAVEL_SOIL_SPEED`]): with the whole force the i20 at 0.75 of NGP's level on both
/// axes still held 0.91 / 0.96 g in a slide, and the real cars' whole g does not rise with the sideslip (the Arctic's
/// accelerometers 0.81 / 0.82 / 0.80 / 0.86 from 5 / 15 / 20 / 30 deg at 60-100 km/h). From 6 m/s, at 0.75 across the
/// wheel, 0.74.
pub const SNOW_SOIL_SPEED: f64 = 10.0;

/// The share of NGP's soil lateral force snow keeps at the contact point's sideways speed `v_lat` (m/s): the whole to
/// [`SNOW_SOIL_SPEED`], its value there above, as [`gravel_soil_share`].
pub fn snow_soil_share(v_lat: f64) -> f64 {
    let v = v_lat.abs();
    if v > SNOW_SOIL_SPEED {
        (SNOW_SOIL_SPEED / v).powi(2)
    } else {
        1.0
    }
}

/// The share of snow's level the snow held in the tread carries. Along the wheel snow builds its force in two stages,
/// the tread on the snow and the held snow shearing on the road's, softer: VTI's trailer (BV12) on hard packed snow at
/// 30 km/h, a Nordic winter tyre's braking fitted as two brushes (Baliga & Hiregoudar, Chalmers MSc 2018:16, Tables
/// 5.1-5.2: friction 0.3105 + 0.2103, stiffness over the load 9.28 + 1.46; a European one 0.3095 + 0.185, 12.61 +
/// 1.29). Measured there (Figs 6.2-6.3, fig ±0.01): 0.80 of the peak at 10 % slip, 0.95 at 20 %, the peak at 40-50 %;
/// the same trailer elsewhere 0.84 / 0.97 of the 30 % value (Albinsson et al., AVEC'16, Fig. 7) and peaks at 28-31 %
/// (Hjort et al., AVEC'24, Fig. 2b); a driven tyre at 8 km/h 0.81-0.86 of its maximum at 12 % (Shoop, CRREL 93-9,
/// Table 3). The one patch held 0.99 at 10 % and peaked at 13 % (4 kN, 15 m/s); the two 0.85 and 29 %. The i20 Coupe
/// WRC 2020 on RSF 531's material (117), its wheels 5 / 11 / 16 / 26 / 47 % under it near 115 km/h, brakes at 0.48 /
/// 0.67 / 0.73 / 0.80 / 0.80 g (the one patch 0.51 / 0.77 / 0.80 / 0.80 / 0.80), where the Arctic's crews of 2021
/// read 0.53 / 0.61 / 0.68 / 0.74 / 0.80 / 0.82 over 3-6 / 6-10 / 10-15 / 15-22 / 22-32 / 32-50 %. Sideways one stage:
/// the same thesis fits two there as well (8.20 + 0.97), and its measured curve (Figs 6.5-6.6: 90 % of the value at
/// 11° by 6°) is the one patch's at NGP's slope (90 % by 7.5°, the peak at 12°). Ours: the Nordic tyre's shares on a
/// studded rally tyre (road studs read as studless on snow there, Hjort), the same driving as braking.
pub const SNOW_HELD_LEVEL: f64 = 0.40;
/// The held snow's share of the longitudinal slip stiffness: the Nordic tyre's 1.46 of 10.74 ([`SNOW_HELD_LEVEL`]).
pub const SNOW_HELD_STIFFNESS: f64 = 0.14;

/// NGP's surface class of its gravel materials (`Gravel_*`, ids 1-51; snow is 3, grass 5, rock 4, rough 0).
pub const GRAVEL_CLASS: u32 = 1;
/// NGP's surface class of its snow materials (SnowOnGravel/SnowOnIce 101-103, 117-119, snowwalls).
pub const SNOW_CLASS: u32 = 3;

/// The share of surface class `class` in the wheel's blend of two materials of `classes`, the second weighted `blend`.
pub fn class_share(classes: [u32; 2], blend: f32, class: u32) -> f64 {
    let is = |c: u32| if c == class { 1.0 } else { 0.0 };
    let z = f64::from(blend).clamp(0.0, 1.0);
    is(classes[0]) * (1.0 - z) + is(classes[1]) * z
}

/// The secant slip stiffness, N per unit slip, of gravel's shear law (Janosi-Hanamoto's form): the force μ Fz (1 -
/// exp(-u)) along the theoretical slip `s`, u = |(sx/s0x, sy/s0)| with s0 = tan `alpha0_deg` ([`gravel_alpha0_deg`])
/// and s0x the longitudinal one that keeps the brush's [`KX_OVER_KY`] at the axes' levels `mu_x`, `mu_y`; `mu` the
/// friction along `s`, `stiffness` a factor on its stiffness (s0 over it). Both axes take it as their secant (-F_i/s_i).
#[allow(clippy::too_many_arguments)]
pub fn shear(fz: f64, mu: f64, mu_x: f64, mu_y: f64, sx: f64, sy: f64, alpha0_deg: f64, stiffness: f64) -> f64 {
    let s_abs = sx.hypot(sy);
    if s_abs <= 0.0 {
        return 0.0;
    }
    let s0 = libm::tan(alpha0_deg.to_radians()) / stiffness;
    let s0x = s0 * mu_x / (KX_OVER_KY * mu_y);
    mu * fz * -libm::expm1(-(sx / s0x).hypot(sy / s0)) / s_abs
}

/// The steady loose-surface tyre (see the module): the brush's patch at NGP's levels times the grip, plus the berm.
/// `gravel` is the share of the soil under the patch that takes gravel's shear law ([`shear`]) at the loose layer's
/// `depth` (m); the rest takes the brush's. `snow` is the share at snow's levels ([`SNOW_LONGITUDINAL`],
/// [`SNOW_LATERAL`]) whose berm is [`SNOW_PLOUGH`]; the rest's is [`PLOUGH`], on the gravel share times
/// [`gravel_berm`]. `mu` is the directional friction including the berm (the force's limit).
pub fn eval(x: &EvalInput, c: &Loose, gravel: f64, snow: f64, depth: f64) -> EvalOutput {
    let (vsx, vsy, vr) = brush::slip_velocities(x);
    let (sx, sy) = (vsx / vr, vsy / vr);
    let cal = &c.cal;
    let cy = brush::stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure_factor, cal.cy0, M_CY, cal);
    let cx = brush::stiffness_at(x.fz, x.bulk_temp, x.tread_remaining, x.pressure_factor, cal.cx0, M_CX, cal);
    let (ly, lx) = c.levels.at(x.fz);
    let share = gravel.clamp(0.0, 1.0);
    let grip = (x.surf_static / cal.surf_s_cal)
        * (x.temp_grip / cal.temp_grip_cal)
        * gravel_damp_level(x.wetness, gravel)
        * (1.0 + (gravel_depth_level(depth) - 1.0) * share);
    let snow = snow.clamp(0.0, 1.0);
    let (mu_y, mu_x) = (
        ly * grip * (1.0 + (GRAVEL_LATERAL - 1.0) * share + (SNOW_LATERAL - 1.0) * snow),
        lx * grip * (1.0 + (GRAVEL_LONGITUDINAL - 1.0) * share + (SNOW_LONGITUDINAL - 1.0) * snow),
    );
    let mut out = brush::patch(x.fz, x.half_length, cx, cy, mu_x, mu_y, sx, sy);
    if snow > 0.0 && share < 1.0 {
        // Snow's two stages along the wheel: two patches side by side, the tread's and the held snow's, each with its
        // share of the load, so of the level and of the cornering stiffness (the side force alone is the one patch's).
        // By snow's share of the soil gravel's shear law leaves, which takes its own share below.
        let part = |w: f64, k: f64| brush::patch(w * x.fz, x.half_length, k * cx, w * cy, mu_x, mu_y, sx, sy);
        let tread = part(1.0 - SNOW_HELD_LEVEL, 1.0 - SNOW_HELD_STIFFNESS);
        let held = part(SNOW_HELD_LEVEL, SNOW_HELD_STIFFNESS);
        let w = (snow / (1.0 - share)).min(1.0);
        out.kx += (tread.kx + held.kx - out.kx) * w;
        out.ky += (tread.ky + held.ky - out.ky) * w;
        out.mz += (tread.mz + held.mz - out.mz) * w;
        out.fx = -out.kx * sx;
        out.fy = -out.ky * sy;
        out.trail = if out.fy != 0.0 { -out.mz / out.fy } else { 0.0 };
    }
    if gravel > 0.0 {
        let k = shear(x.fz, out.mu, mu_x, mu_y, sx, sy, gravel_alpha0_deg(depth, x.wetness), c.shear_stiffness);
        out.kx += (k - out.kx) * gravel;
        out.ky += (k - out.ky) * gravel;
        out.fx = -out.kx * sx;
        out.fy = -out.ky * sy;
        out.trail = if out.fy != 0.0 { -out.mz / out.fy } else { 0.0 };
        if let Some(f) = test_gravel_spin() {
            let t = ((driving_slip(vsx, x.vx, vr) - SPIN_FROM) / (1.0 - SPIN_FROM)).clamp(0.0, 1.0);
            let k = 1.0 - (1.0 - f) * t * t * (3.0 - 2.0 * t) * gravel;
            if k < 1.0 {
                let before = out.fx.hypot(out.fy);
                out.kx *= k;
                out.fx = -out.kx * sx;
                if before > 0.0 {
                    out.mu *= out.fx.hypot(out.fy) / before;
                }
            }
        }
    }
    // The berm: from BERM_FROM, or the braking slip of the longitudinal full slide (|sx| = 3 mu_x Fz / Cx) if later, to
    // lock.
    let s_abs = sx.hypot(sy);
    let full = 3.0 * mu_x * x.fz / cx;
    let start = (full / (1.0 + full)).max(BERM_FROM);
    let b = braking_slip(vsx, x.vx);
    let t = if start < 1.0 { ((b - start) / (1.0 - start)).clamp(0.0, 1.0) } else { 0.0 };
    let plough = PLOUGH + (SNOW_PLOUGH - PLOUGH) * snow;
    let berm = (plough - 1.0) * (1.0 + (gravel_berm(depth) - 1.0) * share) * t * t * (3.0 - 2.0 * t);
    if berm > 0.0 && s_abs > 0.0 {
        let per_s = berm * out.mu * x.fz / s_abs;
        out.kx += per_s;
        out.ky += per_s;
        out.fx = -out.kx * sx;
        out.fy = -out.ky * sy;
        out.mu *= 1.0 + berm;
        out.trail = if out.fy != 0.0 { -out.mz / out.fy } else { 0.0 };
    }
    out
}

/// The input at the calibration point: load `cal.fz0`, speed `v`, slip angle `alpha` (rad), free rolling, unit radius,
/// the stiffness factors at 1 and a unit surface.
fn cal_input(cal: &Calibration, v: f64, alpha: f64) -> EvalInput {
    EvalInput {
        fz: cal.fz0,
        half_length: 0.07,
        vx: -v * libm::cos(alpha),
        vy: v * libm::sin(alpha),
        spin: v * libm::cos(alpha),
        radius: 1.0,
        bulk_temp: cal.t_cal,
        tread_remaining: cal.d_cal,
        pressure: 0.0,
        pressure_factor: cal.pf_cal,
        wetness: cal.w_cal,
        hydroplaning: false,
        temp_grip: cal.temp_grip_cal,
        surf_static: cal.surf_s_cal,
        surf_kinetic: cal.surf_k_cal,
    }
}

/// The least-squares cornering slope over 0 to 2° (in 0.25°) at the calibration point, as `crate::fit` takes NGP's.
pub fn cornering_slope(c: &Loose, v: f64) -> f64 {
    let (sxy, sxx) = (1..=8).map(|k| (f64::from(k) * 0.25).to_radians()).fold((0.0, 0.0), |(sxy, sxx), a| {
        (sxy + a * eval(&cal_input(&c.cal, v, a), c, 0.0, 0.0, 0.0).fy.abs(), sxx + a * a)
    });
    sxy / sxx
}

/// The loose tyre on the brush's calibration `brush` with NGP's `levels`: the cornering stiffness solved so the slope
/// at speed `v` is `slope` (NGP's, as the brush's), the longitudinal stiffness [`KX_OVER_KY`] times it, as the brush's.
pub fn calibrate(brush: &Calibration, levels: Levels, slope: f64, v: f64) -> Loose {
    let mut c = Loose { cal: *brush, levels, shear_stiffness: 1.0 };
    // The slope is close to proportional to the stiffness: a fixed point converges in a few steps.
    for _ in 0..100 {
        let r = slope / cornering_slope(&c, v);
        c.cal.cy0 *= r;
        c.cal.cx0 = KX_OVER_KY * c.cal.cy0;
        if (r - 1.0).abs() < 1e-12 {
            break;
        }
    }
    c
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A loose tyre with round numbers (not NGP's): peaks 0.9 / 0.85 at 2 kN, 0.85 / 0.84 at 4 kN, 0.8 / 0.83 at 6 kN,
    /// cornering stiffness 9 per rad of load at 4 kN.
    pub(crate) fn loose() -> Loose {
        let cal = Calibration {
            fz0: 4000.0,
            cy0: 36000.0,
            cx0: KX_OVER_KY * 36000.0,
            mu_ys: 1.0,
            mu_yk: 0.5,
            mu_xs: 1.0,
            mu_xk: 0.5,
            t_cal: 350.0,
            d_cal: 1.0,
            pf_cal: 1.0,
            w_cal: 0.0,
            surf_s_cal: 1.0,
            surf_k_cal: 1.0,
            temp_grip_cal: 1.0,
        };
        let levels = Levels {
            load: vec![2000.0, 4000.0, 6000.0],
            lateral: vec![0.9, 0.85, 0.8],
            longitudinal: vec![0.85, 0.84, 0.83],
        };
        Loose { cal, levels, shear_stiffness: 1.0 }
    }

    /// At load `fz`, speed `v`, slip angle `alpha` (deg) and slip ratio `kappa` (-1 locked), on a surface of grip
    /// `grip`, under the brush's law.
    fn at(c: &Loose, fz: f64, v: f64, alpha: f64, kappa: f64, grip: f64) -> EvalOutput {
        on(c, fz, v, alpha, kappa, grip, 0.0)
    }

    /// The loose layer's depth of NGP's deeper gravel, m: pea stone's α0 ([`GRAVEL_ALPHA0_DEG`]).
    const DEEP: f64 = 0.025;

    /// As [`at`], with a share `gravel` of gravel's shear law on deeper gravel.
    fn on(c: &Loose, fz: f64, v: f64, alpha: f64, kappa: f64, grip: f64, gravel: f64) -> EvalOutput {
        let a = alpha.to_radians();
        let vx = -v * a.cos();
        let x = EvalInput {
            fz,
            vx,
            vy: v * a.sin(),
            spin: (1.0 + kappa) * -vx / 0.3,
            radius: 0.3,
            surf_static: grip,
            surf_kinetic: grip,
            ..cal_input(&c.cal, v, a)
        };
        eval(&x, c, gravel, 0.0, DEEP)
    }

    #[test]
    fn the_free_rolling_peaks_are_the_levels_times_the_grip_at_each_load() {
        let c = loose();
        for (fz, grip) in [(2000.0, 0.9), (3000.0, 0.9), (4000.0, 0.9), (6000.0, 0.7), (7000.0, 1.0)] {
            let (ly, lx) = c.levels.at(fz);
            let lat = (0..=400).map(|k| at(&c, fz, 15.0, f64::from(k) * 0.1, 0.0, grip).fy.abs()).fold(0.0, f64::max);
            let lon = (0..=100).map(|k| at(&c, fz, 15.0, 0.0, f64::from(k) * 0.01, grip).fx.abs()).fold(0.0, f64::max);
            assert!((lat / (ly * grip * fz) - 1.0).abs() < 1e-6, "{fz} {lat}");
            assert!((lon / (lx * grip * fz) - 1.0).abs() < 1e-6, "{fz} {lon}");
        }
        assert_eq!(c.levels.at(3000.0), (0.875, 0.845));
    }

    #[test]
    fn the_side_force_saturates_without_falling() {
        let c = loose();
        let fy = |a: f64| at(&c, 4000.0, 15.0, a, 0.0, 1.0).fy.abs();
        let peak = 0.85 * 4000.0;
        let mut last = 0.0;
        for k in 0..=400 {
            let f = fy(f64::from(k) * 0.1);
            assert!(f >= last - 1e-9, "{k}");
            last = f;
        }
        assert!((fy(40.0) / peak - 1.0).abs() < 1e-9);
        // Normalised by the peak at 2.5/5/10/15°, at a stiffness of 9 per rad (NGP's gravel tyre's); Coutermarsh's
        // softer road tyre (3-5 per rad) has pea stone 0.56/0.81 of its peak at 5/10°.
        let shape: Vec<f64> = [2.5, 5.0, 10.0, 15.0].iter().map(|&a| fy(a) / peak).collect();
        assert!(shape[0] > 0.35 && shape[0] < 0.5 && shape[2] > 0.85 && shape[3] > 0.95, "{shape:?}");
    }

    #[test]
    fn gravel_rises_by_the_shear_law_to_the_level() {
        let c = loose();
        let peak = 0.85 * GRAVEL_LATERAL * gravel_depth_level(DEEP) * 4000.0;
        let fy = |a: f64| on(&c, 4000.0, 15.0, a, 0.0, 1.0, 1.0).fy.abs() / peak;
        // Coutermarsh & Shoop's pea stone over its maximum at 2.5/5/10/15° (Fig. 15): 0.33/0.55/0.80/0.91.
        for (a, pea) in [(2.5, 0.33), (5.0, 0.55), (10.0, 0.80), (15.0, 0.91)] {
            let law = -libm::expm1(-libm::tan(f64::to_radians(a)) / libm::tan(GRAVEL_ALPHA0_DEG.to_radians()));
            assert!((fy(a) - law).abs() < 1e-9 && (fy(a) - pea).abs() < 0.01, "{a}: {}", fy(a));
        }
        let mut last = 0.0;
        for k in 0..=400 {
            let f = fy(f64::from(k) * 0.1);
            assert!(f > last || k == 0, "{k}");
            last = f;
        }
        assert!(last > 0.999 && last < 1.0, "{last}");
        // The longitudinal side takes the same law at its own level; half a share is between the laws.
        let fx = |k: f64, g: f64| on(&c, 4000.0, 15.0, 0.0, k, 1.0, g).fx.abs();
        let level_x = 0.84 * GRAVEL_LONGITUDINAL;
        let s0x = libm::tan(GRAVEL_ALPHA0_DEG.to_radians()) * level_x / (KX_OVER_KY * 0.85 * GRAVEL_LATERAL);
        let law = -libm::expm1(-(0.5 / 1.5) / s0x) * level_x * gravel_depth_level(DEEP) * 4000.0;
        assert!((fx(0.5, 1.0) - law).abs() < 1e-5, "{} {law}", fx(0.5, 1.0));
        assert!(fx(0.05, 0.5) > fx(0.05, 1.0) && fx(0.05, 0.5) < fx(0.05, 0.0));
        assert_eq!(class_share([GRAVEL_CLASS, SNOW_CLASS], 0.25, GRAVEL_CLASS), 0.75);
        assert_eq!(class_share([SNOW_CLASS, SNOW_CLASS], 0.5, GRAVEL_CLASS), 0.0);
        assert_eq!(class_share([GRAVEL_CLASS, SNOW_CLASS], 0.25, SNOW_CLASS), 0.25);
    }

    #[test]
    fn wet_gravel_rises_slower_by_the_water_factor() {
        let c = loose();
        let peak = 0.85 * GRAVEL_LATERAL * gravel_depth_level(DEEP) * 4000.0;
        let fy = |a: f64, w: f64| {
            let r = a.to_radians();
            let x = EvalInput {
                vy: 15.0 * r.sin(),
                vx: -15.0 * r.cos(),
                spin: 15.0 * r.cos() / 0.3,
                radius: 0.3,
                wetness: w,
                ..cal_input(&c.cal, 15.0, r)
            };
            eval(&x, &c, 1.0, 0.0, DEEP).fy.abs() / peak
        };
        // Coutermarsh & Shoop's wet gravel over its maximum at 2.5/5/10/15° (Fig. 15): 0.23/0.40/0.65/0.79.
        for (a, wet) in [(2.5, 0.23), (5.0, 0.40), (10.0, 0.65), (15.0, 0.79)] {
            assert!((fy(a, 1.0) - wet).abs() < 0.01, "{a}: {}", fy(a, 1.0));
        }
        assert_eq!(fy(10.0, 0.0), on(&c, 4000.0, 15.0, 10.0, 0.0, 1.0, 1.0).fy.abs() / peak);
        let shape = |a: f64, w: f64| fy(a, w) / gravel_damp_level(w, 1.0);
        assert!(shape(10.0, 0.25) < shape(10.0, 0.0) && shape(10.0, 0.25) > shape(10.0, 1.0));
        assert!((gravel_alpha0_deg(DEEP, 0.25) - (6.3 + 3.4 * 0.25)).abs() < 1e-12);
        assert!((gravel_alpha0_deg(0.03, 2.0) - GRAVEL_WET_ALPHA0_DEG).abs() < 1e-12);
    }

    #[test]
    fn loose_gravel_is_lower_and_gravel_brakes_under_its_side_level() {
        assert_eq!(gravel_depth_level(0.009), 1.0);
        assert_eq!(gravel_depth_level(GRAVEL_FIRM_DEPTH), 1.0);
        assert!((gravel_depth_level(0.0225) - (1.0 + GRAVEL_LOOSE_LEVEL) / 2.0).abs() < 1e-12);
        assert_eq!(gravel_depth_level(0.04), GRAVEL_LOOSE_LEVEL);
        // The levels reached at a large slip angle and at a large braking slip short of the berm, firm and loose, over
        // the other soils' (no gravel share: NGP's).
        let c = loose();
        let side = |depth: f64, g: f64| {
            let x = EvalInput { vx: 0.0, vy: 15.0, spin: 0.0, radius: 0.3, ..cal_input(&c.cal, 15.0, 0.0) };
            eval(&x, &c, g, 0.0, depth).fy.abs() / 4000.0
        };
        let brake = |depth: f64, g: f64| {
            let x =
                EvalInput { vx: -15.0, vy: 0.0, spin: 0.25 * 15.0 / 0.3, radius: 0.3, ..cal_input(&c.cal, 15.0, 0.0) };
            eval(&x, &c, g, 0.0, depth).fx.abs() / 4000.0
        };
        assert!((side(0.015, 1.0) / side(0.015, 0.0) - GRAVEL_LATERAL).abs() < 1e-3);
        assert!((side(0.03, 1.0) / side(0.03, 0.0) - GRAVEL_LATERAL * GRAVEL_LOOSE_LEVEL).abs() < 1e-3);
        assert!((brake(0.009, 1.0) / brake(0.009, 0.0) - GRAVEL_LONGITUDINAL).abs() < 0.01);
        assert!((brake(0.03, 1.0) / brake(0.009, 1.0) - GRAVEL_LOOSE_LEVEL).abs() < 0.03);
    }

    #[test]
    fn damp_gravel_grips_as_dry() {
        // The factor: 1 dry and wet, 1/0.925 at damp, linear in W between, only the gravel share's.
        assert_eq!(gravel_damp_level(0.0, 1.0), 1.0);
        assert_eq!(gravel_damp_level(1.0, 1.0), 1.0);
        assert_eq!(gravel_damp_level(0.25, 0.0), 1.0);
        assert!((gravel_damp_level(0.25, 1.0) - 1.0 / 0.925).abs() < 1e-15);
        assert!((gravel_damp_level(0.125, 1.0) - (1.0 + (1.0 / 0.925 - 1.0) / 2.0)).abs() < 1e-15);
        assert!((gravel_damp_level(0.625, 1.0) - (1.0 + (1.0 / 0.925 - 1.0) / 2.0)).abs() < 1e-15);
        assert!((gravel_damp_level(0.25, 0.5) - (1.0 + (1.0 / 0.925 - 1.0) / 2.0)).abs() < 1e-15);
        // On NGP's damp friction (× 0.925), the locked gravel wheel's force is the dry one's; snow keeps NGP's damp.
        let c = loose();
        let locked = |w: f64, mu: f64, gravel: f64| {
            let x =
                EvalInput { spin: 0.0, wetness: w, surf_static: mu, surf_kinetic: mu, ..cal_input(&c.cal, 15.0, 0.0) };
            eval(&x, &c, gravel, 1.0 - gravel, DEEP).fx
        };
        assert!((locked(0.25, 0.925, 1.0) / locked(0.0, 1.0, 1.0) - 1.0).abs() < 1e-12);
        assert_eq!(locked(0.25, 0.925, 0.0), locked(0.0, 0.925, 0.0));
        assert_eq!(locked(1.0, 0.85, 1.0), locked(0.0, 0.85, 1.0));
    }

    #[test]
    fn a_shallow_layer_builds_force_faster() {
        // The law alone, α90 = atan(ln 10 tan α0): 7.8° at 9 mm (8° in the patch with its carcass), 11° at 17.5 mm,
        // 14.25° from 25 mm, dry.
        let alpha90 = |depth: f64| (std::f64::consts::LN_10 * gravel_alpha0_deg(depth, 0.0).to_radians().tan()).atan();
        for (depth, target) in [(0.0, 7.8), (0.009, 7.8), (0.0175, 11.0), (0.025, 14.25), (0.03, 14.25)] {
            assert!((alpha90(depth).to_degrees() - target).abs() < 0.1, "{depth}: {}", alpha90(depth).to_degrees());
        }
        // Linear between the anchors: Coarse Shallow (15 mm) and Coarse Deep (22.5 mm).
        assert!((gravel_alpha0_deg(0.015, 0.0) - (3.4 + 1.4 * 6.0 / 8.5)).abs() < 1e-12);
        assert!((gravel_alpha0_deg(0.0225, 0.0) - (4.8 + 1.5 * 5.0 / 7.5)).abs() < 1e-12);
        // The wet ratio on top of each depth's dry α0.
        for depth in [0.009, 0.0175, 0.025] {
            let r = gravel_alpha0_deg(depth, 1.0) / gravel_alpha0_deg(depth, 0.0);
            assert!((r - GRAVEL_WET_ALPHA0_DEG / GRAVEL_ALPHA0_DEG).abs() < 1e-12);
        }
        // In the patch: the shallow layer's force at 5° is above the deep one's, both reach the level.
        let c = loose();
        let x = |a: f64| {
            let r = f64::to_radians(a);
            EvalInput {
                vy: 15.0 * r.sin(),
                vx: -15.0 * r.cos(),
                spin: 15.0 * r.cos() / 0.3,
                radius: 0.3,
                ..cal_input(&c.cal, 15.0, r)
            }
        };
        let fy = |a: f64, depth: f64| {
            eval(&x(a), &c, 1.0, 0.0, depth).fy.abs() / (0.85 * GRAVEL_LATERAL * gravel_depth_level(depth) * 4000.0)
        };
        assert!(fy(5.0, 0.009) > fy(5.0, 0.0175) && fy(5.0, 0.0175) > fy(5.0, DEEP));
        assert!(fy(40.0, 0.009) > 0.999 && fy(40.0, DEEP) > 0.999);
    }

    #[test]
    fn the_braking_force_rises_past_the_peak_to_the_berm_at_lock() {
        let c = loose();
        for v in [5.0, 15.0, 25.0] {
            let fx = |kappa: f64| at(&c, 4000.0, v, 0.0, kappa, 0.9).fx;
            let peak = 0.84 * 0.9 * 4000.0;
            let mut last = 0.0;
            for k in 0..=1000 {
                let f = fx(-f64::from(k) * 0.001);
                assert!(f >= last - 1e-9, "{v} {k}: {f} after {last}");
                // Continuous: no step larger than the slope allows.
                assert!(f - last < 0.025 * peak, "{v} {k}");
                last = f;
            }
            assert!((fx(-1.0) / peak - PLOUGH).abs() < 1e-9, "{v}");
            // Flat at the peak from the full slide to BERM_FROM.
            assert!((fx(-0.5) / peak - 1.0).abs() < 1e-9 && (fx(-BERM_FROM) / peak - 1.0).abs() < 1e-9, "{v}");
            // Driving: flat at the peak, no berm.
            assert!((fx(1.0) / -peak - 1.0).abs() < 1e-9, "{v}");
            assert!((fx(0.3) / -peak - 1.0).abs() < 1e-9, "{v}");
        }
    }

    #[test]
    fn snow_takes_its_own_berm_by_its_share() {
        let c = loose();
        let locked = |snow: f64| {
            let x = EvalInput { spin: 0.0, surf_static: 0.9, surf_kinetic: 0.9, ..cal_input(&c.cal, 15.0, 0.0) };
            eval(&x, &c, 0.0, snow, 0.0).fx / (0.84 * 0.9 * 4000.0)
        };
        assert!((locked(1.0) - SNOW_PLOUGH * SNOW_LONGITUDINAL).abs() < 1e-9);
        assert!((locked(0.5) - (PLOUGH + SNOW_PLOUGH) / 2.0 * (1.0 + SNOW_LONGITUDINAL) / 2.0).abs() < 1e-9);
        assert!((locked(0.0) - PLOUGH).abs() < 1e-9);
    }

    #[test]
    fn snow_builds_its_force_along_the_wheel_in_two_stages_and_sideways_in_one() {
        let c = loose();
        let at = |kappa: f64, alpha: f64, snow: f64, surf: f64| {
            let free = cal_input(&c.cal, 15.0, alpha);
            let x = EvalInput { spin: (1.0 - kappa) * free.spin, surf_static: surf, ..free };
            eval(&x, &c, 0.0, snow, 0.0)
        };
        // The slip stiffness at no slip is the one patch's; the force at 10 % of braking slip is not.
        assert!((at(1e-6, 0.0, 1.0, 1.0).kx / at(1e-6, 0.0, 0.0, 1.0).kx - 1.0).abs() < 1e-4);
        let rise = |snow: f64| at(0.1, 0.0, snow, 1.0).fx / at(0.4, 0.0, snow, 1.0).fx;
        assert!((0.80..0.88).contains(&rise(1.0)) && rise(0.0) > 0.9, "{} {}", rise(1.0), rise(0.0));
        // Sideways alone it is the one patch at snow's level.
        let alpha = 4.0f64.to_radians();
        let (two, one) = (at(0.0, alpha, 1.0, 1.0), at(0.0, alpha, 0.0, SNOW_LATERAL));
        assert!((two.fy / one.fy - 1.0).abs() < 1e-12 && (two.mz / one.mz - 1.0).abs() < 1e-12);
    }

    #[test]
    fn snow_takes_its_own_level_on_each_axis_by_its_share() {
        // The levels reached sliding sideways and at a braking slip short of the berm, over the other soils' (NGP's).
        let c = loose();
        let side = |snow: f64| {
            let x = EvalInput { vx: 0.0, vy: 15.0, spin: 0.0, radius: 0.3, ..cal_input(&c.cal, 15.0, 0.0) };
            eval(&x, &c, 0.0, snow, 0.013).fy.abs()
        };
        let brake = |snow: f64| {
            let x =
                EvalInput { vx: -15.0, vy: 0.0, spin: 0.5 * 15.0 / 0.3, radius: 0.3, ..cal_input(&c.cal, 15.0, 0.0) };
            eval(&x, &c, 0.0, snow, 0.013).fx.abs()
        };
        assert!((side(1.0) / side(0.0) - SNOW_LATERAL).abs() < 1e-9);
        assert!((side(0.5) / side(0.0) - (1.0 + SNOW_LATERAL) / 2.0).abs() < 1e-9);
        assert!((brake(1.0) / brake(0.0) - SNOW_LONGITUDINAL).abs() < 1e-9);
    }

    #[test]
    fn the_soil_force_stops_growing_at_gravel_s_and_snow_s_sideways_speed() {
        assert_eq!((gravel_soil_share(0.0), gravel_soil_share(-GRAVEL_SOIL_SPEED)), (1.0, 1.0));
        assert_eq!((snow_soil_share(0.0), snow_soil_share(-SNOW_SOIL_SPEED)), (1.0, 1.0));
        // v² times the share is level above it.
        for v in [12.0, -14.0, 25.0] {
            assert!((v * v * gravel_soil_share(v) - GRAVEL_SOIL_SPEED.powi(2)).abs() < 1e-9, "{v}");
            assert!((v * v * snow_soil_share(v) - SNOW_SOIL_SPEED.powi(2)).abs() < 1e-9, "{v}");
        }
    }

    #[test]
    fn gravel_builds_the_berm_by_its_depth() {
        let c = loose();
        let locked = |gravel: f64, depth: f64| {
            let x = EvalInput { spin: 0.0, surf_static: 1.0, surf_kinetic: 1.0, ..cal_input(&c.cal, 15.0, 0.0) };
            let level = GRAVEL_LONGITUDINAL * gravel_depth_level(depth);
            eval(&x, &c, gravel, 0.0, depth).fx / (0.84 * 4000.0 * (1.0 + (level - 1.0) * gravel))
        };
        assert!((locked(1.0, 0.015) - 1.09).abs() < 1e-9);
        assert!((locked(1.0, BERM_DEPTH) - PLOUGH).abs() < 1e-9 && (locked(1.0, 0.08) - PLOUGH).abs() < 1e-9);
        assert!((locked(0.0, 0.015) - PLOUGH).abs() < 1e-9);
        assert!((locked(0.5, 0.015) - (1.09 + PLOUGH) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_locked_tyre_side_force_follows_the_sliding_velocity_below_free_rolling() {
        let c = loose();
        let peak = 0.85 * 4000.0;
        for (a, gravel) in [5.0, 10.0, 20.0, 35.0].into_iter().flat_map(|a| [(a, 0.0), (a, 1.0)]) {
            let locked = on(&c, 4000.0, 15.0, a, -1.0, 1.0, gravel);
            let free = on(&c, 4000.0, 15.0, a, 0.0, 1.0, gravel);
            assert!(locked.fy.abs() < free.fy.abs(), "{a}");
            // PLOUGH mu sin(alpha), mu along the sliding velocity between the axes' peaks.
            let share = locked.fy.abs() / locked.fx.hypot(locked.fy);
            assert!((share - a.to_radians().sin()).abs() < 1e-9, "{a}");
            assert!((locked.fx.hypot(locked.fy) / (locked.mu * 4000.0) - 1.0).abs() < 1e-9, "{a}");
        }
        let ratio = at(&c, 4000.0, 15.0, 35.0, -1.0, 1.0).fy.abs() / peak;
        assert!((0.7..0.78).contains(&ratio), "{ratio}");
    }

    #[test]
    fn it_fades_to_nothing_at_standstill() {
        let c = loose();
        // Locked, the berm fades out over the last V_MIN of speed; stopped, nothing.
        let locked = |v: f64| at(&c, 4000.0, v, 0.0, -1.0, 1.0).fx / (0.84 * 4000.0);
        assert!((locked(0.5) - PLOUGH).abs() < 1e-9);
        assert!(locked(0.45) < PLOUGH && locked(0.45) > 1.0);
        assert_eq!(locked(0.25), 1.0);
        assert!(locked(0.01) < 0.3 && locked(0.01) > 0.0);
        let stopped = at(&c, 4000.0, 0.0, 0.0, 0.0, 1.0);
        assert_eq!((stopped.fx, stopped.fy), (0.0, 0.0));
        assert_eq!(braking_slip(0.0, 0.0), 0.0);
        assert_eq!(braking_slip(-0.2, -0.1), 0.4);
        assert_eq!(braking_slip(0.2, -10.0), 0.0);
        assert_eq!(driving_slip(0.2, -10.0, 10.2), 0.2 / 10.2);
        assert_eq!(driving_slip(1.0, 0.0, 1.0), 1.0);
        assert_eq!(driving_slip(-0.2, -0.1, 0.5), 0.0);
    }

    #[test]
    fn the_calibration_takes_the_slope() {
        let c = loose();
        let solved = calibrate(&c.cal, c.levels.clone(), 30000.0, 15.0);
        assert!((cornering_slope(&solved, 15.0) / 30000.0 - 1.0).abs() < 1e-9);
        assert_eq!(solved.cal.cx0, KX_OVER_KY * solved.cal.cy0);
        assert!(solved.cal.cy0 > 30000.0, "the friction curves the slope below the stiffness");
    }
}
