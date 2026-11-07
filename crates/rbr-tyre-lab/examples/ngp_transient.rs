//! NGP's tyre *dynamics*: run NGP's real per-wheel tyre step
//! (`rbr_physics::tyre::step::step`, via `layout::entries::step` on a `Mem`, exactly as the game calls it)
//! in a loop under scripted kinematics, starting from a frame captured from a real run.
//!
//! Research tooling: the steady-state sweep (`sweep.rs`) cannot show transient lag, the sticking/sliding
//! blend at low speed, or heat build-up; this harness can. It changes nothing the game computes.
//!
//! Capture (read-only): `rbr-standalone --autopilot OUT --boot ... --tyre-image FILE [--tyre-image-step N]`
//! writes `FILE` (records `base size bytes`, u32 LE each, as `boot::dump` writes them) plus a `txt` sidecar
//! with the wheels' addresses and NGP's thermal scalars. This example loads them into a `Mem`, so
//! `TyreStore::load_frame(scratch_at)` works exactly as in the probe.
//!
//! Kinematic conventions (flat road, documented where used):
//! - y is up: the surface normal is `(0, 1, 0)`, the axle `(1, 0, 0)`. The step builds the contact frame as
//!   `rolling = normal x axle = (0, 0, -1)`, `lateral = rolling x normal = (1, 0, 0)` (`contact_frame::build`).
//! - Forward is world `+z`: at speed `V` and slip angle `a`, the contact velocity is `(V*sin a, 0, V*cos a)`,
//!   i.e. lateral `V*sin a` and longitudinal `-V*cos a`, the sweep's sign convention (forward motion gives a
//!   negative longitudinal velocity, free-rolling spin is positive, `spin = -vlong / R`).
//! - The x87 runs at PC=53 (`Precision::Pc53`): the race thread's control word (`notes/ngp.md`).
//! - Each harness step is one tyre step (`entries::step`) plus one relaxation (`entries::relax_count`) at
//!   `dt = 1/720` s, the wheel step's order (`wheel_step`: `scratch_step`, then `relax_count`). The contact
//!   angle (`record+0x5ac`, the wheel's own word) advances by `spin*dt` as the wheel step advances it, so
//!   heat spreads over the tread sectors as the wheel rolls.
//! - The surface friction words are refreshed to their base values before every step: the step copies the
//!   material into the frame (`copy_material`) and then multiplies by the temperature grip, so the game
//!   refreshes them from the surface each wheel step (on dry tarmac they are constant). Base values are the
//!   captured frictions divided by the captured temperature grip.
//!
//! Outputs: one CSV per test plus `summary.md` in the output dir (default `/tmp/tyre-lab/ngp-dyn`).
//!
//! ```sh
//! cargo run --release -p rbr-tyre-lab --example ngp_transient -- \
//!     --image /tmp/tyre-lab/dyn.img --out /tmp/tyre-lab/ngp-dyn
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use rbr_physics::fpu::{set_precision, Precision};
use rbr_physics::mem::Mem;
use rbr_physics::ngp::heat::{Airflow, TyreShape, TyreThermal};
use rbr_physics::tyre::layout::{entries, Frame, WORDS};
use rbr_physics::tyre::material::Material;
use rbr_physics::tyre::store::TyreStore;

/// The physics step, s.
const DT: f32 = 1.0 / 720.0;
/// The region sizes, matching `tyre_probe::write_image`.
const SIZES: [u32; 5] = [0x600, 0x200, 0x400, 32, 0x600];
/// The harness's argument/output blocks, far from any game address.
const ARGS_AT: u32 = 0x6000_0000;
/// The normal force the scripted tests hold, N.
const TARGET_FZ: f32 = 4000.0;

/// One captured wheel: its footprint addresses and thermal scalars (the sidecar's `wheel K:` line).
#[derive(Clone, Debug)]
struct WheelCap {
    scratch: u32,
    state: u32,
    def: u32,
    wear: u32,
    record: u32,
    kind: u32,
    spin: f32,
    car_speed: f32,
    boost: Option<f32>,
}

/// NGP's thermal scalars (the sidecar's `ngp:` line).
#[derive(Clone, Copy, Debug)]
struct NgpCap {
    air: f32,
    relax: f32,
    factors: [f32; 2],
}

fn parse_u32(s: &str) -> u32 {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(s, 16).unwrap_or_else(|_| s.parse().unwrap())
}

fn parse_f32(s: &str) -> f32 {
    if s == "nan" {
        f32::NAN
    } else {
        s.parse().unwrap()
    }
}

/// The sidecar's `key value ...` words after the colon as `(key, value)` pairs.
fn fields(line: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let rest = line.split_once(':').map_or("", |(_, r)| r);
    let words: Vec<&str> = rest.split_whitespace().collect();
    let mut k = 0;
    while k + 1 < words.len() {
        map.insert(words[k].to_owned(), words[k + 1].to_owned());
        k += 2;
    }
    map
}

fn parse_sidecar(text: &str) -> (Vec<WheelCap>, NgpCap) {
    let mut wheels = Vec::new();
    let mut ngp = NgpCap { air: 293.0, relax: 800.0, factors: [6.0, 6.0] };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("wheel") {
            let m = fields(line);
            let num = |k: &str| parse_u32(m.get(k).map_or("0", String::as_str));
            let flt = |k: &str| parse_f32(m.get(k).map_or("nan", String::as_str));
            let boost = m.get("boost").map(|b| parse_f32(b)).filter(|b| !b.is_nan());
            wheels.push(WheelCap {
                scratch: num("scratch"),
                state: num("state"),
                def: num("def"),
                wear: num("wear"),
                record: num("record"),
                kind: num("kind"),
                spin: flt("spin"),
                car_speed: flt("car_speed"),
                boost,
            });
        } else if line.starts_with("ngp:") {
            let w: Vec<&str> = line.split_whitespace().collect();
            let at =
                |k: &str| w.iter().position(|x| *x == k).and_then(|i| w.get(i + 1)).map_or(f32::NAN, |v| parse_f32(v));
            let f2 = w.iter().position(|x| *x == "factors").and_then(|i| w.get(i + 2)).map_or(6.0, |v| parse_f32(v));
            ngp = NgpCap { air: at("air"), relax: at("relax_rate"), factors: [at("factors"), f2] };
        }
    }
    assert!(wheels.len() == 4, "the sidecar holds four wheels");
    (wheels, ngp)
}

