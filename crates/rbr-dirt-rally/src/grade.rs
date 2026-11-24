//! The area's colour grade: the day's palette (`postprocess/palettes.pssg`, the `Palette` of the day's post effect,
//! `dd_<area>_day`), a 3D look-up table of 32 x 16 x 16 `u8` BGRA texels (x green, y blue * 16 + red) that DiRT Rally
//! applies last, to the tone-mapped, display-coded frame. It is written as a 32 x 256 RGBA PNG of the same layout,
//! which the renderer reads as a 3D texture (`[ReRbrAir]` `grade`, `notes/btb.md`).
use crate::install::Route;
use crate::pssg::Pssg;

/// The table's axes (green, red, blue) and its texels' bytes.
const AXES: [usize; 3] = [32, 16, 16];
const BYTES: usize = AXES[0] * AXES[1] * AXES[2] * 4;
/// The display grey a sunlit road sits at in the frames (150 to 200 of 255) at which the table's lift is read.
const ROAD: f32 = 0.6;
/// How much light one unit of display grey is (the display's own transfer).
const DISPLAY_GAMMA: f32 = 2.2;

/// One area's grade.
#[derive(Debug, Clone, PartialEq)]
pub struct Grade {
    /// The table as a 32 x 256 RGBA PNG.
    pub png: Vec<u8>,
    /// The linear light the table adds to a sunlit road's grey ([`ROAD`]): above 1 it brightens.
    pub lift: f32,
}

/// The table's RGBA texels (R, G, B, 255; row y = blue * 16 + red) from the palette's bytes (B, G, R, A).
fn rgba(payload: &[u8]) -> Option<Vec<u8>> {
    let texels = payload.get(..BYTES)?;
    Some(texels.chunks_exact(4).flat_map(|t| [t[2], t[1], t[0], 255]).collect())
}

/// The luminance a grey `value` leaves the table at, over `value`, to the power of the display's transfer: the light it adds.
fn lift(rgba: &[u8], value: f32) -> f32 {
    let at = |x: usize, c: usize| {
        let (r, b) = (value * 15.0, value * 15.0);
        let (r, b) = (r.round() as usize, b.round() as usize);
        f32::from(rgba[((b * AXES[1] + r) * AXES[0] + x) * 4 + c]) / 255.0
    };
    let g = value * 31.0;
    let (g0, f) = (g.floor() as usize, g.fract());
    let luma = |x| 0.2126 * at(x, 0) + 0.7152 * at(x, 1) + 0.0722 * at(x, 2);
    let out = luma(g0) * (1.0 - f) + luma((g0 + 1).min(AXES[0] - 1)) * f;
    (out / value).powf(DISPLAY_GAMMA)
}

fn png(rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, AXES[0] as u32, (AXES[1] * AXES[2]) as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

impl Grade {
    /// From a palette's bytes; `None` when they are short.
    pub fn from_palette(payload: &[u8]) -> Option<Grade> {
        let table = rgba(payload)?;
        Some(Grade { png: png(&table).ok()?, lift: lift(&table, ROAD) })
    }

    /// The day variant's palette of `route`'s area, named by its post effect; `None` where the files are missing.
    pub fn day(route: &Route) -> Option<Grade> {
        let data = route.dir.ancestors().nth(5)?.join("postprocess");
        let effects = crate::binxml::parse(&std::fs::read(data.join("effects.xml")).ok()?).ok()?;
        let effect = format!("t~{}~{}~day", route.location, route.area);
        let name = crate::lighting::effect_param(&effects, &effect, "Palette", "paletteName")?.trim().to_owned();
        let palettes = Pssg::read(&data.join("palettes.pssg")).ok()?;
        let texture = palettes.find("TEXTURE").find(|t| t.id() == Some(name.as_str()))?;
        let block = texture.child("TEXTUREIMAGEBLOCK")?;
        Grade::from_palette(block.child_payload("TEXTUREIMAGEBLOCKDATA"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table that maps every colour to itself, in the palette's byte order.
    fn identity() -> Vec<u8> {
        let mut bytes = Vec::new();
        for b in 0..16 {
            for r in 0..16 {
                for g in 0..32 {
                    let v = |i: usize, n: usize| (i * 255 / (n - 1)) as u8;
                    bytes.extend([v(b, 16), v(g, 32), v(r, 16), 255]);
                }
            }
        }
        bytes
    }

    #[test]
    fn the_palette_is_read_as_rgb_by_green_then_blue_and_red() {
        let table = rgba(&identity()).unwrap();
        // Texel x = 31 (green full), y = 2 * 16 + 5: blue 2, red 5.
        let i = ((2 * 16 + 5) * 32 + 31) * 4;
        assert_eq!(&table[i..i + 4], &[(5 * 255 / 15) as u8, 255, (2 * 255 / 15) as u8, 255]);
        assert!(rgba(&identity()[..100]).is_none());
    }

    #[test]
    fn an_identity_table_adds_no_light_and_a_brighter_one_does() {
        let table = rgba(&identity()).unwrap();
        assert!((lift(&table, ROAD) - 1.0).abs() < 0.03);
        let bright: Vec<u8> =
            table.iter().enumerate().map(|(i, &v)| if i % 4 == 3 { v } else { v.saturating_add(20) }).collect();
        assert!(lift(&bright, ROAD) > 1.2);
        assert!(Grade::from_palette(&identity()).unwrap().png.starts_with(b"\x89PNG"));
    }
}
