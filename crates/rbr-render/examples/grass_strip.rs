//! The grass field's ground unrolled along the driveline (measurement, ours): for judging by eye where the field grows
//! grass against where the ground shows it. Each picture is a stretch of the driveline (across) by the band either
//! side of it (down, the driveline in the middle, left of the driving direction on top), seen from above: the ground's
//! texture with its vertex hue, then the same with the field's grass marked (`grass::Field::probe`): magenta where a
//! tuft grows, cyan where it would grow under other ground or an object more than 15 cm over it, dark orange where the
//! ground is grassy but too small a patch. Ticks every 10 m along the top. The third shows which texture is on top,
//! by rank in the report (`OUT_DIR/<stage>-textures.png` frames them in the same colours).
//!
//! `--truth` on a converted stage with GrassFX: the field by colour (its GrassFX left out) against the GrassFX one, the
//! stage's own answer: magenta where both grow, red where only ours does, blue where only GrassFX does; precision and
//! recall printed.
//!
//! RBR_DATA=<install> cargo run --release -p rbr-render --example grass_strip -- STAGE OUT_DIR [--count N]
//!     [--length M] [--at M,M...] [--truth]
//! STAGE as the viewer takes it (`track-41_M`, a stage folder's path); one stretch at each `--at` metre along the
//! driveline, else `--count` (6) spread over it; `--length` 150 m.

use glam::{Vec2, Vec3, Vec3Swizzles};
use rbr_formats::data::Data;
use rbr_formats::dds::{self, Format};
use rbr_render::stage::{self, Anchor, Image, Layer, Selection, TextureRef};
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

/// Metres a pixel.
const STEP: f32 = 0.1;
/// Metres either side of the driveline (the field's `BAND`).
const BAND: f32 = 20.0;
/// The ground index's cell, metres.
const CELL: f32 = 4.0;
/// The widest texture level sampled.
const WIDEST: u32 = 512;
/// Ground this far over the field's, metres, hides its grass.
const COVERED: f32 = 0.15;

struct Texels {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

impl Texels {
    fn of(bytes: &[u8]) -> Option<Texels> {
        let dds = dds::parse(bytes).ok()?;
        let levels = dds.levels();
        let level = levels.iter().find(|l| l.width <= WIDEST).or(levels.last())?;
        let (width, height) = (level.width.max(4) as usize, level.height.max(4) as usize);
        let mut rgba = vec![0; width * height * 4];
        match dds.format {
            Format::Dxt1 => texpresso::Format::Bc1.decompress(level.data, width, height, &mut rgba),
            Format::Dxt3 => texpresso::Format::Bc2.decompress(level.data, width, height, &mut rgba),
            Format::Dxt5 => texpresso::Format::Bc3.decompress(level.data, width, height, &mut rgba),
            Format::Rgba8 => rgba = level.data.to_vec(),
            Format::Bc7 => return None,
        }
        (rgba.len() >= width * height * 4).then_some(Texels { width, height, rgba })
    }

    fn at(&self, uv: Vec2) -> [f32; 4] {
        let x = (uv.x.rem_euclid(1.0) * self.width as f32) as usize % self.width;
        let y = (uv.y.rem_euclid(1.0) * self.height as f32) as usize % self.height;
        let i = (y * self.width + x) * 4;
        std::array::from_fn(|c| f32::from(self.rgba[i + c]) / 255.0)
    }
}

/// A world triangle seen from above: corners, texture coordinates, vertex colours, textures.
struct Tri {
    p: [Vec3; 3],
    uv0: [Vec2; 3],
    uv1: [Vec2; 3],
    color: [[u8; 4]; 3],
    first: Option<TextureRef>,
    second: Option<TextureRef>,
}

impl Tri {
    fn weights(&self, q: Vec2) -> Option<[f32; 3]> {
        let [a, b, c] = self.p.map(|p| p.xz());
        let det = (b - a).perp_dot(c - a);
        if det.abs() < 1e-8 {
            return None;
        }
        let (wb, wc) = ((q - a).perp_dot(c - a) / det, (b - a).perp_dot(q - a) / det);
        let w = [1.0 - wb - wc, wb, wc];
        w.iter().all(|&w| w >= -1e-4).then_some(w)
    }