/// The image's records into a `Mem` at their addresses.
fn load_image(path: &str) -> Mem {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut mem = Mem::default();
    let mut at = 0;
    while at < bytes.len() {
        assert!(at + 8 <= bytes.len(), "{path}: truncated record");
        let base = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        assert!(at + 8 + size <= bytes.len(), "{path}: truncated body");
        mem.map(base, &bytes[at + 8..at + 8 + size]);
        at += 8 + size;
    }
    mem
}

/// The lab rig: the captured memory, the chosen wheel's footprint, the step's argument blocks and the
/// thermal state. Each harness step mirrors the wheel step's tyre sequence on this memory.
struct Rig {
    mem: Mem,
    scratch: u32,
    record: u32,
    /// The captured region bytes, for `reset`.
    saved: Vec<(u32, Vec<u8>)>,
    /// The captured temperature words `(address, value)`, for `reset_temps`.
    temps: Vec<(u32, f32)>,
    thermal: TyreThermal,
    material: Material,
    static_idx: usize,
    kinetic_idx: usize,
    base_static: f32,
    base_kinetic: f32,
    slip_scale: f32,
    contact_point: [f32; 4],
    radius0: f32,
    mat: u32,
    vel: u32,
    trq: u32,
    words: [u32; 16],
}

impl Rig {
    fn new(mem: Mem, cap: &WheelCap, ngp: NgpCap) -> Rig {
        let frame: Frame = mem.load_frame(cap.scratch);
        let (s, st, d) = (&frame.scratch, &frame.state, &frame.setup);
        let static_idx = WORDS.iter().position(|&w| w == 0x12).expect("the material holds 0x1e8");
        let kinetic_idx = WORDS.iter().position(|&w| w == 0x16).expect("the material holds 0x1f8");
        let base_static = s.static_friction / s.temperature_grip;
        let base_kinetic = s.kinetic_friction / s.temperature_grip;
        let thermal = TyreThermal {
            air_temperature: ngp.air,
            relax_rate: ngp.relax,
            shape: TyreShape {
                carcass_radius: d.carcass_radius,
                tread_depth: d.tread_depth,
                carcass_thickness: d.carcass_thickness,
                tread_width: d.tread_width,
                sidewall_height: d.sidewall_height,
            },
            airflow: Airflow {
                kind: cap.kind,
                spin: cap.spin,
                car_speed: cap.car_speed,
                boost: cap.boost,
                factors: ngp.factors,
            },
        };
        let mut rig = Rig {
            mem,
            scratch: cap.scratch,
            record: cap.record,
            saved: Vec::new(),
            temps: Vec::new(),
            thermal,
            material: s.material,
            static_idx,
            kinetic_idx,
            base_static,
            base_kinetic,
            slip_scale: s.slip_area_scale,
            contact_point: [s.contact_point.x, s.contact_point.y, s.contact_point.z, s.contact_point.w],
            radius0: st.effective_radius,
            mat: ARGS_AT,
            vel: ARGS_AT + 0x200,
            trq: ARGS_AT + 0x240,
            words: [0; 16],
        };
        // The argument/output blocks: material, debug words, normal, axle, velocity, contact point, force,
        // torque (in/out), reaction torque, sticking load, output block; `words[7]` is unused (`load_step`).
        let (nrm, axl, con, frc, rea, stk, blk, dbg) = (
            ARGS_AT + 0x220,
            ARGS_AT + 0x230,
            ARGS_AT + 0x250,
            ARGS_AT + 0x260,
            ARGS_AT + 0x270,
            ARGS_AT + 0x274,
            ARGS_AT + 0x280,
            ARGS_AT + 0x340,
        );
        rig.mem.map(ARGS_AT, &vec![0; 0x400]);
        // `load_step_flag` reads the game's debug word (`[0x1659b70]+0x24 -> +0x160e0`, bit 0x20);
        // production runs have it clear, so a zeroed stand-in keeps the flag off without the globals.
        rig.mem.map(0x0165_9B70, &[0; 8]);
        rig.mem.set_u32(0x0165_9B70, 0x0165_9B80);
        rig.mem.map(0x0165_9B80, &[0; 0x30]);
        rig.mem.set_u32(0x0165_9B80 + 0x24, 0x0165_9C00);
        rig.mem.map(0x0165_9C00, &vec![0; 0x16100]);
        for (k, v) in [0.0, 1.0, 0.0, 0.0].iter().enumerate() {
            rig.mem.set_f32(nrm + 4 * k as u32, *v);
        }
        for (k, v) in [1.0, 0.0, 0.0, 0.0].iter().enumerate() {
            rig.mem.set_f32(axl + 4 * k as u32, *v);
        }
        rig.words = [
            0,
            DT.to_bits(),
            0,
            s.slip_area_scale.to_bits(),
            rig.mat,
            dbg,
            nrm,
            0,
            axl,
            rig.vel,
            con,
            frc,
            rig.trq,
            rea,
            stk,
            blk,
        ];
        for (k, v) in rig.contact_point.iter().enumerate() {
            rig.mem.set_f32(con + 4 * k as u32, *v);
        }
        // The saved regions and the captured temperature words.
        for (base, size) in [
            (cap.scratch, SIZES[0]),
            (cap.state, SIZES[1]),
            (cap.def, SIZES[2]),
            (cap.wear, SIZES[3]),
            (cap.record, SIZES[4]),
        ] {
            let bytes = rig.mem.try_read(base, size as usize).expect("the image holds the footprint");
            rig.saved.push((base, bytes));
        }
        for k in 0..8 {
            rig.temps.push((cap.state + 0xD0 + 12 * k, st.surface_temperature[k as usize]));
        }
        rig.temps.push((cap.state + 0x12C, st.tread_temperature));
        rig.temps.push((cap.state + 0x130, st.carcass_temperature));
        rig.temps.push((cap.state + 0x64, rig.mem.f32(cap.state + 0x64)));
        rig
    }

