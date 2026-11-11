//! The cut-out draws' textures of a stage (measurement, ours): per `.dds` format, how many textures and triangles, how
//! much of the alpha is neither 0 nor 255 (a smooth edge) and how many of the clear texels are black (the colour a
//! filtered edge texel is mixed with).
//!
//! cargo run --release -p rbr-render --example cutout_survey -- MAPS track-41_M [STAGE...]

use rbr_render::stage::{self, Selection};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default)]
struct Tally {
    textures: usize,
    triangles: usize,
    texels: f64,
    partial: f64,
    clear: f64,
    clear_black: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let maps = args.next().ok_or("MAPS")?;
    let maps = Path::new(&maps);
    let mut all: BTreeMap<String, Tally> = BTreeMap::new();
    for name in args {
        let mut s = stage::load(maps, &name, Selection::default())?;
        s.load_textures(maps)?;
        let mut triangles: BTreeMap<_, usize> = BTreeMap::new();
        for m in &s.meshes {
            for d in m.draws.iter().filter(|d| d.state.alpha_ref.is_some() && d.state.shade.is_none()) {
                if let Some(t) = d.state.textures[0] {
                    *triangles.entry(t).or_default() += d.indices.len() / 3;
                }
            }
        }
        for (t, count) in triangles {
            let Some(stage::Image::Dds(bytes)) = s.textures.get(&t).and_then(|t| t.image.as_ref()) else { continue };
            let Ok(dds) = rbr_formats::dds::parse(bytes) else { continue };
            let level = &dds.levels()[0];
            let rgba = rbr_render::upscale::decompress(dds.format, level.data, level.width, level.height);
            let tally = all.entry(format!("{:?}", dds.format)).or_default();
            tally.textures += 1;
            tally.triangles += count;
            for texel in rgba.chunks_exact(4) {
                tally.texels += 1.0;
                match texel[3] {
                    0 => {
                        tally.clear += 1.0;
                        tally.clear_black += f64::from(u8::from(texel[..3].iter().all(|&c| c <= 8)));
                    }
                    255 => {}
                    _ => tally.partial += 1.0,
                }
            }
        }
    }
    for (format, t) in all {
        println!(
            "{format}: {} textures, {} triangles, alpha partial {:.1}%, clear {:.1}% of which black {:.1}%",
            t.textures,
            t.triangles,
            100.0 * t.partial / t.texels.max(1.0),
            100.0 * t.clear / t.texels.max(1.0),
            100.0 * t.clear_black / t.clear.max(1.0),
        );
    }
    Ok(())
}
