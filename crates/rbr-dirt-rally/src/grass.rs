//! The ground cover: `grass.grs` (where tufts grow) and `ground_cover.pssg` (what they look like), per area.
//!
//! `grass.grs` is little-endian: `7, T, N, 0` (`T` model types: 8, Lydden Hill's 6), the tuft types' parameters and model names (words 4 to 907, not read),
//! a table of the byte offsets of its `N` cells (word 909, a fixed-size block of the types' parameters and names before it) and the cells. A cell is
//! a piece of the terrain surface where grass may grow, the road cut out of it, on a 4 m grid away from the road:
//! `u16 vertices, u16 triangles, 3 zero words`, the box `[xmin ymin zmin xmax ymax zmax]` (floats, DiRT Rally's world),
//! the vertices (8 bytes: `u16` x and `u16` z normalised over the box, `u8` y over its height, then three bytes that are
//! `11 10 84` but for 7% of them, not read) and the triangles (4 bytes: three indices and a flag). The flag's bits 4 and
//! 5 are a class (0 the shoulders, 1 the main field, 2 and 3 other zones: what each means is not known); bit 3 and the
//! rest are not read. Heights match the gates' crossings to 0.25 m and the vertices lie on the 4 m lattice.
//!
//! `ground_cover.pssg` holds one quad a tuft (`RENDERNODE` `t<type>_<model>`: its size in metres, its rectangle of the
//! area's atlas `gc_billboard_d`, 2048 x 1024 DXT5, golden grass on Greece, green on Wales).
use crate::pssg::Pssg;
use crate::{mesh, texture};
use std::path::Path;

/// The word the table of cell offsets starts at, with 8 model types and with Lydden Hill's 6.
const TABLE: usize = 909;
/// A cell's header in bytes: counts, three zero words, the box.
const HEADER: usize = 40;

/// A triangle of the grass ground in DiRT Rally's plan (x, z), with its class 0 to 3.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub p: [[f32; 2]; 3],
    pub class: u8,
}

/// The ground where grass grows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Grass {
    pub triangles: Vec<Triangle>,
}

/// A cover map: a byte a texel over the rectangle, 0 where no grass grows, else the triangles' class plus 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Cover {
    pub size: [u32; 2],
    pub texels: Vec<u8>,
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .ok_or_else(|| format!("grass.grs: no word at {at}"))
}

fn f32_at(b: &[u8], at: usize) -> Result<f32, String> {
    u32_at(b, at).map(f32::from_bits)
}

impl Grass {
    /// The ground of a `grass.grs`.
    pub fn parse(bytes: &[u8]) -> Result<Grass, String> {
        if u32_at(bytes, 0)? != 7 {
            return Err("grass.grs: not version 7".into());
        }
        let cells = u32_at(bytes, 8)? as usize;
        // The table: `cells` ascending offsets, the first past the table itself, the last inside the file.
        let ascending = |at: usize| {
            let offset = |i: usize| u32_at(bytes, (at + i) * 4).ok().map(|o| o as usize);
            let (first, last) = (offset(0)?, offset(cells.checked_sub(1)?)?);
            (first >= (at + cells) * 4 && last < bytes.len() && (1..cells).all(|i| offset(i) > offset(i - 1)))
                .then_some(at)
        };
        let at = ascending(TABLE).ok_or("grass.grs: no table of cells")?;
        let table =
            (0..cells).map(|i| u32_at(bytes, (at + i) * 4).map(|o| o as usize)).collect::<Result<Vec<_>, _>>()?;
        let mut triangles = Vec::new();
        for (i, &start) in table.iter().enumerate() {
            let end = table.get(i + 1).copied().unwrap_or(bytes.len());
            let counts = u32_at(bytes, start)?;
            let (vertices, count) = ((counts & 0xffff) as usize, (counts >> 16) as usize);
            let faces = start + HEADER + vertices * 8;
            if end > bytes.len() || faces + count * 4 > end {
                return Err(format!(
                    "grass.grs: cell {i} holds {vertices} vertices and {count} triangles in {} bytes",
                    end - start
                ));
            }
            let [x0, z0, x1, z1] = [16, 24, 28, 36].map(|k| f32_at(bytes, start + k));
            let (x0, z0, x1, z1) = (x0?, z0?, x1?, z1?);
            let plan: Vec<[f32; 2]> = (0..vertices)
                .map(|v| {
                    let at = start + HEADER + v * 8;
                    let unit = |k: usize| f32::from(u16::from_le_bytes([bytes[at + k], bytes[at + k + 1]])) / 65535.;
                    [x0 + (x1 - x0) * unit(0), z0 + (z1 - z0) * unit(2)]
                })
                .collect();
            for t in bytes[faces..faces + count * 4].chunks_exact(4) {
                let corner = |k: usize| {
                    plan.get(t[k] as usize).copied().ok_or_else(|| format!("grass.grs: cell {i} index {}", t[k]))
                };
                triangles.push(Triangle { p: [corner(0)?, corner(1)?, corner(2)?], class: t[3] >> 4 & 3 });
            }
        }
        Ok(Grass { triangles })
    }

