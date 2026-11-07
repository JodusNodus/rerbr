//! Fits the class table (`rbr_season::pace::CLASSES`) to class effects: each class's time over the anchor class
//! (`WRC 2.0`) on tarmac, gravel and snow, as the RallySimFans hot laps give them (`notes/season.md` "Calibration";
//! the CSV is the boards' and stays out of git). On the retail stages' roads, from the class's measured shape, a
//! uniform speed scale `c` (grip, accel and brake times `c²`, top speed times `c`, every time over `c`, repeated as
//! the plan's shared high-speed grip does not scale) meets the tarmac effect, then `loose` the mean of the gravel and
//! snow ones; the rows print as the table's source.
//!
//! Run with `bash -c 'source scripts/dev-env.sh; cargo run -p rbr-season --example class_fit -- EFFECTS.csv'`
//! (`RBR_DATA`; the CSV's lines `class,tarmac,gravel,snow[,grip,brake,accel,top]`, the last four the shape of the
//! class's pace over the anchor's, measured, else the row's own shape is kept).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rbr_formats::rsf::Surface;
use rbr_formats::trk::{self, DriveLineData};
use rbr_season::pace::{class_pace, reference_time, timed_road, ClassPace, CLASSES};

/// A class's times over the anchor's on tarmac, gravel and snow, and the shape of its pace over the anchor's (grip,
/// brake, accel, top speed: the NGP physics' own runs of its installed cars, `scripts/car-pace-sweep.sh`), if given.
struct Effect {
    times: [f32; 3],
    shape: Option<[f32; 4]>,
}

fn read_trk(path: &Path) -> Option<DriveLineData> {
    let file = trk::parse(&std::fs::read(path).ok()?).ok()?;
    file.chunks.into_iter().find_map(|c| match c.payload {
        trk::ChunkPayload::DriveLine(d) => Some(d),
        _ => None,
    })
}

fn main() {
    let data = std::env::var_os("RBR_DATA").map(PathBuf::from).expect("RBR_DATA");
    let effects = std::env::args().nth(1).expect("the class effects CSV");
    let effects: HashMap<String, Effect> = std::fs::read_to_string(&effects)
        .expect("the class effects CSV")
        .lines()
        .filter_map(|l| {
            let v: Vec<&str> = l.split(',').collect();
            let n = |k: usize| v.get(k)?.trim().parse::<f32>().ok();
            let shape = (|| Some([n(4)?, n(5)?, n(6)?, n(7)?]))();
            Some((v[0].to_owned(), Effect { times: [n(1)?, n(2)?, n(3)?], shape }))
        })
        .collect();
    // The retail roads: the France maps (4x) are tarmac, the rest gravel; snow runs on the gravel roads.
    let mut roads: Vec<(Surface, Vec<[f32; 3]>)> = Vec::new();
    for entry in std::fs::read_dir(data.join("Maps")).expect("Maps").flatten() {
        let stem = entry.path().file_stem().unwrap().to_string_lossy().into_owned();
        let Some(map) = stem.strip_prefix("track-").and_then(|r| r.split('_').next()?.parse::<u32>().ok()) else {
            continue;
        };
        if let Some(line) = read_trk(&entry.path()) {
            let surface = if map / 10 == 4 { Surface::Tarmac } else { Surface::Gravel };
            roads.push((surface, timed_road(&line, None).points));
        }
    }
    let time = |pace: &ClassPace, surface: Surface| -> f32 {
        let on = if surface == Surface::Tarmac { Surface::Tarmac } else { Surface::Gravel };
        roads.iter().filter(|r| r.0 == on).map(|(_, p)| reference_time(p, surface, 0.0, pace)).sum()
    };
    let anchor = class_pace("WRC 2.0");
    let base = [Surface::Tarmac, Surface::Gravel, Surface::Snow].map(|s| time(&anchor, s));
    let ratio = |pace: &ClassPace, k: usize, s: Surface| time(pace, s) / base[k];
    for row in CLASSES {
        let Some(effect) = effects.get(row.name) else {
            println!("    // {}: no effect, kept", row.name);
            continue;
        };
        let [tarmac, gravel, snow] = effect.times;
        // The row's shape: the measured one over the anchor's, else the row's own.
        let p = match effect.shape {
            Some([grip, brake, accel, top]) => ClassPace {
                grip: anchor.grip * grip,
                brake: anchor.brake * brake,
                accel: anchor.accel * accel,
                top_speed: anchor.top_speed * top,
                ..row.pace
            },
            None => row.pace,
        };
        // The plan's shared high-speed grip does not scale with `c`: a few rounds settle it.
        let mut scaled = p;
        for _ in 0..6 {
            let c = ratio(&scaled, 0, Surface::Tarmac) / tarmac;
            scaled = ClassPace {
                grip: scaled.grip * c * c,
                accel: scaled.accel * c * c,
                brake: scaled.brake * c * c,
                top_speed: scaled.top_speed * c,
                ..scaled
            };
        }
        // The loose share: bisection on the mean of the gravel and snow misses.
        let miss = |l: f32| -> f32 {
            let q = ClassPace { loose: l, ..scaled };
            (ratio(&q, 1, Surface::Gravel) / gravel).ln() + (ratio(&q, 2, Surface::Snow) / snow).ln()
        };
        let (mut lo, mut hi) = (0.6f32, 1.4f32);
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            if miss(mid) > 0.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let fit = ClassPace { loose: 0.5 * (lo + hi), ..scaled };
        println!(
            "    row({:?}, {:?}, {:.2}, {:.2}, {:.2}, {:.1}, {:.3}), // {:+.1} / {:+.1} / {:+.1} %",
            row.name,
            row.keys,
            fit.grip,
            fit.accel,
            fit.brake,
            fit.top_speed,
            fit.loose,
            100.0 * (ratio(&fit, 0, Surface::Tarmac) / tarmac - 1.0),
            100.0 * (ratio(&fit, 1, Surface::Gravel) / gravel - 1.0),
            100.0 * (ratio(&fit, 2, Surface::Snow) / snow - 1.0),
        );
    }
}
