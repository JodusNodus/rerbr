//! The surface across the road: the two materials NGP's wheel probe would blend, and the second one's weight, at
//! lateral offsets from a driven path. `cargo run -p rbr-tyre-lab --example surface_across -- DATA STAGE TRACE
//! [WEAR WETNESS] [EVERY_M] [OFFSETS]` with `STAGE` a retail track (`track-11_O`) or an RSF stage's path without the
//! extension, `TRACE` an autopilot `--trace-speed` file (x, y in columns 8-9) or `driveline` (the stage's `.trk`
//! driveline points, straight between them), `EVERY_M` the spacing along the path
//! (default 2 m) and `OFFSETS` comma-separated metres to the left (default `-3,-2,-1,0,1,2,3`). Prints
//! `s offset id0 id1 weight z` per point (`-` off the mesh).
//!
//! The ground is the highest `.col` triangle under the point (within 0.5 m over the path's height plus 3 m), as
//! `rbr-render`'s particle ground finds it. Each material's id is NGP's sheet lookup
//! (`rbr_physics::ngp::material`): the run's 16 by 16 cells at the barycentric low and high nibble sums of the
//! triangle's corners over 15; the weight is attr `+6` over 31 (`notes/formats.md` "Collision"). Ours: the
//! barycentric weights in the ground plane, not the probe's ray. Values stay on stdout.

use std::collections::HashMap;

use rbr_formats::{col, data::Data, mat, stage_files, trk};
use rbr_physics::ngp::material::{cell, SIDE};

const GRID: f32 = 4.0;

struct Face {
    corners: [[f32; 3]; 3],
    /// Per material, per corner: low and high nibbles over 15.
    grid: [[[f32; 2]; 3]; 2],
    materials: [u8; 2],
    weight: [f32; 3],
}

