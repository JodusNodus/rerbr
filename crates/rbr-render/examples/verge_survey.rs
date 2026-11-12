//! Surveys what the verge sound (`rbr_render::verge`) can hear on each stage (measurement, ours): the ground's
//! surface ids beside the driveline and the bushes and bendable trees the `.trk` places near it.
//! Usage: `verge_survey MAPS_DIR OUT.csv`: one variant of every map under `MAPS_DIR` (one folder deep).

use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;

use glam::Vec2;
use rbr_formats::{col, mat, stage_files, trk};
use rbr_render::particles::ground::Ground;
use rbr_render::particles::wheel_spray::ground::{material, Kind};
use rbr_render::roadside::growth;

/// Sideways offsets from the driveline, m: the road, then the verge.
const OFFSETS: [f32; 4] = [0.0, 4.0, 6.0, 9.0];

fn class(id: u32) -> usize {
    if u8::try_from(id).ok().and_then(growth).is_some() {
        2
    } else if matches!(id, 0x81 | 0x82 | 0x89 | 0x8a) {
        1
    } else {
        match material(id).kind {
            Kind::Grass => 0,
            Kind::Gravel | Kind::Dirt => 3,
            Kind::Tarmac => 4,
            Kind::Snow => 5,
            _ => 6,
        }
    }
}

fn survey(stage: &str) -> Result<String, String> {
    let read = |ext: &str| {
        let bytes = std::fs::read(format!("{stage}.{ext}")).map_err(|e| format!("{ext}: {e}"))?;
        if stage_files::encrypted(&bytes) {
            Err(format!("{ext}: encrypted"))
        } else {
            Ok(bytes)
        }
    };
    let trk = trk::parse(&read("trk")?).map_err(|e| format!("trk: {e:?}"))?;
    let line: Vec<Vec2> = trk
        .chunks
        .iter()
        .find_map(|c| match &c.payload {
            trk::ChunkPayload::DriveLine(d) => Some(&d.records),
            _ => None,
        })
        .ok_or("no driveline")?
        .iter()
        .map(|r| Vec2::new(r.vectors[0].x, r.vectors[0].y))
        .collect();
    let heights: Vec<f32> = trk
        .chunks
        .iter()
        .find_map(|c| match &c.payload {
            trk::ChunkPayload::DriveLine(d) => Some(d.records.iter().map(|r| r.vectors[0].z).collect()),
            _ => None,
        })
        .unwrap_or_default();
    let col = col::parse(&read("col")?).map_err(|e| format!("col: {e:?}"))?;
    let mat = mat::parse(&read("mat")?).map_err(|e| format!("mat: {e:?}"))?;
    let ground = Ground::new(&col, &mat, "new", "dry");
    if !ground.has_surfaces() {
        return Err("no dry surfaces".into());
    }
    // counts[offset][class]
    let mut counts = [[0u32; 7]; OFFSETS.len()];
    let mut length = 0.0;
    let mut next = 0.0;
    for (k, pair) in line.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let step = (b - a).length();
        if step < 1e-3 {
            continue;
        }
        let ahead = (b - a) / step;
        let right = Vec2::new(ahead.y, -ahead.x);
        while next <= length + step {
            let at = a + ahead * (next - length);
            for (o, &offset) in OFFSETS.iter().enumerate() {
                for side in [-1.0, 1.0] {
                    let p = at + right * offset * side;
                    if let Some(hit) = ground.below([p.x, p.y, heights[k] + 30.0]) {
                        counts[o][class(hit.surface)] += 1;
                    }
                    if offset == 0.0 {
                        break;
                    }
                }
            }
            next += 5.0;
        }
        length += step;
    }
    // The `.trk`'s vegetation placements, all and within 8 m of the driveline.
    let posts = rbr_render::roadside::posts(&trk);
    let grown: Vec<_> = posts.iter().filter(|p| p.growth.is_some()).collect();
    let cell = |p: Vec2| ((p.x / 16.0).floor() as i32, (p.y / 16.0).floor() as i32);
    let mut near_line = std::collections::HashMap::<(i32, i32), Vec<Vec2>>::new();
    for p in &line {
        near_line.entry(cell(*p)).or_default().push(*p);
    }
    let close = grown
        .iter()
        .filter(|post| {
            let (cx, cy) = cell(post.at);
            (-1..=1).any(|dx| {
                (-1..=1).any(|dy| {
                    near_line.get(&(cx + dx, cy + dy)).is_some_and(|ps| ps.iter().any(|p| p.distance(post.at) < 8.0))
                })
            })
        })
        .count();
    let mut row = format!("{:.0},{},{},{}", length, posts.len(), grown.len(), close);
    for c in counts {
        for n in c {
            write!(row, ",{n}").unwrap();
        }
    }
    Ok(row)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let (maps, out) = (args.next().ok_or("MAPS_DIR")?, args.next().ok_or("OUT.csv")?);
    let mut stages = Vec::new();
    for dir in std::iter::once(Path::new(&maps).to_path_buf())
        .chain(std::fs::read_dir(&maps)?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()))
    {
        // One variant a map number: M, else N, O, E (they share the ground).
        let mut found = std::collections::BTreeMap::<String, (usize, String)>::new();
        for file in std::fs::read_dir(&dir)?.filter_map(Result::ok).map(|e| e.path()) {
            let name = file.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
            let Some(stem) = name.strip_suffix(".trk").filter(|n| n.starts_with("track-")) else { continue };
            let Some((map, letter)) = stem.rsplit_once('_') else { continue };
            let rank = "mnoe".find(letter).unwrap_or(9);
            let stage = file.with_extension("").to_string_lossy().into_owned();
            let entry = found.entry(map.to_owned()).or_insert((rank, stage.clone()));
            if rank < entry.0 {
                *entry = (rank, stage);
            }
        }
        stages.extend(found.into_values().map(|(_, stage)| stage));
    }
    stages.sort();
    let mut csv = String::from("stage,status,length,posts,grown,grown_near");
    for o in OFFSETS {
        for c in ["grass", "rough", "bush", "loose", "tarmac", "snow", "other"] {
            write!(csv, ",{c}_{o}")?;
        }
    }
    csv.push('\n');
    for (i, stage) in stages.iter().enumerate() {
        let result = std::panic::catch_unwind(|| survey(stage)).unwrap_or_else(|_| Err("panic".into()));
        match result {
            Ok(row) => writeln!(csv, "\"{stage}\",ok,{row}")?,
            Err(e) => writeln!(csv, "\"{stage}\",\"{e}\"")?,
        }
        if i % 50 == 0 {
            eprintln!("{i}/{}", stages.len());
        }
    }
    std::fs::write(out, csv)?;
    Ok(())
}