    /// The ground of the area's `grass.grs`; `None` where it has none.
    pub fn read(area: &Path) -> Result<Option<Grass>, String> {
        let path = area.join("grass.grs");
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Grass::parse(&bytes).map(Some)
    }

    /// The cover map over the stage rectangle `[xmin, ymin, xmax, ymax]` (the stage's axes about its `origin`, row 0 at
    /// `ymin`) with texels `texel` metres wide. A texel takes the class plus 1 of a triangle its centre lies in, or
    /// within [`REACH`] of (the thin triangles at the road's edge pass between centres).
    pub fn cover(&self, rect: [f64; 4], origin: [f64; 2], texel: f64) -> Cover {
        let size = [((rect[2] - rect[0]) / texel).ceil() as u32, ((rect[3] - rect[1]) / texel).ceil() as u32];
        let mut texels = vec![0u8; size[0] as usize * size[1] as usize];
        for t in &self.triangles {
            // DiRT Rally (x, z) to the stage (x, y): `frame::world`, less the origin.
            let p = t.p.map(|[x, z]| [f64::from(x) - origin[0], -f64::from(z) - origin[1]]);
            let low = |k: usize| p.iter().map(|q| q[k]).fold(f64::MAX, f64::min) - REACH;
            let high = |k: usize| p.iter().map(|q| q[k]).fold(f64::MIN, f64::max) + REACH;
            let cell =
                |v: f64, from: f64, n: u32| (((v - from) / texel).floor().max(0.) as u32).min(n.saturating_sub(1));
            let (i0, i1) = (cell(low(0), rect[0], size[0]), cell(high(0), rect[0], size[0]));
            let (j0, j1) = (cell(low(1), rect[1], size[1]), cell(high(1), rect[1], size[1]));
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let c = [rect[0] + (f64::from(i) + 0.5) * texel, rect[1] + (f64::from(j) + 0.5) * texel];
                    if inside(&p, c) {
                        texels[j as usize * size[0] as usize + i as usize] = t.class + 1;
                    }
                }
            }
        }
        Cover { size, texels }
    }
}

/// How far outside a triangle a texel centre still takes it, metres.
const REACH: f64 = 0.4;

/// Whether `c` is within [`REACH`] of the inside of triangle `p`, of either winding.
fn inside(p: &[[f64; 2]; 3], c: [f64; 2]) -> bool {
    let edge = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        ((c[0] - a[0]) * dy - (c[1] - a[1]) * dx) / dx.hypot(dy).max(1e-9)
    };
    let d = [edge(p[0], p[1]), edge(p[1], p[2]), edge(p[2], p[0])];
    let side = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
    // Distance inside each edge, positive inside, whichever way round the triangle is wound.
    d.iter().all(|&e| -(e * side.signum()) >= -REACH)
}

impl Cover {
    /// The cover map as an 8-bit greyscale PNG.
    pub fn png(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.size[0], self.size[1]);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&self.texels).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        Ok(out)
    }
}

/// A tuft model: its rectangle in the atlas's texels and its size in metres.
#[derive(Debug, Clone, PartialEq)]
pub struct Tuft {
    pub name: String,
    /// `x, y, width, height`, y down from the top.
    pub rect: [u32; 4],
    pub metres: [f32; 2],
}

/// The area's tuft models and their atlas.
#[derive(Debug, Clone, PartialEq)]
pub struct Tufts {
    /// The atlas as a DDS file, and its size in texels.
    pub atlas: Vec<u8>,
    pub size: [u32; 2],
    pub models: Vec<Tuft>,
}

/// The metres one atlas texel is drawn as at the renderer's cell size ([`Tufts::section`]'s `size`): with the grid one
/// texel a cell, a piece's `mult` is its size in cells.
const CELL: f32 = 1.0;
/// The renderer's cell in metres per square root of `size` (`grass::CELL_PER_SIZE`) and its card widening
/// (`grass::FX_WIDEN`).
const CELL_PER_SIZE: f32 = 0.09;
const WIDEN: f32 = 1.8;
/// A tuft taller or wider than this many metres grows less often, by the square of the ratio.
const COMMON: f32 = 0.8;

