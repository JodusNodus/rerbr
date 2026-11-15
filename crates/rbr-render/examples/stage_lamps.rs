//! The stage's lamps (`rbr_render::stage_lamps::find`): how many, and each lamp's place, the way its light falls and
//! its sprite's size.
//!
//! cargo run --release -p rbr-render --example stage_lamps -- MAPS STAGE... (a stage folder's path without extension)

use rbr_render::stage::{self, Selection};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let maps = args.next().ok_or("MAPS")?;
    let maps = Path::new(&maps);
    let verbose = std::env::var_os("VERBOSE").is_some();
    for name in args {
        let mut s = match stage::load(maps, &name, Selection::default()) {
            Ok(s) => s,
            Err(e) => {
                println!("{name}: {e}");
                continue;
            }
        };
        s.load_textures(maps)?;
        let taken = rbr_render::stage_lamps::take(&mut s);
        let lamps = taken.lamps;
        let cones = lamps.iter().filter(|l| l.axis != glam::Vec3::ZERO).count();
        println!(
            "{name}: {} lamps ({cones} cones), {} sprite textures, {} sprite triangles",
            lamps.len(),
            taken.sprites.len(),
            taken.sprites.iter().map(|s| s.corners.len() / 3).sum::<usize>()
        );
        let mut by: std::collections::BTreeMap<String, (usize, f32)> = Default::default();
        for l in &lamps {
            let name =
                s.texture_names.get(&l.texture).map_or_else(|| format!("{:?}", l.texture), |n| n.file_name.clone());
            let e = by.entry(name).or_default();
            e.0 += 1;
            e.1 = e.1.max(l.size);
        }
        for (name, (n, size)) in by {
            println!("  {n:4} {name} (largest {size:.1} m)");
        }
        if verbose {
            for l in &lamps {
                println!(
                    "  {:8.1} {:6.1} {:8.1}  axis {:5.2} {:5.2} {:5.2}  size {:4.1}",
                    l.position.x, l.position.y, l.position.z, l.axis.x, l.axis.y, l.axis.z, l.size
                );
            }
        }
    }
    Ok(())
}
