//! Validates the generated AI tracks (`rbr_season::track`) against the install's own `ai.ini`: for every retail map the
//! generated pars, material and checks beside the retail rows. The `ai.ini` is read at run time from
//! `RBR_DATA/Root/misc.rbz` and nothing of it is kept.
//!
//! `bash -c 'source scripts/dev-env.sh; cargo run --release -p rbr-season --example track'`

use std::path::{Path, PathBuf};

use rbr_formats::ai_ini::Track;
use rbr_formats::trk::{self, DriveLineData};
use rbr_season::config::Surface;
use rbr_season::pace::{class_pace, driveline_points, path_length, timed_slice};
use rbr_season::track::{profile, PAR_SCALE};

fn read_trk(path: &Path) -> Option<DriveLineData> {
    let bytes = std::fs::read(path).ok()?;
    let file = trk::parse(&bytes).ok()?;
    file.chunks.into_iter().find_map(|c| match c.payload {
        trk::ChunkPayload::DriveLine(d) => Some(d),
        _ => None,
    })
}

fn main() {
    let data = rbr_formats::data::Data::from_env().expect("RBR_DATA");
    let (master, roster) = rbr_season::ai::load_ai(&data).expect("ai.ini");
    let root = std::env::var_os("RBR_DATA").map(PathBuf::from).unwrap();
    let class = class_pace("WRC 2.0");
    println!("map surf  retail pars (s)          generated pars (s)         total ratio | checks retail -> generated");
    let (mut ratio_by, mut type_hits, mut type_total, mut value_near, mut share_err, mut share_n) =
        (std::collections::BTreeMap::<&str, Vec<f32>>::new(), 0, 0, 0, 0.0f32, 0);
    let mut mat_hits = 0;
    let mut maps = 0;
    let mut engine: Vec<(f32, f32)> = Vec::new();
    for retail in master.tracks.iter().filter(|t| t.wet == 0) {
        let Some(path) = std::fs::read_dir(root.join("Maps"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().is_some_and(|e| e == "trk")
                    && p.file_stem().unwrap().to_string_lossy().starts_with(&format!("track-{}_", retail.stage))
            })
            .min()
        else {
            continue;
        };
        let Some(driveline) = read_trk(&path) else { continue };
        let points = driveline_points(&driveline);
        let markers = std::fs::read(path.with_extension("dls")).ok().and_then(|b| rbr_formats::markers::parse(&b).ok());
        let (start, finish, splits) = match &markers {
            Some(m) => {
                let len = path_length(&points);
                let (s, f) = (m.start().unwrap_or(0.0), m.finish().unwrap_or(len));
                let (s, f) = if 0.0 <= s && s < f { (s, f.min(len)) } else { (0.0, len) };
                (s, f, m.splits().filter(|&d| d > s && d < f).map(|d| d - s).collect::<Vec<_>>())
            }
            None => (0.0, path_length(&points), vec![]),
        };
        let points = timed_slice(&points, start, finish);
        let surface = match retail.sections[0].material {
            1 => Surface::Tarmac,
            2 => Surface::Snow,
            _ => Surface::Gravel,
        };
        let p = profile(&points, &splits, surface, &class);
        let gen: Track = p.track(retail.stage, PAR_SCALE);
        let (rt, gt): (f32, f32) =
            (retail.sections.iter().map(|s| s.par).sum(), gen.sections.iter().map(|s| s.par).sum());
        ratio_by.entry(surface.name()).or_default().push(gt / rt);
        maps += 1;
        // The generated roster's fastest driver on the generated track at full pace: its mean time over seeds against
        // the stage's reference.
        {
            let (_, base) = rbr_season::field::field(&["car".to_owned()], 12, 1.0, 7, &roster);
            let mut best = f32::MAX;
            for c in &base {
                let (mut total, mut n) = (0.0f32, 0);
                for seed in 0..40u32 {
                    let mut c = c.clone();
                    let team = roster.teams[c.team].clone();
                    let mut rng = rbr_gameplay::crt::CrtRand::srand(100 + seed);
                    let mut log = rbr_season::ai::RunLog::default();
                    if let rbr_season::field::AiResult::Time(t) =
                        rbr_season::field::drive_stage(&master, &gen, 0, 0, &mut c, &team, &mut rng, &mut log)
                    {
                        total += t;
                        n += 1;
                    }
                }
                if n > 0 {
                    best = best.min(total / n as f32);
                }
            }
            engine.push((best, p.dry));
        }
        let fmt_p = |t: &Track| t.sections.iter().map(|s| format!("{:6.1}", s.par)).collect::<Vec<_>>().join(" ");
        let fmt_c = |t: &Track| {
            t.sections
                .iter()
                .map(|s| format!("{}{}/{}{}", s.checks[0].0, s.checks[0].1, s.checks[1].0, s.checks[1].1))
                .collect::<Vec<_>>()
                .join(" ")
        };
        println!(
            "{:3} {:6} {} | {} | {:5.3} | {} -> {}  splits:{}",
            retail.stage,
            surface.name(),
            fmt_p(retail),
            fmt_p(&gen),
            gt / rt,
            fmt_c(retail),
            fmt_c(&gen),
            splits.len()
        );
        if std::env::var_os("DUMP").is_some() {
            for k in 0..3 {
                for c in 0..2 {
                    let h = p.sections[k].halves[c];
                    eprintln!(
                        "H,{},{},{},{:.3},{:.3},{},{}",
                        retail.stage,
                        surface.name(),
                        k,
                        h.tightness,
                        h.straight,
                        retail.sections[k].checks[c].0,
                        retail.sections[k].checks[c].1
                    );
                }
            }
        }
        for k in 0..3 {
            share_err += ((gen.sections[k].par / gt) - (retail.sections[k].par / rt)).abs();
            share_n += 1;
            mat_hits += u32::from(gen.sections[k].material == retail.sections[k].material);
            for c in 0..2 {
                let (r, g) = (retail.sections[k].checks[c], gen.sections[k].checks[c]);
                type_total += 1;
                type_hits += u32::from(r.0 == g.0);
                value_near += u32::from(r.0 == g.0 && r.1.abs_diff(g.1) <= 1);
            }
        }
    }
    println!("\n{maps} maps");
    for (s, v) in &ratio_by {
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let dev = (v.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
        println!("  total par, generated / retail, {s:6}: mean {mean:.3}, sd {dev:.3}, n {}", v.len());
    }
    println!("  section share of the stage, mean |generated - retail|: {:.3}", share_err / share_n as f32);
    let mean = engine.iter().map(|(t, r)| t / r).sum::<f32>() / engine.len() as f32;
    let sd = (engine.iter().map(|(t, r)| (t / r - mean).powi(2)).sum::<f32>() / engine.len() as f32).sqrt();
    println!("  fastest generated driver / reference time at PAR_SCALE {PAR_SCALE}: mean {mean:.4}, sd {sd:.4} (PAR_SCALE for 1.000: {:.4})", PAR_SCALE / mean);
    println!("  material matches: {mat_hits}/{share_n}");
    println!(
        "  check type matches: {type_hits}/{type_total}; value within 1 of a type match: {value_near}/{type_total}"
    );
}
