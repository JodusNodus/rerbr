//! Probe-dump parser: one wheel's frame (setup tables and operating point) per
//! probe step, from `rbr-standalone --tyre-probe` dumps (line format, see
//! `rbr-standalone/src/tyre_probe.rs`).
//!
//! Research tooling (ours, experimental). Nothing here changes what the game
//! computes.

use std::collections::BTreeMap;

/// One wheel's frame at one probe step: the setup tables and the operating point.
#[derive(Clone, Debug, Default)]
pub struct Params {
    pub step: u32,
    pub wheel: u32,
    pub reference_friction: f32,
    pub carcass_lat: f32,
    pub carcass_tor: f32,
    pub temp_axis: Vec<f32>,
    pub rub_my: Vec<f32>,
    pub load_axis: Vec<f32>,
    pub crn_stf: Vec<f32>,
    pub trc_stf: Vec<f32>,
    pub crn_my: Vec<f32>,
    pub trc_my: Vec<f32>,
    pub crn_pk: Vec<f32>,
    pub trc_pk: Vec<f32>,
    pub sfric: Vec<f32>,
    pub wet_slope: Vec<f32>,
    pub wear_ftr: Vec<f32>,
    pub design_wetness: f32,
    pub wet_wear_factor: f32,
    pub static_friction: f32,
    pub kinetic_friction: f32,
    pub temperature_grip: f32,
    pub normal_force: f32,
    pub patch_length: f32,
    pub effective_radius: f32,
    pub stiffness_factor: f32,
    pub air_pressure: f32,
    pub press_axis: Vec<f32>,
    pub stf_val: Vec<f32>,
    pub tread_temp: f32,
    pub water_factor: f32,
    pub wear_op: f32,
    /// Contact speed from the dump's `slip_op` line, m/s.
    pub speed: f32,
}

/// Linear interpolation of `values` on `axis` at `x` (clamped ends).
pub fn interp(axis: &[f32], values: &[f32], x: f32) -> f32 {
    let n = axis.len().min(values.len());
    if n == 0 {
        return f32::NAN;
    }
    if x <= axis[0] {
        return values[0];
    }
    if x >= axis[n - 1] {
        return values[n - 1];
    }
    for k in 0..n - 1 {
        if x <= axis[k + 1] {
            let f = (x - axis[k]) / (axis[k + 1] - axis[k]);
            return (1.0 - f) * values[k] + values[k + 1] * f;
        }
    }
    values[n - 1]
}

fn parse_floats(rest: &str) -> Vec<f32> {
    rest.split_whitespace().filter_map(|w| w.parse().ok()).collect()
}