    /// The footprint back to the captured bytes.
    fn reset(&mut self) {
        let saved = std::mem::take(&mut self.saved);
        for (base, bytes) in &saved {
            self.mem.map(*base, bytes);
        }
        self.saved = saved;
    }

    /// The captured temperatures back (the mechanics keeps running).
    fn reset_temps(&mut self) {
        for (at, v) in self.temps.clone() {
            self.mem.set_f32(at, v);
        }
    }

    /// Every temperature word (surface sectors, tread, carcass, inner air) to `t`.
    fn set_cold(&mut self, t: f32) {
        for (at, _) in self.temps.clone() {
            self.mem.set_f32(at, t);
        }
    }

    /// One harness step at world velocity `(vx, vz)`, `spin` and `hub_height`: the tyre step plus the
    /// relaxation, the contact angle advanced as the wheel advances it. Returns the loaded frame.
    fn step_once(&mut self, vx: f32, vz: f32, spin: f32, hub: f32) -> Frame {
        self.mem.set_f32(self.vel, vx);
        self.mem.set_f32(self.vel + 4, 0.0);
        self.mem.set_f32(self.vel + 8, vz);
        self.mem.set_f32(self.vel + 12, 0.0);
        for k in 0..4 {
            self.mem.set_u32(self.trq + 4 * k, 0);
        }
        let mut material = self.material;
        material[self.static_idx] = self.base_static.to_bits();
        material[self.kinetic_idx] = self.base_kinetic.to_bits();
        self.mem.store_material(self.mat, &material);
        self.words[0] = spin.to_bits();
        self.words[2] = hub.to_bits();
        self.words[3] = self.slip_scale.to_bits();
        let words = self.words;
        entries::step(&mut self.mem, self.scratch, &words, &self.thermal)
            .expect("the flat-road step stays in the frame");
        entries::relax_count(&mut self.mem, self.scratch, DT, &self.thermal);
        let angle = self.mem.f32(self.record + 0x5AC) + spin * DT;
        self.mem.set_f32(self.record + 0x5AC, angle);
        self.thermal.airflow.spin = spin;
        self.mem.load_frame(self.scratch)
    }

    /// The hub height giving `target` N at `(vx, vz, spin)`, by bisection from a restored footprint.
    fn bisect_hub(&mut self, vx: f32, vz: f32, spin: f32, target: f32) -> f32 {
        let frame: Frame = self.mem.load_frame(self.scratch);
        let wear = frame.scratch.wear[frame.state.sector as usize].clamp(0.0, 0.99);
        let outer = frame.setup.carcass_radius + (1.0 - wear) * frame.setup.tread_depth;
        let (mut lo, mut hi) = (outer - 0.06, outer - 0.00015);
        for _ in 0..24 {
            self.reset();
            let mid = 0.5 * (lo + hi);
            let f = self.step_once(vx, vz, spin, mid);
            if f.scratch.normal_force > target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        self.reset();
        0.5 * (lo + hi)
    }

    /// Nudge a live hub toward `target` N (Newton with the contact's sign, preserving temperatures):
    /// higher hub, less penetration, less force. A few dozen steps per iteration.
    fn hold_fz(&mut self, vx: f32, vz: f32, spin: f32, hub: f32, target: f32) -> f32 {
        let mut hub = hub;
        for _ in 0..6 {
            for _ in 0..30 {
                self.step_once(vx, vz, spin, hub);
            }
            let fz = self.mem.f32(self.scratch + 0x314);
            if (fz - target).abs() < 5.0 {
                break;
            }
            hub += (fz - target) / 3.0e5;
        }
        hub
    }

    /// Run `max_steps` at `(vx, vz, spin, hub)`; the settled step count and final `|Fy|` once the last-200
    /// window's spread is under `max(2 N, 0.2% of the mean)`.
    fn settle(&mut self, vx: f32, vz: f32, spin: f32, hub: f32, max_steps: u32) -> (u32, f32) {
        let mut ring = [0.0f32; 200];
        let mut done = max_steps;
        let mut fy = 0.0;
        for k in 0..max_steps {
            let f = self.step_once(vx, vz, spin, hub);
            fy = f.scratch.lateral_force;
            ring[k as usize % 200] = fy;
            if k >= 199 {
                let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
                for v in ring {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
                let mean = ring.iter().sum::<f32>() / 200.0;
                if hi - lo < (2.0f32).max(0.002 * mean.abs()) {
                    done = k + 1;
                    break;
                }
            }
        }
        (done, fy.abs())
    }
}

/// World `(vx, vz)` and free-rolling spin at `v` m/s and slip angle `alpha_deg` (radius `r`):
/// the contact sees lateral `v*sin a`, longitudinal `-v*cos a`, spin `-vlong/r`.
fn lateral_kin(v: f32, alpha_deg: f32, r: f32) -> (f32, f32, f32) {
    let a = alpha_deg.to_radians();
    let (vx, vz) = (v * a.sin(), v * a.cos());
    (vx, vz, vz / r)
}

/// World `(vx, vz)` and spin at `v` m/s and slip ratio `kappa` (straight ahead).
fn long_kin(v: f32, kappa: f32, r: f32) -> (f32, f32, f32) {
    (0.0, v, (1.0 + kappa) * v / r)
}

fn write(path: &std::path::Path, text: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    }
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn main() {
    set_precision(Precision::Pc53);
    let mut image = String::new();
    let mut out = std::path::PathBuf::from("/tmp/tyre-lab/ngp-dyn");
    let mut sweep = std::path::PathBuf::from("/tmp/tyre-lab/ngp");
    let mut wheel_arg: Option<u32> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--image" => image = args.next().expect("--image FILE"),
            "--out" => out = args.next().expect("--out DIR").into(),
            "--sweep" => sweep = args.next().expect("--sweep DIR").into(),
            "--wheel" => wheel_arg = Some(args.next().expect("--wheel K").parse().expect("wheel")),
            _ => panic!("unknown arg {a}"),
        }
    }
    assert!(!image.is_empty(), "--image FILE required");
    let mem = load_image(&image);
    let side_text =
        std::fs::read_to_string(Path::new(&image).with_extension("txt")).unwrap_or_else(|e| panic!("sidecar: {e}"));
    let (caps, ngp) = parse_sidecar(&side_text);
    println!("ngp thermal: air {:.3} K relax {} factors {:?}", ngp.air, ngp.relax, ngp.factors);