    fn height(&self, w: [f32; 3]) -> f32 {
        self.p[0].y * w[0] + self.p[1].y * w[1] + self.p[2].y * w[2]
    }
}

fn blend<T: std::ops::Mul<f32, Output = T> + std::ops::Add<Output = T> + Copy>(c: &[T; 3], w: [f32; 3]) -> T {
    c[0] * w[0] + c[1] * w[1] + c[2] * w[2]
}

/// The ground's hue as the grass field takes it (`grass::tint`: 0.3 of the vertex colour's hue).
fn tint(c: [u8; 4]) -> Vec3 {
    let c = Vec3::new(c[0].into(), c[1].into(), c[2].into()) / 255.0;
    let most = c.max_element();
    let t = ((most - 0.02) / 0.06).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    Vec3::ONE.lerp(Vec3::ONE.lerp(c / most.max(1e-3), t), 0.3)
}

/// Precision and recall of ours against the truth from `[both, ours only, truth only]`.
fn score([both, ours, theirs]: [usize; 3]) -> String {
    let share = |a: usize, b: usize| 100.0 * a as f32 / (a + b).max(1) as f32;
    format!("precision {:.0}%, recall {:.0}%", share(both, ours), share(both, theirs))
}

/// A colour for the texture of rank `i` (the first twelve apart, the rest grey).
fn palette(i: usize) -> Vec3 {
    const COLOURS: [[f32; 3]; 12] = [
        [1.0, 0.0, 0.0],
        [0.0, 0.4, 1.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.3],
        [1.0, 0.5, 0.0],
        [0.6, 0.0, 1.0],
        [0.0, 1.0, 1.0],
        [1.0, 0.0, 0.6],
        [0.5, 0.3, 0.1],
        [1.0, 1.0, 1.0],
        [0.0, 0.0, 0.0],
        [0.4, 0.6, 0.2],
    ];
    COLOURS.get(i).map_or(Vec3::splat(0.5), |&c| Vec3::from(c))
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "grass_strip STAGE OUT_DIR [--count N] [--length M] [--at M,M...] [--truth]";
    let (name, out) = (args.next().ok_or(usage)?, PathBuf::from(args.next().ok_or(usage)?));
    let (mut count, mut length, mut at, mut truth) = (6usize, 150.0f32, Vec::new(), false);
    while let Some(arg) = args.next() {
        if arg == "--truth" {
            truth = true;
            continue;
        }
        let value = args.next().ok_or(usage)?;
        match arg.as_str() {
            "--count" => count = value.parse()?,
            "--length" => length = value.parse()?,
            "--at" => at = value.split(',').map(str::parse).collect::<Result<_, _>>()?,
            _ => return Err(usage.into()),
        }
    }
    let data = Data::from_env().ok_or("RBR_DATA")?;
    let maps = data.install().join("Maps");
    let mut s = stage::load(&maps, &name, Selection::default())?;
    s.mark_surfaces(&maps, true);
    s.load_textures(&maps)?;
    rbr_render::decals::blend_ground_decals(&mut s);
    let line = s.driveline.clone();
    if line.len() < 2 {
        return Err("no driveline".into());
    }
    // Before the field's own meshes go in.
    let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    let mut tris = Vec::new();
    let near: std::collections::HashSet<(i32, i32)> = line
        .iter()
        .flat_map(|p| {
            let c = (p.xz() / CELL).floor();
            let r = ((BAND + 10.0) / CELL).ceil() as i32;
            (-r..=r).flat_map(move |x| (-r..=r).map(move |z| (c.x as i32 + x, c.y as i32 + z)))
        })
        .collect();
    for m in s.meshes.iter().filter(|m| m.anchor == Anchor::World && m.layer == Layer::Solid) {
        for d in m.draws.iter().filter(|d| d.state.alpha_ref.is_none() && d.lod != stage::Lod::Far) {
            for t in m.indices[d.indices.clone()].chunks_exact(3) {
                let v = [0, 1, 2].map(|k| &m.vertices[t[k] as usize]);
                let p = v.map(|v| Vec3::from(v.position));
                let (low, high) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
                let (from, to) = ((low.xz() / CELL).floor(), (high.xz() / CELL).floor());
                if (to - from).max_element() > 64.0 {
                    continue;
                }
                let index = tris.len() as u32;
                let mut used = false;
                for x in from.x as i32..=to.x as i32 {
                    for z in from.y as i32..=to.y as i32 {
                        if near.contains(&(x, z)) {
                            cells.entry((x, z)).or_default().push(index);
                            used = true;
                        }
                    }
                }
                if used {
                    tris.push(Tri {
                        p,
                        uv0: v.map(|v| Vec2::from(v.uv0)),
                        uv1: v.map(|v| Vec2::from(v.uv1)),
                        color: v.map(|v| v.color),
                        first: d.state.textures[0],
                        second: d.state.textures[1].filter(|_| d.state.second_texture),
                    });
                }
            }
        }
    }
    let truth = match truth {
        true if s.grass_fx.is_some() => {
            let truth = rbr_render::grass::add(&mut s.clone(), rbr_render::grass::Level::Dense);
            s.grass_fx = None;
            truth
        }
        true => return Err("--truth: the stage has no GrassFX".into()),
        false => None,
    };
    let field =
        rbr_render::grass::add(&mut s, rbr_render::grass::Level::Dense).ok_or("no grass field (a snow stage?)")?;
    eprintln!("{name}: {} ground triangles in the field, {} seen from above", field.triangles(), tris.len());
    let mut report = field.report.clone();
    report.sort_by(|a, b| b.area.total_cmp(&a.area));
    let rank: HashMap<TextureRef, usize> = report.iter().enumerate().map(|(i, r)| (r.texture, i)).collect();
    let mut texels: HashMap<TextureRef, Option<Texels>> = HashMap::new();
    let mut texel = |t: TextureRef, uv: Vec2| {
        let t = texels.entry(t).or_insert_with(|| match s.textures.get(&t).and_then(|t| t.image.as_ref()) {
            Some(Image::Dds(bytes)) => Texels::of(bytes),
            Some(Image::Solid(c)) => Some(Texels { width: 1, height: 1, rgba: c.to_vec() }),
            _ => None,
        });
        t.as_ref().map(|t| t.at(uv))
    };

    // The driveline by distance.
    let mut along = vec![0.0f32];
    for w in line.windows(2) {
        along.push(along.last().unwrap() + w[0].xz().distance(w[1].xz()));
    }
    let total = *along.last().unwrap();
    let pose = |d: f32| {
        let i = along.partition_point(|&a| a <= d).clamp(1, line.len() - 1);
        let (a, b) = (line[i - 1].xz(), line[i].xz());
        let t = ((d - along[i - 1]) / (along[i] - along[i - 1]).max(1e-6)).clamp(0.0, 1.0);
        let forward = (b - a).normalize_or_zero();
        (a.lerp(b, t), forward)
    };
    if at.is_empty() {
        at = (0..count).map(|i| (i as f32 + 0.5) / count as f32 * total).collect();
    }
    std::fs::create_dir_all(&out)?;
    let path = PathBuf::from(&name);
    // The stage folder's name (a BTB stage's file is always `track-41_M`), else the file's.
    let folder = path.parent().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().replace(' ', "_"));
    let file: String = path.file_name().unwrap().to_string_lossy().into();
    let short = folder.filter(|f| f != "Maps").unwrap_or(file);
    let (width, rows) = ((length / STEP) as usize, (2.0 * BAND / STEP) as usize);
    let height = rows * 3 + 8;
    let (mut grows_all, mut field_all, mut band_all) = (0usize, 0usize, 0usize);
    let mut agree_all = [0usize; 3];
    for &centre in &at {
        let start = (centre - length * 0.5).clamp(0.0, (total - length).max(0.0));
        let mut rgba = vec![0u8; width * height * 4];
        let (mut grows, mut on_field) = (0usize, 0usize);
        // Both, ours only, the truth's only.
        let mut agree = [0usize; 3];
        for x in 0..width {
            let (p, forward) = pose(start + (x as f32 + 0.5) * STEP);
            let left = Vec2::new(forward.y, -forward.x);
            for y in 0..rows {
                let q = p + left * (BAND - (y as f32 + 0.5) * STEP);
                let key = ((q.x / CELL).floor() as i32, (q.y / CELL).floor() as i32);
                let top = cells.get(&key).into_iter().flatten().filter_map(|&i| {
                    let t = &tris[i as usize];
                    t.weights(q).map(|w| (t, w, t.height(w)))
                });
                let top = top.max_by(|a, b| a.2.total_cmp(&b.2));
                let mut colour = Vec3::splat(0.08);
                let mut ground_y = f32::NEG_INFINITY;
                let mut look = None;
                if let Some((t, w, h)) = top {
                    let alpha = blend(&t.color.map(|c| f32::from(c[3]) / 255.0), w);
                    look = if t.second.is_some() && alpha > 0.5 { t.second } else { t.first };
                    ground_y = h;
                    let mut c = t.first.and_then(|f| texel(f, blend(&t.uv0, w))).unwrap_or([0.5, 0.5, 0.5, 1.0]);
                    if let Some(over) = t.second.and_then(|f| texel(f, blend(&t.uv1, w))) {
                        let alpha = blend(&t.color.map(|c| f32::from(c[3]) / 255.0), w).clamp(0.0, 1.0) * over[3];
                        c = std::array::from_fn(|k| c[k] + (over[k] - c[k]) * alpha);
                    }
                    let hue = blend(&t.color.map(tint), w);
                    colour = Vec3::new(c[0], c[1], c[2]) * hue;
                }
                let ours = field.probe(q).is_some_and(|p| p.grows && ground_y <= p.height + COVERED);
                let theirs =
                    truth.as_ref().map(|t| t.probe(q).is_some_and(|p| p.grows && ground_y <= p.height + COVERED));
                let marked = match theirs {
                    Some(theirs) => {
                        if ours || theirs {
                            agree[usize::from(!theirs) + 2 * usize::from(!ours)] += 1;
                        }
                        match (ours, theirs) {
                            (true, true) => colour.lerp(Vec3::new(1.0, 0.0, 1.0), 0.6),
                            (true, false) => colour.lerp(Vec3::new(1.0, 0.0, 0.0), 0.6),
                            (false, true) => colour.lerp(Vec3::new(0.0, 0.3, 1.0), 0.6),
                            _ => colour,
                        }
                    }
                    None => match field.probe(q) {
                        Some(probe) => {
                            let hidden = ground_y > probe.height + COVERED;
                            match (probe.grows, hidden, probe.grassy >= 0.5) {
                                (true, false, _) => colour.lerp(Vec3::new(1.0, 0.0, 1.0), 0.6),
                                (true, true, _) => colour.lerp(Vec3::new(0.0, 1.0, 1.0), 0.6),
                                (false, _, true) => colour.lerp(Vec3::new(0.6, 0.3, 0.0), 0.6),
                                _ => colour,
                            }
                        }
                        None => colour,
                    },
                };
                if let Some(p) = field.probe(q) {
                    on_field += 1;
                    grows += usize::from(p.grows);
                }
                let tick = y < 6 && (start + x as f32 * STEP) % 10.0 < STEP;
                let centre_row = y == rows / 2;
                // The texture the field sees there (none: our own view's, darker).
                let seen = field.probe(q).and_then(|p| p.texture);
                let which = seen.or(look).and_then(|l| rank.get(&l)).map_or(Vec3::splat(0.3), |&r| palette(r));
                let which = if seen.is_none() { which * 0.4 } else { which };
                let which = colour.lerp(which, 0.55);
                for (panel, c) in [(0, colour), (rows + 4, marked), (2 * rows + 8, which)] {
                    let c = if tick || centre_row { Vec3::ONE } else { c };
                    let i = ((panel + y) * width + x) * 4;
                    rgba[i..i + 4].copy_from_slice(&[
                        (c.x.clamp(0.0, 1.0) * 255.0) as u8,
                        (c.y.clamp(0.0, 1.0) * 255.0) as u8,
                        (c.z.clamp(0.0, 1.0) * 255.0) as u8,
                        255,
                    ]);
                }
            }
        }
        for band in [rows, 2 * rows + 4] {
            for i in (band * width * 4..(band + 4) * width * 4).step_by(4) {
                rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let file = out.join(format!("{short}-{:05}.png", start as u32));
        rbr_render::upscale::write_png(&file, width as u32, height as u32, &rgba)?;
        println!(
            "{} {:.0}-{:.0} m: field {:.0}% of the band, grass {:.0}% of the band",
            file.display(),
            start,
            start + length,
            100.0 * on_field as f32 / (width * rows) as f32,
            100.0 * grows as f32 / (width * rows) as f32
        );
        (grows_all, field_all, band_all) = (grows_all + grows, field_all + on_field, band_all + width * rows);
        if truth.is_some() {
            println!("  against GrassFX: {}", score(agree));
            agree_all = std::array::from_fn(|k| agree_all[k] + agree[k]);
        }
    }
    println!("rank area_m2 {} green grey mean texel_cm name", rbr_render::grass::SURFACE_CLASSES.join(" "));
    for (i, r) in report.iter().enumerate().take(24) {
        let surfaces: Vec<String> = r.surfaces.iter().map(|s| format!("{:.2}", s)).collect();
        let looks = r.looks.map_or("-".into(), |l| {
            format!("{:.2} {:.2} {:.2},{:.2},{:.2}", l.green, l.grey, l.mean[0], l.mean[1], l.mean[2])
        });
        println!("{i:2} {:7.0} {} {looks} {:.1} {}", r.area, surfaces.join(" "), r.texel * 100.0, r.name);
    }
    // The textures by rank, framed in their colour in the third panel.
    let (thumb, columns) = (160usize, 8usize);
    let shown = report.len().min(24);
    let (sheet_w, sheet_h) = (columns * thumb, shown.div_ceil(columns).max(1) * thumb);
    let mut sheet = vec![0u8; sheet_w * sheet_h * 4];
    for (i, r) in report.iter().take(shown).enumerate() {
        let (ox, oy) = (i % columns * thumb, i / columns * thumb);
        for y in 0..thumb {
            for x in 0..thumb {
                let frame = x < 8 || y < 8 || x >= thumb - 8 || y >= thumb - 8;
                let c = if frame {
                    let p = palette(i);
                    [p.x, p.y, p.z, 1.0]
                } else {
                    texel(r.texture, Vec2::new(x as f32 / thumb as f32, y as f32 / thumb as f32)).unwrap_or([0.0; 4])
                };
                let o = ((oy + y) * sheet_w + ox + x) * 4;
                sheet[o..o + 4].copy_from_slice(&[0, 1, 2, 3].map(|k| (c[k].clamp(0.0, 1.0) * 255.0) as u8));
                sheet[o + 3] = 255;
            }
        }
    }
    rbr_render::upscale::write_png(&out.join(format!("{short}-textures.png")), sheet_w as u32, sheet_h as u32, &sheet)?;
    if truth.is_some() {
        println!("{short} against GrassFX: {}", score(agree_all));
    }
    println!(
        "{short}: {:.0} m driveline; field {:.0}% of the band, grass {:.0}%",
        total,
        100.0 * field_all as f32 / band_all as f32,
        100.0 * grows_all as f32 / band_all as f32
    );
    Ok(())
}
