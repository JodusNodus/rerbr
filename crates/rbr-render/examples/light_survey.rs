//! The textures of a stage by the light its bake gives them (measurement, ours): per texture, the triangles, the draw
//! states and the mean and peak vertex luminance, brightest first. On a night stage the lamps' own faces and the
//! ground they light stand out of the dark bake.
//!
//! cargo run --release -p rbr-render --example light_survey -- MAPS track-126_O [STAGE...]

use rbr_render::stage::{self, Selection};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default)]
struct Tally {
    triangles: usize,
    vertices: f64,
    light: f64,
    peak: u8,
    blend: usize,
    cutout: usize,
    min: [f32; 3],
    max: [f32; 3],
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let maps = args.next().ok_or("MAPS")?;
    let maps = Path::new(&maps);
    let names: Vec<String> = args.collect();
    let find = std::env::var("FIND").unwrap_or_default();
    for name in names {
        let s = stage::load(maps, &name, Selection::default())?;
        let mut all: BTreeMap<String, Tally> = BTreeMap::new();
        for m in &s.meshes {
            for d in &m.draws {
                let Some(t) = d.state.textures[0] else { continue };
                let file = s.texture_names.get(&t).map_or_else(|| format!("{t:?}"), |n| n.file_name.to_lowercase());
                let tally = all.entry(file.clone()).or_insert_with(|| Tally { min: [f32::MAX; 3], max: [f32::MIN; 3], ..Tally::default() });
                if !find.is_empty() && find.split(',').any(|f| file.contains(f)) {
                    let quad: Vec<_> = m.indices[d.indices.clone()].iter().map(|&i| glam::Vec3::from(m.vertices[i as usize].position)).collect();
                    let (lo, hi) = quad.iter().fold((glam::Vec3::MAX, glam::Vec3::MIN), |(lo, hi), &p| (lo.min(p), hi.max(p)));
                    let c = (lo + hi) / 2.0;
                    let near = s.driveline.iter().enumerate().min_by(|a, b| a.1.distance(c).total_cmp(&b.1.distance(c)));
                    println!("  at {:8.1} {:6.1} {:8.1} size {:5.1} {:5.1} {:5.1} tris {:4} road {:?} {file}", c.x, c.y, c.z, hi.x - lo.x, hi.y - lo.y, hi.z - lo.z, quad.len() / 3, near.map(|(i, p)| (i, p.distance(c) as i32, p.x as i32, p.y as i32, p.z as i32)));
                }
                tally.triangles += d.indices.len() / 3;
                tally.blend += usize::from(d.state.blend);
                tally.cutout += usize::from(d.state.alpha_ref.is_some());
                for &i in &m.indices[d.indices.clone()] {
                    let v = &m.vertices[i as usize];
                    let peak = v.color[..3].iter().copied().max().unwrap_or(0);
                    tally.vertices += 1.0;
                    tally.light += f64::from(peak);
                    tally.peak = tally.peak.max(peak);
                    for k in 0..3 {
                        tally.min[k] = tally.min[k].min(v.position[k]);
                        tally.max[k] = tally.max[k].max(v.position[k]);
                    }
                }
            }
        }
        let mut rows: Vec<_> = all.into_iter().collect();
        rows.sort_by(|a, b| (b.1.light / b.1.vertices).total_cmp(&(a.1.light / a.1.vertices)));
        println!("{name}: {} textures", rows.len());
        for (file, t) in rows {
            println!(
                "  {:5.1} peak {:3} tris {:7} blend {:3} cutout {:3} span {:6.0} {:5.0} {:6.0}  {file}",
                t.light / t.vertices,
                t.peak,
                t.triangles,
                t.blend,
                t.cutout,
                t.max[0] - t.min[0],
                t.max[1] - t.min[1],
                t.max[2] - t.min[2],
            );
        }
    }
    Ok(())
}