fn barycentric(c: [[f32; 3]; 3], x: f32, y: f32) -> Option<[f32; 3]> {
    let d = (c[1][1] - c[2][1]) * (c[0][0] - c[2][0]) + (c[2][0] - c[1][0]) * (c[0][1] - c[2][1]);
    if d.abs() < 1e-9 {
        return None;
    }
    let a = ((c[1][1] - c[2][1]) * (x - c[2][0]) + (c[2][0] - c[1][0]) * (y - c[2][1])) / d;
    let b = ((c[2][1] - c[0][1]) * (x - c[2][0]) + (c[0][0] - c[2][0]) * (y - c[2][1])) / d;
    let e = 1.0 - a - b;
    (a >= -1e-4 && b >= -1e-4 && e >= -1e-4).then_some([a, b, e])
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, stage, trace, ..] = &args[..] else {
        panic!("usage: surface_across DATA STAGE TRACE [WEAR WETNESS] [EVERY_M] [OFFSETS]")
    };
    let wear = args.get(3).map_or("new", String::as_str);
    let wetness = args.get(4).map_or("dry", String::as_str);
    let every: f32 = args.get(5).map_or(2.0, |s| s.parse().expect("EVERY_M"));
    let offsets: Vec<f32> =
        args.get(6).map_or("-3,-2,-1,0,1,2,3", String::as_str).split(',').map(|s| s.parse().expect("offset")).collect();
    let data = Data::new(data, None);
    let col = col::parse(&stage_files::read(&data, stage, "col").expect("col")).expect("col parse");
    let mat = mat::parse(&stage_files::read(&data, stage, "mat").expect("mat")).expect("mat parse");
    let runs = &mat.entries[mat::select(&mat.entries, wear, wetness).expect("mat entry")].runs;

    let mut faces = Vec::new();
    let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    for c in &col.cells {
        for t in &c.tree.triangles {
            let [Some(a), Some(b), Some(d)] = t.vertices.map(|v| c.vertices.get(v as usize).copied()) else { continue };
            let corners = [a, b, d];
            let nib = |byte: u8| [f32::from(byte & 0xf) / 15.0, f32::from(byte >> 4) / 15.0];
            let face = Face {
                corners,
                grid: [std::array::from_fn(|i| nib(t.grid[i])), std::array::from_fn(|i| nib(t.grid[3 + i]))],
                materials: t.materials,
                weight: t.attr_a().0.map(|w| f32::from(w) / 31.0),
            };
            let lo = |k: usize| corners.iter().map(|p| p[k]).fold(f32::MAX, f32::min);
            let hi = |k: usize| corners.iter().map(|p| p[k]).fold(f32::MIN, f32::max);
            let (x0, x1) = ((lo(0) / GRID).floor() as i32, (hi(0) / GRID).floor() as i32);
            let (y0, y1) = ((lo(1) / GRID).floor() as i32, (hi(1) / GRID).floor() as i32);
            if x1 - x0 > 16 || y1 - y0 > 16 {
                continue;
            }
            for x in x0..=x1 {
                for y in y0..=y1 {
                    cells.entry((x, y)).or_default().push(faces.len() as u32);
                }
            }
            faces.push(face);
        }
    }

    let below = |x: f32, y: f32, z: f32| -> Option<(u32, u32, f32, f32)> {
        let mut best: Option<(f32, &Face, [f32; 3])> = None;
        for &i in cells.get(&((x / GRID).floor() as i32, (y / GRID).floor() as i32))? {
            let f = &faces[i as usize];
            let Some(w) = barycentric(f.corners, x, y) else { continue };
            let h = (0..3).map(|k| w[k] * f.corners[k][2]).sum::<f32>();
            if h <= z + 3.5 && best.is_none_or(|(bz, _, _)| h > bz) {
                best = Some((h, f, w));
            }
        }
        let (h, f, w) = best?;
        let id = |k: usize| {
            let (a, b) = (0..3).fold((0.0, 0.0), |(a, b), i| (a + w[i] * f.grid[k][i][0], b + w[i] * f.grid[k][i][1]));
            runs.get(f.materials[k] as usize).map_or(0, |r| u32::from(r.cells[(cell(b) * SIDE + cell(a)) as usize]))
        };
        Some((id(0), id(1), (0..3).map(|i| w[i] * f.weight[i]).sum(), h))
    };

    let path: Vec<[f32; 3]> = if trace == "driveline" {
        let file = trk::parse(&stage_files::read(&data, stage, "trk").expect("trk")).expect("trk parse");
        file.chunks
            .iter()
            .find_map(|c| match &c.payload {
                trk::ChunkPayload::DriveLine(d) => Some(d.records.iter().map(|r| r.vectors[0]).collect::<Vec<_>>()),
                _ => None,
            })
            .expect("driveline")
            .windows(2)
            .flat_map(|w| {
                let (a, b) = ([w[0].x, w[0].y, w[0].z], [w[1].x, w[1].y, w[1].z]);
                let n = (a[0] - b[0]).hypot(a[1] - b[1]).ceil().max(1.0) as usize;
                (0..n).map(move |k| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k as f32 / n as f32))
            })
            .collect()
    } else {
        let text = std::fs::read_to_string(trace).expect("trace");
        text.lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let v: Vec<f32> = l.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                (v.len() > 9).then(|| [v[7], v[8], v[9]])
            })
            .collect()
    };
    let mut s = 0.0f32;
    let mut next = 0.0f32;
    for p in path.windows(2) {
        let (dx, dy) = (p[1][0] - p[0][0], p[1][1] - p[0][1]);
        let step = (dx * dx + dy * dy).sqrt();
        s += step;
        if s < next || step < 1e-4 {
            continue;
        }
        next = s + every;
        // The left normal of the direction of travel.
        let (nx, ny) = (-dy / step, dx / step);
        for &o in &offsets {
            let (x, y) = (p[1][0] + o * nx, p[1][1] + o * ny);
            match below(x, y, p[1][2]) {
                Some((a, b, w, h)) => println!("{s:.1} {o} {a} {b} {w:.3} {h:.2}"),
                None => println!("{s:.1} {o} - - - -"),
            }
        }
    }
}