impl Tufts {
    /// The models of an area's `ground_cover.pssg`; `None` where it has none.
    pub fn read(area: &Path) -> Result<Option<Tufts>, String> {
        let path = area.join("ground_cover.pssg");
        if !path.is_file() {
            return Ok(None);
        }
        let pssg = Pssg::read(&path)?;
        let Some(atlas) = texture::read(&pssg).into_iter().next() else { return Ok(None) };
        let (w, h) = (atlas.width as f32, atlas.height as f32);
        let mut models = Vec::new();
        for (name, meshes) in mesh::tuft_meshes(&pssg)? {
            for m in meshes.iter().filter(|m| m.uv.len() >= 3) {
                let span = |v: &[[f32; 3]], k: usize| {
                    v.iter().map(|p| p[k]).fold(f32::MIN, f32::max) - v.iter().map(|p| p[k]).fold(f32::MAX, f32::min)
                };
                let bound = |k: usize, f: fn(f32, f32) -> f32, init: f32| m.uv.iter().map(|p| p[k]).fold(init, f);
                let (u0, u1) =
                    ((bound(0, f32::min, f32::MAX) * w).floor(), (bound(0, f32::max, f32::MIN) * w).ceil().min(w));
                let (v0, v1) =
                    ((bound(1, f32::min, f32::MAX) * h).floor(), (bound(1, f32::max, f32::MIN) * h).ceil().min(h));
                let metres = [span(&m.positions, 0), span(&m.positions, 1)];
                if u1 > u0 && v1 > v0 && metres[0] > 0. && metres[1] > 0. {
                    models.push(Tuft {
                        name: name.clone(),
                        rect: [u0 as u32, v0 as u32, (u1 - u0) as u32, (v1 - v0) as u32],
                        metres,
                    });
                    break;
                }
            }
        }
        models.sort_by(|a, b| a.name.cmp(&b.name));
        Ok((!models.is_empty()).then(|| Tufts { atlas: atlas.dds(), size: [atlas.width, atlas.height], models }))
    }

    /// The stage's `[ReRbrGrass]` section: the atlas `texture` on a grid of one texel a cell, one group of every model
    /// drawn at its own size, with no base grass, and the cover map `cover` over `rect`.
    pub fn section(&self, texture: &str, cover: &str, rect: [f64; 4]) -> String {
        // `size` makes the cell [`CELL`] metres: `CELL_PER_SIZE * sqrt(size)`.
        let size = (CELL / CELL_PER_SIZE).powi(2);
        let mut out = format!(
            "[ReRbrGrass]\ntexture={texture}\ngrid={},{}\nbase=0\nsize={size:.3}\ngroup0=1\n",
            self.size[0], self.size[1]
        );
        for (i, t) in self.models.iter().enumerate() {
            let [x, y, w, h] = t.rect;
            let chance = (COMMON / t.metres[0].max(t.metres[1])).powi(2).min(1.);
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    "group0_piece{i}={},{y},{w},{h},{chance:.3},{:.4},{:.4}\n",
                    x + 1,
                    t.metres[0] / (CELL * WIDEN),
                    t.metres[1] / CELL
                ),
            );
        }
        let _ = std::fmt::Write::write_fmt(
            &mut out,
            format_args!("cover={cover}\ncover_rect={:.1},{:.1},{:.1},{:.1}\n", rect[0], rect[1], rect[2], rect[3]),
        );
        out
    }
}

/// An area's grass: where it grows and what it looks like.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaGrass {
    pub grass: Grass,
    pub tufts: Tufts,
}