/// Parse one probe file into `(step, wheel) -> (Params, patch cloud entry)`.
/// Returns the dumps in file order plus every wheel-0 `(Fz, patch)` pair with
/// clean soil, for the patch-length interpolation.
pub fn parse_probe(text: &str) -> (Vec<Params>, Vec<(f32, f32)>) {
    let mut dumps: BTreeMap<(u32, u32), BTreeMap<String, String>> = BTreeMap::new();
    let mut step = 0u32;
    let mut wheel = 0u32;
    let mut order: Vec<(u32, u32)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("# step") {
            step = line.split_whitespace().nth(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("wheel:") {
            wheel = rest.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
            dumps.entry((step, wheel)).or_default();
            order.push((step, wheel));
        } else if let Some((k, v)) = line.split_once(':') {
            if let Some(m) = dumps.get_mut(&(step, wheel)) {
                m.insert(k.trim().to_owned(), v.trim().to_owned());
            }
        }
    }
    let get = |m: &BTreeMap<String, String>, k: &str| m.get(k).cloned().unwrap_or_default();
    let table = |m: &BTreeMap<String, String>, k: &str| -> Vec<f32> {
        let v = parse_floats(&get(m, k));
        // Probe table lines start with the entry count.
        if !v.is_empty() && v[0] as usize == v.len() - 1 {
            v[1..].to_vec()
        } else {
            v
        }
    };
    let mut out = Vec::new();
    let mut cloud = Vec::new();
    for ((s, w), m) in &dumps {
        let surf = parse_floats(&get(m, "surface"));
        let op = parse_floats(&get(m, "op"));
        let temps = parse_floats(&get(m, "temps"));
        let misc = parse_floats(&get(m, "misc_setup"));
        let carc = parse_floats(&get(m, "carcass"));
        let cls = parse_floats(&get(m, "tyre_classes"));
        let mut sfric = vec![];
        let mut wslope = vec![];
        let mut wearf = vec![];
        for (i, c) in cls.chunks(3).enumerate() {
            if c.len() == 3 {
                sfric.push(c[0]);
                wslope.push(c[1]);
                wearf.push(c[2]);
            } else {
                let _ = i;
            }
        }
        let wear_line = parse_floats(&get(m, "wear"));
        let slip_op = parse_floats(&get(m, "slip_op"));
        let p = Params {
            step: *s,
            wheel: *w,
            reference_friction: get(m, "reference_friction").parse().unwrap_or(1.0),
            carcass_lat: carc.first().copied().unwrap_or(f32::NAN),
            carcass_tor: carc.get(1).copied().unwrap_or(f32::NAN),
            temp_axis: table(m, "temperature_axis"),
            rub_my: table(m, "rubber_friction"),
            load_axis: table(m, "load_axis"),
            crn_stf: table(m, "cornering_stiffness"),
            trc_stf: table(m, "traction_stiffness"),
            crn_my: table(m, "cornering_friction"),
            trc_my: table(m, "traction_friction"),
            crn_pk: table(m, "cornering_peak_slip"),
            trc_pk: table(m, "traction_peak_slip"),
            sfric,
            wet_slope: wslope,
            wear_ftr: wearf,
            design_wetness: get(m, "design_wetness").parse().unwrap_or(0.0),
            wet_wear_factor: misc.get(2).copied().unwrap_or(1.0),
            static_friction: surf.first().copied().unwrap_or(f32::NAN),
            kinetic_friction: surf.get(1).copied().unwrap_or(f32::NAN),
            temperature_grip: surf.get(8).copied().unwrap_or(1.0),
            normal_force: op.first().copied().unwrap_or(f32::NAN),
            patch_length: op.get(1).copied().unwrap_or(f32::NAN),
            effective_radius: op.get(2).copied().unwrap_or(0.315),
            stiffness_factor: op.get(4).copied().unwrap_or(1.0),
            air_pressure: op.get(3).copied().unwrap_or(200000.0),
            press_axis: table(m, "pressure_axis"),
            stf_val: table(m, "stiffness_factor"),
            tread_temp: temps.first().copied().unwrap_or(337.0),
            water_factor: surf.get(6).copied().unwrap_or(0.0),
            wear_op: wear_line.first().copied().unwrap_or(0.0),
            speed: slip_op.first().copied().unwrap_or(0.0),
        };
        // Patch cloud: wheel 0, clean soil (thickness field index 5 of surface line).
        if *w == 0 && surf.get(5).copied().unwrap_or(1.0) == 0.0 && p.normal_force > 0.0 {
            cloud.push((p.normal_force, p.patch_length));
        }
        out.push(p);
    }
    let _ = order;
    cloud.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    (out, cloud)
}

/// Patch length at `fz`: linear interpolation of the probe's clean-soil cloud,
/// else the probed operating patch length (gravel never runs clean soil).
pub fn patch_at(cloud: &[(f32, f32)], fz: f32, fallback: f32) -> f32 {
    if cloud.is_empty() {
        return fallback;
    }
    let xs: Vec<f32> = cloud.iter().map(|c| c.0).collect();
    let ys: Vec<f32> = cloud.iter().map(|c| c.1).collect();
    interp(&xs, &ys, fz)
}

/// The game's tyre model's view of the probed wheel (`rbr_tyre::Wheel`): its tables.
pub fn wheel(p: &Params) -> rbr_tyre::Wheel {
    rbr_tyre::Wheel {
        tyre: 0,
        reference_friction: p.reference_friction,
        load_axis: p.load_axis.clone(),
        cornering_friction: p.crn_my.clone(),
        traction_friction: p.trc_my.clone(),
        cornering_peak_slip: p.crn_pk.clone(),
        traction_peak_slip: p.trc_pk.clone(),
        cornering_stiffness: p.crn_stf.clone(),
        traction_stiffness: p.trc_stf.clone(),
        temperature_axis: p.temp_axis.clone(),
        rubber_friction: p.rub_my.clone(),
    }
}