    // The wheel: `--wheel K`, else the front wheel (0/1) with the larger captured normal force.
    let pick = match wheel_arg {
        Some(k) => k,
        None => {
            let fz: Vec<f32> = caps.iter().map(|c| mem.load_frame(c.scratch).scratch.normal_force).collect();
            println!("captured Fz per wheel: {fz:.0?}");
            if fz[0] >= fz[1] {
                0
            } else {
                1
            }
        }
    };
    println!("wheel {pick}");
    let mut rig = Rig::new(mem, &caps[pick as usize], ngp);
    println!(
        "setup: R0 {:.4} m base_static {:.4} base_kinetic {:.4} slip_scale {:.4} kind {}",
        rig.radius0, rig.base_static, rig.base_kinetic, rig.slip_scale, caps[pick as usize].kind
    );
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{}: {e}", out.display()));

    let r0 = rig.radius0;
    let mut md = String::from("# NGP tyre dynamics (per-wheel step harness)\n\n");
    md.push_str(&format!("Image `{image}`, wheel {pick} (front, loaded outside), dt = 1/720 s.\n\n"));
    md.push_str(
        "Methods: flat road (normal +y, axle +x), slip angle by rotating the world velocity, spin free-rolling \
(+kappa for longitudinal), hub bisected for 4 kN unless noted. Surface friction words refreshed to base \
(captured/temperature-grip) each step, as the game refreshes them per wheel step; one tyre step plus one \
relaxation per harness step; contact angle advances by spin*dt; x87 at PC=53; the game's step debug flag stood \
in as zero. Temperatures restored to captured before each validation point and each step test's settle.\n\n",
    );