/// The area's grass, where it has both files.
pub fn read(area: &Path) -> Result<Option<AreaGrass>, String> {
    Ok(match (Grass::read(area)?, Tufts::read(area)?) {
        (Some(grass), Some(tufts)) => Some(AreaGrass { grass, tufts }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(class: u8) -> Grass {
        // DiRT Rally (x, z): the stage's y is -z.
        Grass { triangles: vec![Triangle { p: [[0., 0.], [10., 0.], [0., -10.]], class }] }
    }

    #[test]
    fn a_triangle_covers_the_texels_it_holds_with_its_class_and_none_beyond() {
        let cover = square(2).cover([-4., -4., 16., 16.], [0., 0.], 2.);
        assert_eq!(cover.size, [10, 10]);
        let at = |x: f64, y: f64| cover.texels[((y + 4.) / 2.) as usize * 10 + ((x + 4.) / 2.) as usize];
        // Inside the stage triangle (0, 0), (10, 0), (0, 10); outside past the hypotenuse and the legs.
        assert_eq!((at(1., 1.), at(2., 6.)), (3, 3));
        assert_eq!((at(9., 9.), at(-3., 1.), at(1., -3.)), (0, 0, 0));
        // Either winding, and the origin shifts it.
        let moved = square(0).cover([-4., -4., 16., 16.], [-2., 0.], 2.);
        assert_eq!(moved.texels[((1. + 4.) / 2.) as usize * 10 + ((3. + 4.) / 2.) as usize], 1);
        assert!(cover.png().unwrap().starts_with(b"\x89PNG"));
    }

    #[test]
    fn a_cell_is_read_by_its_box_and_flags() {
        let word = |b: &mut Vec<u8>, w: u32| b.extend(w.to_le_bytes());
        let mut b = Vec::new();
        for w in [7, 8, 1, 0] {
            word(&mut b, w);
        }
        b.resize(909 * 4, 0);
        word(&mut b, (909 + 1 + 3) * 4);
        for _ in 0..3 {
            word(&mut b, 0);
        }
        word(&mut b, 3 | 1 << 16);
        for _ in 0..3 {
            word(&mut b, 0);
        }
        for f in [10f32, 0., 20., 14., 1., 24.] {
            word(&mut b, f.to_bits());
        }
        for (x, z) in [(0u16, 0u16), (65535, 0), (0, 65535)] {
            b.extend(x.to_le_bytes());
            b.extend(z.to_le_bytes());
            b.extend([0, 0x11, 0x10, 0x84]);
        }
        b.extend([0, 1, 2, 0xb7]);
        let g = Grass::parse(&b).unwrap();
        assert_eq!(g.triangles, vec![Triangle { p: [[10., 20.], [14., 20.], [10., 24.]], class: 3 }]);
        assert!(Grass::parse(&b[..b.len() - 2]).is_err());
    }

    /// Over the install (`RBR_DIRT_RALLY`, the game folder): every area's `grass.grs` and `ground_cover.pssg` read, the
    /// cover lies around the gates of each route, and its tufts have rectangles in their atlas.
    #[test]
    fn the_install_grass_covers_the_ground_beside_the_road() {
        let Some(game) = std::env::var_os("RBR_DIRT_RALLY") else { return };
        let install = crate::install::Install::at(Path::new(&game)).unwrap();
        let (mut areas, mut checked) = (std::collections::BTreeSet::new(), 0);
        for route in install.routes() {
            let area = route.area_dir().to_owned();
            let Some(grass) = read(&area).unwrap_or_else(|e| panic!("{}: {e}", area.display())) else { continue };
            if areas.insert(area.clone()) {
                assert!(grass.grass.triangles.len() > 1000, "{}", area.display());
                for t in &grass.tufts.models {
                    let [x, y, w, h] = t.rect;
                    assert!(x + w <= grass.tufts.size[0] && y + h <= grass.tufts.size[1] && w > 4 && h > 4, "{t:?}");
                }
                assert!(grass.tufts.models.len() > 20, "{} models", grass.tufts.models.len());
            }
            let Ok(course) = crate::route::course(&route) else { continue };
            let gates: Vec<_> =
                course.gates.iter().map(|g| [f64::from(g.crossing[0]), -f64::from(g.crossing[2])]).collect();
            let rect = gates.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |r, g| {
                [r[0].min(g[0] - 40.), r[1].min(g[1] - 40.), r[2].max(g[0] + 40.), r[3].max(g[1] + 40.)]
            });
            let cover = grass.grass.cover(rect, [0., 0.], 2.);
            let near = |g: &[f64; 2]| {
                let (i, j) = (((g[0] - rect[0]) / 2.) as i64, ((g[1] - rect[1]) / 2.) as i64);
                (-4..=4).any(|dj| {
                    (-4..=4).any(|di| {
                        let (i, j) = (i + di, j + dj);
                        i >= 0
                            && j >= 0
                            && i < i64::from(cover.size[0])
                            && j < i64::from(cover.size[1])
                            && cover.texels[j as usize * cover.size[0] as usize + i as usize] > 0
                    })
                })
            };
            let share = gates.iter().filter(|g| near(g)).count() as f64 / gates.len() as f64;
            assert!(share > 0.5, "{}: grass within 8 m of {share:.2} of its gates", route.id());
            checked += 1;
        }
        assert!(areas.len() >= 12 && checked >= 20, "{} areas, {checked} routes", areas.len());
    }
}