    // --- validation: quasi-static slip-angle ramp at 15 m/s vs the sweep -------------------------------
    let lat_text = std::fs::read_to_string(sweep.join("lat_alpha.csv")).expect("sweep lat_alpha.csv");
    let mut sweep_mu: BTreeMap<(u32, u32), f32> = BTreeMap::new();
    for line in lat_text.lines().skip(1) {
        let v: Vec<&str> = line.split(',').collect();
        if v.len() < 4 {
            continue;
        }
        let fz: u32 = v[0].parse().unwrap_or(0);
        let alpha_bits = (v[1].parse::<f32>().unwrap_or(-1.0) * 100.0).round() as u32;
        sweep_mu.insert((fz, alpha_bits), v[3].parse().unwrap_or(f32::NAN));
    }
    rig.reset();
    let (vx0, vz0, spin0) = lateral_kin(15.0, 0.0, r0);
    let hub15 = rig.bisect_hub(vx0, vz0, spin0, TARGET_FZ);
    println!("validation hub at 15 m/s for 4 kN: {hub15:.5} m");
    rig.settle(vx0, vz0, spin0, hub15, 3000);
    let mut val = String::from("alpha_deg,Fy_N,Fz_N,mu,temp_tread_K,mu_sweep\n");
    let mut val_curve: Vec<(f32, f32)> = Vec::new();
    let (mut worst, mut sum, mut n, mut calpha_num, mut calpha_den) = (0.0f32, 0.0f32, 0u32, 0.0f64, 0.0f64);
    let mut na = 0;
    loop {
        let alpha = na as f32 * 0.25;
        if alpha > 30.0 {
            break;
        }
        na += 1;
        rig.reset_temps();
        let (vx, vz, spin) = lateral_kin(15.0, alpha, r0);
        let mut f = rig.mem.load_frame(rig.scratch);
        for _ in 0..400 {
            f = rig.step_once(vx, vz, spin, hub15);
        }
        let mu = f.scratch.lateral_force.abs() / f.scratch.normal_force;
        let key = (4000u32, (alpha * 100.0).round() as u32);
        let mu_s = sweep_mu.get(&key).copied().unwrap_or(f32::NAN);
        if alpha <= 12.0 && mu_s.is_finite() {
            worst = worst.max((mu - mu_s).abs());
            sum += (mu - mu_s).abs();
            n += 1;
        }
        if alpha <= 2.0 && alpha > 0.0 {
            calpha_num += f64::from(alpha.to_radians()) * f64::from(-f.scratch.lateral_force);
            calpha_den += f64::from(alpha.to_radians()).powi(2);
        }
        val_curve.push((alpha, f.scratch.lateral_force));
        val.push_str(&format!(
            "{alpha:.2},{:.2},{:.1},{mu:.4},{:.2},{mu_s:.4}\n",
            f.scratch.lateral_force, f.scratch.normal_force, f.state.tread_temperature,
        ));
    }
    write(&out.join("validation.csv"), &val);
    // --- (a) lateral step: 0 -> 2 and 0 -> 6 deg at 5, 10, 20, 30 m/s, Fz = 4 kN ----------------
    let mut lat = String::from("V_ms,target_deg,t_s,x_m,Fy_N,Fz_N,sliding_w,sticking_w\n");
    let mut a_md = String::from("## (a) Lateral step response\n\n");
    a_md.push_str("Settled at 0 deg, then stepped; the 6 deg / 5 m/s run never met the settle criterion (slow heat drift), so its final is the tail mean - the crossing distances are unaffected.\n\n");
    a_md.push_str("| V (m/s) | target (deg) | final Fy (N) | x63 (m) | x90 (m) |\n| --- | --- | --- | --- | --- |\n");
    for v in [5.0f32, 10.0, 20.0, 30.0] {
        rig.reset();
        rig.reset_temps();
        let (vx0, vz0, spin0) = lateral_kin(v, 0.0, r0);
        let hub = rig.bisect_hub(vx0, vz0, spin0, TARGET_FZ);
        for target in [2.0f32, 6.0] {
            rig.reset();
            rig.reset_temps();
            rig.settle(vx0, vz0, spin0, hub, 3000);
            rig.reset_temps();
            let (vx, vz, spin) = lateral_kin(v, target, r0);
            let mut rows: Vec<(f32, f32, f32, f32)> = Vec::new();
            let mut settled_at = 3000u32;
            let mut ring = [0.0f32; 200];
            for k in 0..3000u32 {
                let f = rig.step_once(vx, vz, spin, hub);
                let t = k as f32 * DT;
                rows.push((t, f.scratch.lateral_force, f.scratch.normal_force, f.scratch.sliding_weight));
                ring[k as usize % 200] = f.scratch.lateral_force;
                if k >= 1199 && settled_at == 3000 {
                    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
                    for x in ring {
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                    let mean = ring.iter().sum::<f32>() / 200.0;
                    if hi - lo < 2.0f32.max(0.002 * mean.abs()) {
                        settled_at = k + 1;
                    }
                }
                if settled_at != 3000 && k >= settled_at + 300 {
                    break;
                }
            }
            let tail = &rows[rows.len() * 9 / 10..];
            let f_final = tail.iter().map(|r| r.1).sum::<f32>() / tail.len() as f32;
            let cross =
                |frac: f32| rows.iter().find(|r| r.1.abs() >= frac * f_final.abs()).map_or(f32::NAN, |r| r.0 * v);
            let (x63, x90) = (cross(0.63), cross(0.90));
            for (t, fy, fz, w) in &rows {
                lat.push_str(&format!(
                    "{v:.1},{target:.1},{t:.4},{:.4},{fy:.2},{fz:.1},{w:.4},{:.4}\n",
                    t * v,
                    1.0 - w
                ));
            }
            a_md.push_str(&format!("| {v:.0} | {target:.0} | {f_final:.0} | {x63:.3} | {x90:.3} |\n"));
            println!(
                "lat step V {v} -> {target} deg: F {f_final:.0} N x63 {x63:.3} m x90 {x90:.3} m ({} steps)",
                rows.len()
            );
        }
    }
    write(&out.join("lat_step.csv"), &lat);

    let calpha = (calpha_num / calpha_den) as f32;
    println!("validation: worst |dmu| {worst:.4} mean {:.4} Calpha {calpha:.0} N/rad", sum / n.max(1) as f32);
    md.push_str(&format!(
        "## Harness validation (quasi-static ramp at 15 m/s, Fz = 4 kN)\n\n400 steps per 0.25 deg point \
(with the captured temperatures restored at each point), hub bisected once at 0 deg. \
Worst |mu - mu_sweep| over 0-12 deg: {worst:.4}, mean {:.4}; cornering stiffness {:.0} kN/rad \
(sweep: {:.1}).\n\n",
        sum / n.max(1) as f32,
        calpha / 1000.0,
        70.4,
    ));
    md.push_str(&a_md);

    // --- (b) longitudinal step: 0 -> 0.03 and 0 -> 0.1 at 10 and 20 m/s -------------------------
    let mut lon = String::from("V_ms,target_kappa,t_s,x_m,Fx_N,Fz_N,sliding_w,sticking_w\n");
    let mut b_md = String::from("## (b) Longitudinal step response\n\n");
    b_md.push_str("The torsion state integrates spin-slip per step toward the equilibrium set by the torsional stiffness, which NGP scales 0.1x at rest to 1x from 20 m/s on - hence the sluggish low-speed response.\n\n");
    b_md.push_str("| V (m/s) | target kappa | final Fx (N) | x63 (m) | x90 (m) |\n| --- | --- | --- | --- | --- |\n");
    for v in [10.0f32, 20.0] {
        rig.reset();
        rig.reset_temps();
        let (vx0, vz0, spin0) = long_kin(v, 0.0, r0);
        let hub = rig.bisect_hub(vx0, vz0, spin0, TARGET_FZ);
        for kappa in [0.03f32, 0.1] {
            rig.reset();
            rig.reset_temps();
            rig.settle(vx0, vz0, spin0, hub, 3000);
            rig.reset_temps();
            let (vx, vz, spin) = long_kin(v, kappa, r0);
            let mut rows: Vec<(f32, f32, f32, f32)> = Vec::new();
            let mut settled_at = 3000u32;
            let mut ring = [0.0f32; 200];
            for k in 0..3000u32 {
                let f = rig.step_once(vx, vz, spin, hub);
                rows.push((
                    k as f32 * DT,
                    f.scratch.longitudinal_force,
                    f.scratch.normal_force,
                    f.scratch.sliding_weight,
                ));
                ring[k as usize % 200] = f.scratch.longitudinal_force;
                if k >= 1199 && settled_at == 3000 {
                    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
                    for x in ring {
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                    let mean = ring.iter().sum::<f32>() / 200.0;
                    if hi - lo < 2.0f32.max(0.002 * mean.abs()) {
                        settled_at = k + 1;
                    }
                }
                if settled_at != 3000 && k >= settled_at + 300 {
                    break;
                }
            }
            let tail = &rows[rows.len() * 9 / 10..];
            let f_final = tail.iter().map(|r| r.1).sum::<f32>() / tail.len() as f32;
            let cross =
                |frac: f32| rows.iter().find(|r| r.1.abs() >= frac * f_final.abs()).map_or(f32::NAN, |r| r.0 * v);
            let (x63, x90) = (cross(0.63), cross(0.90));
            for (t, fx, fz, w) in &rows {
                lon.push_str(&format!("{v:.1},{kappa:.3},{t:.4},{:.4},{fx:.2},{fz:.1},{w:.4},{:.4}\n", t * v, 1.0 - w));
            }
            b_md.push_str(&format!("| {v:.0} | {kappa:.2} | {f_final:.0} | {x63:.3} | {x90:.3} |\n"));
            println!("long step V {v} -> k {kappa}: F {f_final:.0} N x63 {x63:.3} m x90 {x90:.3} m");
        }
    }
    write(&out.join("long_step.csv"), &lon);
    md.push_str(&b_md);

    // --- (c) sinusoidal steer: 2 deg at 0.5, 1, 2, 5 Hz at 20 m/s --------------------------------
    rig.reset();
    rig.reset_temps();
    let (cx0, cz0, cs0) = lateral_kin(20.0, 0.0, r0);
    let chub = rig.bisect_hub(cx0, cz0, cs0, TARGET_FZ);
    // The quasi-static amplitude from short cool holds (400 steps, as in the validation): long settles
    // would warm the surface sector and bias the reference down while the sine runs start cool.
    rig.settle(cx0, cz0, cs0, chub, 3000);
    let (px, pz, ps) = lateral_kin(20.0, 2.0, r0);
    rig.reset_temps();
    let mut f = rig.mem.load_frame(rig.scratch);
    for _ in 0..400 {
        f = rig.step_once(px, pz, ps, chub);
    }
    let f_plus = f.scratch.lateral_force.abs();
    rig.reset();
    rig.reset_temps();
    rig.settle(cx0, cz0, cs0, chub, 3000);
    let (mx, mz, ms) = lateral_kin(20.0, -2.0, r0);
    rig.reset_temps();
    for _ in 0..400 {
        f = rig.step_once(mx, mz, ms, chub);
    }
    let f_minus = f.scratch.lateral_force.abs();
    println!("sine reference: |Fy(+2)| {f_plus:.0} N |Fy(-2)| {f_minus:.0} N");
    // The static curve's own fundamental at 2 deg (the curve is nonlinear, so the sine's fundamental
    // must be compared against it, not against the endpoint value). Trapezoid over one cycle.
    let static_at = |a: f32| {
        let s = if a >= 0.0 { 1.0 } else { -1.0 };
        let a = a.abs();
        let mut k = 0;
        while k + 1 < val_curve.len() && val_curve[k + 1].0 < a {
            k += 1;
        }
        let (a0, f0) = val_curve[k];
        let (a1, f1) = val_curve[(k + 1).min(val_curve.len() - 1)];
        let t = if a1 > a0 { ((a - a0) / (a1 - a0)).clamp(0.0, 1.0) } else { 0.0 };
        ((1.0 - t) * f0 + t * f1) * s
    };
    let mut static_fund = 0.0f32;
    let nn = 720usize;
    for k in 0..nn {
        let t = k as f32 / nn as f32 * 2.0 * std::f32::consts::PI;
        static_fund += static_at(2.0 * t.sin()) * t.sin();
    }
    static_fund = 2.0 * static_fund / nn as f32;
    println!("sine static fundamental at 2 deg: {static_fund:.0} N");
    let a_qs = 0.5 * (f_plus + f_minus);
    let mut sine = String::from("freq_Hz,t_s,alpha_deg,Fy_N\n");
    let mut c_md = String::from("## (c) Sinusoidal steer (2 deg at 20 m/s)\n\n");
    c_md.push_str(&format!("Quasi-static endpoint at +-2 deg: {a_qs:.0} N; static curve's fundamental: {static_fund:.0} N (gain is against the fundamental).\n\n"));
    c_md.push_str("| f (Hz) | gain | phase lag (deg) | temp_grip first -> last |\n| --- | --- | --- | --- |\n");
    for freq in [0.5f32, 1.0, 2.0, 5.0] {
        rig.reset();
        rig.reset_temps();
        rig.settle(cx0, cz0, cs0, chub, 3000);
        rig.reset_temps();
        let total = 1.0 + 6.0 / freq;
        let steps = (total / DT) as u32;
        let mut rec: Vec<(f32, f32)> = Vec::new();
        let (mut g0, mut g1) = (f32::NAN, f32::NAN);
        for k in 0..steps {
            let t = k as f32 * DT;
            let alpha = 2.0 * (2.0 * std::f32::consts::PI * freq * t).sin();
            let (vx, vz, spin) = lateral_kin(20.0, alpha, r0);
            let f = rig.step_once(vx, vz, spin, chub);
            if k == 0 {
                g0 = f.scratch.temperature_grip;
            }
            g1 = f.scratch.temperature_grip;
            rec.push((alpha, f.scratch.lateral_force));
            sine.push_str(&format!("{freq:.1},{t:.4},{alpha:.4},{:.2}\n", f.scratch.lateral_force));
        }
        // Gain: the least-squares fundamental over the last 4 cycles vs the static curve's fundamental;
        // lag from the correlation peak of -Fy vs alpha.
        let per = (1.0 / freq / DT) as usize;
        let fit = &rec[rec.len() - 4 * per..];
        let w = f64::from(2.0 * std::f32::consts::PI * freq);
        let t0 = f64::from(total - 4.0 / freq);
        let mean = fit.iter().map(|r| f64::from(r.1)).sum::<f64>() / fit.len() as f64;
        let (mut qss, mut qcc, mut qsc, mut qs, mut qc) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (i, r) in fit.iter().enumerate() {
            let (sn, cs) = (w * (t0 + i as f64 * f64::from(DT))).sin_cos();
            let y = f64::from(r.1) - mean;
            qss += sn * sn;
            qcc += cs * cs;
            qsc += sn * cs;
            qs += sn * y;
            qc += cs * y;
        }
        let det = qss * qcc - qsc * qsc;
        let amp = ((qs * qcc - qsc * qc).powi(2) + (qss * qc - qsc * qs).powi(2)).sqrt() / det;
        let gain = amp as f32 / static_fund.abs();
        let mut best = (0usize, f64::NEG_INFINITY);
        for lag in 0..per {
            let mut num = 0.0f64;
            let (mut sa, mut sb) = (0.0f64, 0.0f64);
            for (i, r) in fit.iter().enumerate() {
                let a = f64::from(fit[(i + per - lag) % fit.len()].0);
                let y = f64::from(r.1) - mean;
                num += a * -y;
                sa += a * a;
                sb += y * y;
            }
            let corr = num / (sa * sb).sqrt().max(1.0e-9);
            if corr > best.1 {
                best = (lag, corr);
            }
        }
        let lag_deg = best.0 as f32 * DT * freq * 360.0;
        c_md.push_str(&format!("| {freq:.1} | {gain:.3} | {lag_deg:.1} | {g0:.4} -> {g1:.4} |\n"));
        println!("sine {freq} Hz: gain {gain:.3} lag {lag_deg:.1} deg grip {g0:.4} -> {g1:.4}");
    }
    write(&out.join("sine.csv"), &sine);
    md.push_str(&c_md);

    // --- (d) low speed: 4 deg slip at 0.2..2.5 m/s, plus a standstill push ------------------------
    let mut low = String::from("V_ms,t_s,Fy_N,Fz_N,sliding_w,sticking_w\n");
    let mut d_md = String::from("## (d) Low speed (4 deg slip)\n\n");
    d_md.push_str("Weights are the sticking/sliding blend (full sliding from 2 m/s). The sticking regime chatters with period 2 steps (360 Hz), amplitude growing with speed; at 0.2 m/s the run's std is slow drift, not chatter (max step jump 9 N).\n\n");
    d_md.push_str(
        "| V (m/s) | Fy (N) | sliding w | chatter std (N) | max step jump (N) |\n| --- | --- | --- | --- | --- |\n",
    );
    for v in [0.2f32, 0.5, 1.0, 1.5, 2.5] {
        rig.reset();
        rig.reset_temps();
        let (zx, zz, zs) = lateral_kin(v, 0.0, r0);
        let hub = rig.bisect_hub(zx, zz, zs, TARGET_FZ);
        let (vx, vz, spin) = lateral_kin(v, 4.0, r0);
        let mut rec: Vec<(f32, f32, f32)> = Vec::new();
        for k in 0..2000u32 {
            let f = rig.step_once(vx, vz, spin, hub);
            if k >= 500 {
                rec.push((f.scratch.lateral_force, f.scratch.normal_force, f.scratch.sliding_weight));
                low.push_str(&format!(
                    "{v:.1},{:.4},{:.2},{:.1},{:.4},{:.4}\n",
                    k as f32 * DT,
                    f.scratch.lateral_force,
                    f.scratch.normal_force,
                    f.scratch.sliding_weight,
                    f.scratch.sticking_weight
                ));
            }
        }
        let tail = &rec[rec.len() - 500..];
        let mean = tail.iter().map(|r| r.0).sum::<f32>() / tail.len() as f32;
        let std = (tail.iter().map(|r| (r.0 - mean).powi(2)).sum::<f32>() / tail.len() as f32).sqrt();
        let jump = tail.windows(2).map(|w| (w[1].0 - w[0].0).abs()).fold(0.0f32, f32::max);
        let w = tail.last().map_or(f32::NAN, |r| r.2);
        d_md.push_str(&format!("| {v:.1} | {mean:.0} | {w:.3} | {std:.2} | {jump:.2} |\n"));
        println!("low speed V {v}: Fy {mean:.0} N w_slide {w:.3} std {std:.2} jump {jump:.2}");
    }
    // Standstill (V = 0, spin 0) with a small lateral push of 5 cm/s.
    rig.reset();
    rig.reset_temps();
    let hub0 = rig.bisect_hub(0.0, 0.0, 0.0, TARGET_FZ);
    let mut push = String::from("t_s,Fy_N,Fz_N,sliding_w,sticking_w\n");
    let mut fy: Vec<f32> = Vec::new();
    for k in 0..1440u32 {
        let f = rig.step_once(0.05, 0.0, 0.0, hub0);
        fy.push(f.scratch.lateral_force);
        // Every step: the sticking regime chatters with period 2, which sparser sampling would hide.
        push.push_str(&format!(
            "{:.4},{:.2},{:.1},{:.4},{:.4}\n",
            k as f32 * DT,
            f.scratch.lateral_force,
            f.scratch.normal_force,
            f.scratch.sliding_weight,
            f.scratch.sticking_weight
        ));
    }
    let tail = &fy[fy.len() - 500..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    let std = (tail.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / tail.len() as f32).sqrt();
    let jump = tail.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    d_md.push_str(&format!(
        "\nStandstill with a 5 cm/s lateral push: Fy {mean:.0} N, std {std:.2} N, max step jump {jump:.2} N.\n"
    ));
    println!("standstill push: Fy {mean:.0} N std {std:.2} jump {jump:.2}");
    write(&out.join("low_speed.csv"), &low);
    write(&out.join("standstill.csv"), &push);
    md.push_str(&d_md);

    // --- (e) heat: 6 deg at 20 m/s for 20 s from captured temps, 20 s cool, then from cold --------
    // The hub is nudged to 4 kN at each phase start (Newton, preserving temperatures, <=180 steps of
    // pre-warming); the hub then stays fixed and Fz drifts slightly with the air pressure.
    let mut heat = String::from("run,t_s,hub_m,tread_K,surf_K,carcass_K,Fy_N,Fz_N,mu,temp_grip\n");
    // A heat phase: `secs` at (vx, vz, spin), sampled every 72 steps; returns (t, tread, mu).
    fn phase(
        rig: &mut Rig,
        heat: &mut String,
        run: &str,
        kin: [f32; 3],
        hub: f32,
        secs: f32,
        t0: f32,
    ) -> (f32, Vec<(f32, f32, f32)>) {
        let steps = (secs / DT) as u32;
        let mut trace = Vec::new();
        for k in 0..steps {
            let t = t0 + k as f32 * DT;
            let f = rig.step_once(kin[0], kin[1], kin[2], hub);
            if k % 72 == 0 {
                let sector = f.state.sector as usize;
                let mu = f.scratch.lateral_force.abs() / f.scratch.normal_force;
                trace.push((t, f.state.tread_temperature, mu));
                heat.push_str(&format!(
                    "{run},{t:.3},{hub:.5},{:.2},{:.2},{:.2},{:.1},{:.1},{mu:.4},{:.4}\n",
                    f.state.tread_temperature,
                    f.state.surface_temperature[sector],
                    f.state.carcass_temperature,
                    f.scratch.lateral_force,
                    f.scratch.normal_force,
                    f.scratch.temperature_grip,
                ));
            }
        }
        (t0 + steps as f32 * DT, trace)
    }
    /// The exponential time constant of `trace` (t, T): Tinf and tau by grid search, T0 fixed.
    fn tau_of(trace: &[(f32, f32)]) -> (f32, f32) {
        let t0 = trace[0].1;
        let mut best = (f32::NAN, f32::NAN, f32::INFINITY);
        let mut tau = 0.5f32;
        while tau <= 60.0 {
            let (mut sxx, mut sxy) = (0.0f64, 0.0f64);
            for (t, temp) in trace {
                let x = 1.0 - (-(t - trace[0].0) / tau).exp();
                sxx += x as f64 * x as f64;
                sxy += x as f64 * f64::from(temp - t0);
            }
            let slope = sxy / sxx;
            let mut sse = 0.0f64;
            for (t, temp) in trace {
                let x = 1.0 - (-(t - trace[0].0) / tau).exp();
                let r = f64::from(temp - t0) - slope * x as f64;
                sse += r * r;
            }
            if sse < best.2 as f64 {
                best = (slope as f32 + t0, tau, sse as f32);
            }
            tau *= 1.15;
        }
        (best.0, best.1)
    }
    let (hx, hz, hs) = lateral_kin(20.0, 6.0, r0);
    let (cx, cz, cs) = lateral_kin(20.0, 0.0, r0);
    rig.reset();
    let mut hub = rig.hold_fz(hx, hz, hs, rig.mem.f32(rig.scratch + 0x3A8), TARGET_FZ);
    let (_, hot_heat) = phase(&mut rig, &mut heat, "hot_heat", [hx, hz, hs], hub, 20.0, 0.0);
    hub = rig.hold_fz(cx, cz, cs, hub, TARGET_FZ);
    let (_, hot_cool) = phase(&mut rig, &mut heat, "hot_cool", [cx, cz, cs], hub, 20.0, 20.0);
    rig.reset();
    rig.set_cold(293.0);
    hub = rig.hold_fz(hx, hz, hs, hub, TARGET_FZ);
    let (_, cold_heat) = phase(&mut rig, &mut heat, "cold_heat", [hx, hz, hs], hub, 20.0, 0.0);
    write(&out.join("heat.csv"), &heat);
    let tread_of = |trace: &[(f32, f32, f32)]| trace.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>();
    let (tinf_hot, tau_hot) = tau_of(&tread_of(&hot_heat));
    let (tinf_cold, tau_cold) = tau_of(&tread_of(&cold_heat));
    let (_, tau_cool) = tau_of(&tread_of(&hot_cool));
    let end = |trace: &[(f32, f32, f32)]| trace.last().map_or((f32::NAN, f32::NAN), |x| (x.1, x.2));
    let (hot_end_t, hot_end_mu) = end(&hot_heat);
    let (cold_end_t, cold_end_mu) = end(&cold_heat);
    let hot_start_mu = hot_heat.first().map_or(f32::NAN, |x| x.2);
    let cold_start_mu = cold_heat.first().map_or(f32::NAN, |x| x.2);
    let mut e_md = String::from("## (e) Heat (6 deg at 20 m/s)\n\n");
    e_md.push_str(&format!(
        "Tread temperature: hot start {:.1} K -> 20 s {hot_end_t:.1} K (fit Tinf {tinf_hot:.1} K, tau {tau_hot:.1} s); \
cool-down tau {tau_cool:.1} s; cold start 293.0 K -> 20 s {cold_end_t:.1} K (fit Tinf {tinf_cold:.1} K, tau {tau_cold:.1} s).\n\n\
Grip (mu at the held 6 deg slip): hot {hot_start_mu:.3} -> {hot_end_mu:.3}; cold {cold_start_mu:.3} -> {cold_end_mu:.3}.\n",
        hot_heat[0].1,
    ));
    md.push_str(&e_md);
    println!(
        "heat: hot {:.1} -> {hot_end_t:.1} K tau {tau_hot:.1} s mu {hot_start_mu:.3} -> {hot_end_mu:.3}; \
cold -> {cold_end_t:.1} K tau {tau_cold:.1} s mu {cold_start_mu:.3} -> {cold_end_mu:.3}",
        hot_heat[0].1,
    );

    write(&out.join("summary.md"), &md);
    println!("csv + summary.md in {}", out.display());
}
