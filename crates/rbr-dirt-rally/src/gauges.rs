//! A DiRT car's working gauges as an RBRHUD gauge pack (`rbr_formats::rbrhud`, drawn by `rbr_render::dials`).
//!
//! DiRT moves a needle mesh, scrolls strips of digits over small quads and swaps the lamps' texture halves
//! (`interior_config`'s `displays`: the tach's `int_needle_tach`, the speed and gear `lcd_text` quads, the shift and
//! limit `indicator_light`s). The renderer has none of those animations, but it redraws a car's own textures from an
//! RBRHUD pack (needles turned, digits and lamps shown by the live values), so the pack here is made from the interior
//! file's textures: the needle cut from the dials atlas and turned about the hub, the speed digits drawn into the
//! display's box on the atlas, the gear digit onto a small texture of its own (the gear quad's), the lamps' lit halves
//! over their unlit ones, and DiRT's backlight map (`int_cme_dials_on`) as the night glow. Where the pack puts things is
//! measured on the cme's textures ([`Layout`]).
use crate::pssg::Pssg;
use crate::texture::Texture;
use rbr_formats::dds;
use std::fs;
use std::path::Path;

/// Where a car's gauges lie in its interior textures, texels, as measured on them: the hub and the angles of the tach's
/// thirteen long tick marks (a straight-line fit through their centres, 0.23 degrees at worst), the needle's rectangle
/// in the dials atlas with its pivot in it and its lean, the speed display's box.
pub struct Layout {
    pub car: &'static str,
    pub hub: [f32; 2],
    /// The needle's angle at 0 and each 1000 rpm further, degrees clockwise from up.
    pub zero_deg: f32,
    pub deg_per_krpm: f32,
    /// The needle painted in the dials atlas (`x, y, width, height`), its pivot in that rectangle and how far its tip
    /// leans from its axis, degrees clockwise.
    pub needle: [usize; 4],
    pub pivot: [f32; 2],
    pub lean: f32,
    /// The speed display's box on the dials atlas (`x0, y0, x1, y1`).
    pub display: [f32; 4],
}

/// The Escort Mk II (`cme`).
pub const CME: Layout = Layout {
    car: "cme",
    hub: [282.0, 276.0],
    zero_deg: 65.29,
    deg_per_krpm: 28.238,
    needle: [556, 228, 36, 230],
    pivot: [18.0, 186.0],
    lean: 1.42,
    display: [174.0, 351.0, 389.0, 415.0],
};

pub fn layout(car: &str) -> Option<&'static Layout> {
    [&CME].into_iter().find(|l| l.car == car)
}

/// The gear display's texture size (the gear quad has its own texture): the quad's proportions, 2.07 by 3.5 cm, the
/// font's 32 pixel cell stretched over it as DiRT does.
pub const GEAR_SIZE: [usize; 2] = [64, 112];
/// The dials atlas's texture and the lamps' file names, as the car pack writes them.
const DIALS: &str = "dr_int_dials.dds";
const GEAR: &str = "dr_int_gear.dds";
const LAMPS: &str = "dr_int_indicator.dds";
/// An LED display's red (the font's own is dim: DiRT adds it to the dash as light).
const GEAR_RED: [u8; 3] = [255, 48, 36];
/// The speed digits' colour: an LCD's, dark (DiRT's shader draws them black).
const LCD_INK: [u8; 3] = [20, 22, 18];
/// How much of DiRT's backlight the needle's sweep gets by night (its colour at this fraction over the whole face: the
/// needle moves, the map does not).
const NEEDLE_GLOW: f32 = 0.25;
/// An unlit lamp is drawn this much of its lifted atlas colour, and glows this much of its lit state's map by night:
/// the glow is the texel times the map, so a dim lamp drawn as the atlas has it glows at night nearly as a lit one.
const LAMP_OFF: f32 = 0.4;
const LAMP_GLOW: f32 = 0.6;
/// The font's cell: the digit strip's 64 x 51.2 pixels per character, and the gear font's 32 x 32.
const DIGIT: [usize; 2] = [64, 51];
const GEAR_CELL: usize = 32;

/// RGBA8 texels.
#[derive(Clone, Debug, PartialEq)]
struct Image {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Image {
    fn filled(w: usize, h: usize, texel: [u8; 4]) -> Image {
        Image { w, h, px: texel.repeat(w * h) }
    }

    fn decode(texture: &Texture) -> Image {
        Image { w: texture.width as usize, h: texture.height as usize, px: texture.rgba() }
    }

    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.w + x) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    fn set(&mut self, x: usize, y: usize, texel: [u8; 4]) {
        let i = (y * self.w + x) * 4;
        self.px[i..i + 4].copy_from_slice(&texel);
    }

    fn crop(&self, [x, y, w, h]: [usize; 4]) -> Image {
        let mut out = Image::filled(w, h, [0; 4]);
        (0..h).for_each(|j| (0..w).for_each(|i| out.set(i, j, self.at(x + i, y + j))));
        out
    }

    fn paste(&mut self, from: &Image, x: usize, y: usize) {
        (0..from.h).for_each(|j| (0..from.w).for_each(|i| self.set(x + i, y + j, from.at(i, j))));
    }

    fn map(&self, f: impl Fn(usize, usize, [u8; 4]) -> [u8; 4]) -> Image {
        let mut out = self.clone();
        (0..self.h).for_each(|y| (0..self.w).for_each(|x| out.set(x, y, f(x, y, self.at(x, y)))));
        out
    }

    /// Bilinear, texel centres on texel centres.
    fn resized(&self, w: usize, h: usize) -> Image {
        let mut out = Image::filled(w, h, [0; 4]);
        for y in 0..h {
            let v = ((y as f32 + 0.5) * self.h as f32 / h as f32 - 0.5).clamp(0.0, (self.h - 1) as f32);
            let (y0, fy) = (v.floor() as usize, v.fract());
            for x in 0..w {
                let u = ((x as f32 + 0.5) * self.w as f32 / w as f32 - 0.5).clamp(0.0, (self.w - 1) as f32);
                let (x0, fx) = (u.floor() as usize, u.fract());
                let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
                let texel = std::array::from_fn(|c| {
                    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
                    let at = |x, y| f32::from(self.at(x, y)[c]);
                    lerp(lerp(at(x0, y0), at(x1, y0), fx), lerp(at(x0, y1), at(x1, y1), fx), fy).round() as u8
                });
                out.set(x, y, texel);
            }
        }
        out
    }

    fn dds(&self) -> Vec<u8> {
        dds::rgba8_file(self.w as u32, self.h as u32, &self.px)
    }
}

/// The font's glyphs the gear display shows, as the sprites' names and drawings: `N` and `1` to `9` from the file's
/// `translate="0123456789"` cells (the first is the neutral's `N`), and `R`, a reverse the font has no glyph for, made
/// from the lower left of its `6`. `font` is the file's 32 x 320 texels, B, G, R, A, rows from the bottom of the
/// image. The glyphs are red on black; the sprite takes the red as its alpha, stretched to the display.
fn gear_glyphs(font: &[u8]) -> Result<Vec<(String, Image)>, String> {
    if font.len() < GEAR_CELL * GEAR_CELL * 10 * 4 {
        return Err("the gear font is short".into());
    }
    let cell = |i: usize| -> Image {
        let mut image = Image::filled(GEAR_CELL, GEAR_CELL, [0; 4]);
        for y in 0..GEAR_CELL {
            // The file's rows run from the bottom of the strip: cell `i`'s top row is the file's row `319 - 32 i`.
            let row = (10 - i) * GEAR_CELL - 1 - y;
            for x in 0..GEAR_CELL {
                let red = font[(row * GEAR_CELL + x) * 4 + 2];
                image.set(x, y, [0, 0, 0, (u32::from(red) * 255 / 153).min(255) as u8]);
            }
        }
        image
    };
    let reverse = cell(6).map(|x, y, t| if (14..=25).contains(&y) && (3..=13).contains(&x) { t } else { [0; 4] });
    let stretch = |glyph: &Image| {
        glyph.resized(GEAR_SIZE[0], GEAR_SIZE[1]).map(|_, _, t| {
            // The stretch softens the strokes; a steeper edge keeps them lines.
            let alpha = ((f32::from(t[3]) / 255.0 - 0.2) / 0.6).clamp(0.0, 1.0);
            [GEAR_RED[0], GEAR_RED[1], GEAR_RED[2], (alpha * 255.0).round() as u8]
        })
    };
    let mut out = vec![("GearN".to_owned(), stretch(&cell(0)))];
    out.extend((1..=9).map(|i| (format!("Gear{i}"), stretch(&cell(i)))));
    out.push(("GearR".to_owned(), stretch(&reverse)));
    Ok(out)
}

/// What the pack is made from: the interior file's textures and config, and the font of the gear display.
pub struct Sources<'a> {
    pub dials: &'a Texture,
    pub dials_on: &'a Texture,
    pub indicator: &'a Texture,
    pub indicator_on: &'a Texture,
    pub digits: &'a Texture,
    pub gear_font: &'a [u8],
    pub config: &'a str,
    /// The tone curve the dials atlas was written with (the needle, cut from the original, takes it too).
    pub tone: fn(f32) -> f32,
}

/// The value of attribute `name` of the first element of `config` that has `marker` (`value="gear"`).
fn attribute<'a>(config: &'a str, marker: &str, name: &str) -> Option<&'a str> {
    let element = config.split('<').find(|e| e.contains(marker))?;
    let rest = element.split(&format!("{name}=\"")).nth(1)?;
    rest.split('"').next()
}

/// A display of the config's `displays` as a number: the tach's top value (thousands of rpm), the lamps' thresholds.
fn number(config: &str, marker: &str, name: &str) -> Option<f32> {
    attribute(config, marker, name)?.trim().parse().ok()
}

/// The font of the gear display: the config's `gear` display names it (`int_gear`), and the shared fonts file
/// (`simplefonts/simple_fonts.pssg`, under `data`) holds its 32 x 320 texels, B, G, R, A.
pub fn gear_font(data: &Path, config: &str) -> Result<Vec<u8>, String> {
    let font = attribute(config, "value=\"gear\"", "font").ok_or("the interior config has no gear display")?;
    let pssg = Pssg::read(&data.join("simplefonts/simple_fonts.pssg"))?;
    let name = format!("{font}.dds");
    let texture =
        pssg.find("TEXTURE").find(|n| n.id() == Some(name.as_str())).ok_or(format!("no font texture {name}"))?;
    Ok(texture.child("TEXTUREIMAGE").map(|image| image.payload().to_vec()).unwrap_or_default())
}

/// The sprite sheet: the speed digits as a `simplefont` strip (`rbrhud::FONT_CHARS` is its order of 13 cells, the
/// last three empty), then the needle, the gear glyphs and the lamps' lit and dimmed halves. Its `.ini` names them.
struct Atlas {
    image: Image,
    ini: String,
}

fn atlas(layout: &Layout, sources: &Sources) -> Result<Atlas, String> {
    let digits = Image::decode(sources.digits);
    let glyphs = gear_glyphs(sources.gear_font)?;
    let glyph = |i: usize| digits.crop([0, (i as f32 * 51.2).round() as usize, DIGIT[0], DIGIT[1]]);
    let ink = |t: [u8; 4]| [LCD_INK[0], LCD_INK[1], LCD_INK[2], t[3]];
    let mut image = Image::filled(13 * DIGIT[0], 288, [0; 4]);
    (0..10).for_each(|i| image.paste(&glyph(i).map(|_, _, t| ink(t)), i * DIGIT[0], 0));
    let mut objects = Vec::new();
    let mut place = |name: &str, sprite: &Image, x: usize, y: usize| {
        image.paste(sprite, x, y);
        objects.push(format!("{name}.png={x},{y},{},{}", sprite.w, sprite.h));
    };
    // The needle: the atlas's painted one, its alpha from how much brighter it is than the face.
    let dials = Image::decode(sources.dials);
    let toned = |c: u8| ((sources.tone)(f32::from(c) / 255.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    let needle = dials.crop(layout.needle).map(|_, _, t| {
        let light = (f32::from(t[0]) + f32::from(t[1]) + f32::from(t[2])) / 3.0;
        [toned(t[0]), toned(t[1]), toned(t[2]), (((light - 60.0) / 50.0).clamp(0.0, 1.0) * 255.0).round() as u8]
    });
    place("Needle", &needle, 0, 56);
    for (i, (name, sprite)) in glyphs.iter().enumerate() {
        place(name, sprite, 48 + i * 68, 56);
    }
    // The lamps: the indicator texture's right column is the lit state (its diffuse and its emissive map add), the left
    // column the unlit one; red on top (the limit), amber below (the shift).
    let (lamps, lit) = (Image::decode(sources.indicator), Image::decode(sources.indicator_on));
    let half = lamps.w / 2;
    let lit_cell = |row: usize| {
        let at = [half, row * lamps.h / 2, half, lamps.h / 2];
        let (diffuse, emissive) = (lamps.crop(at), lit.crop(at));
        let mut cell = diffuse;
        cell.px.iter_mut().zip(&emissive.px).for_each(|(c, e)| *c = c.saturating_add(*e));
        cell.map(|_, _, t| [t[0], t[1], t[2], 255])
    };
    let unlit_cell = |row: usize| {
        let cell = lamps.crop([0, row * lamps.h / 2, half, lamps.h / 2]);
        let dim = |c: u8| ((sources.tone)(f32::from(c) / 255.0) * LAMP_OFF * 255.0).round().clamp(0.0, 255.0) as u8;
        cell.map(|_, _, t| [dim(t[0]), dim(t[1]), dim(t[2]), 255])
    };
    place("LimitOn", &lit_cell(0), 48, 176);
    place("ShiftOn", &lit_cell(1), 116, 176);
    place("LimitOff", &unlit_cell(0), 184, 176);
    place("ShiftOff", &unlit_cell(1), 252, 176);
    let ini = format!(
        "[MAIN]\ntexture=textures/atlas.dds\n[Objects]\n{}\n[Digits.png]\nsimplefont=0,0,{},{}\n",
        objects.join("\n"),
        13 * DIGIT[0],
        DIGIT[1]
    );
    Ok(Atlas { image, ini })
}

/// The dials atlas's night glow map (`emissive_mask`): DiRT's backlight map at the atlas's size, with the needle's
/// sweep lit a little (the needle moves where the map does not).
fn dials_glow(layout: &Layout, sources: &Sources) -> Image {
    let dials = Image::decode(sources.dials);
    let map = Image::decode(sources.dials_on);
    let map = if map.w == dials.w { map } else { map.resized(dials.w, dials.h) };
    // The colour DiRT lights the needle with: the map's, where the needle is painted.
    let [nx, ny, nw, nh] = layout.needle.map(|v| v * map.w / dials.w);
    let lit: Vec<[u8; 4]> =
        (0..nh).flat_map(|j| (0..nw).map(move |i| (i, j))).map(|(i, j)| map.at(nx + i, ny + j)).collect();
    let bright: Vec<&[u8; 4]> = lit.iter().filter(|t| t[0].max(t[1]).max(t[2]) > 120).collect();
    let mean = |c: usize| bright.iter().map(|t| u32::from(t[c])).sum::<u32>() as f32 / bright.len().max(1) as f32;
    let glow = [mean(0), mean(1), mean(2)].map(|v| (v * NEEDLE_GLOW).round() as u8);
    let reach = layout.pivot[1] * map.w as f32 / dials.w as f32;
    let [hx, hy] = layout.hub.map(|v| v * map.w as f32 / dials.w as f32);
    map.map(|x, y, t| {
        let inside = ((x as f32 + 0.5 - hx).powi(2) + (y as f32 + 0.5 - hy).powi(2)).sqrt() <= reach;
        if inside {
            [t[0].max(glow[0]), t[1].max(glow[1]), t[2].max(glow[2]), 0]
        } else {
            [t[0], t[1], t[2], 0]
        }
    })
}

/// The lamps' night glow map: the lit half's map, scaled, on both halves (the lit sprite is drawn over the unlit half).
fn lamps_glow(sources: &Sources) -> Image {
    let lit = Image::decode(sources.indicator_on);
    let half = lit.w / 2;
    let glow = |c: u8| (f32::from(c) * LAMP_GLOW).round() as u8;
    lit.map(|x, y, t| {
        let own = if x < half { lit.at(x + half, y) } else { t };
        [glow(own[0]), glow(own[1]), glow(own[2]), 0]
    })
}

/// The gear display's base: black, which the digit is drawn on.
pub fn gear_base() -> Vec<u8> {
    let black = Image::filled(GEAR_SIZE[0], GEAR_SIZE[1], [0, 0, 0, 255]);
    rbr_ac_tracks::btb::textures::encode(false, black.w as u32, black.h as u32, black.px)
}

/// The three `config<N>.ini`s of the pack.
fn configs(layout: &Layout, config: &str) -> [String; 3] {
    let tach_top = number(config, "value=\"tach\" node=\"int_needle_tach\"", "max_value").unwrap_or(12.0);
    let (shift, limit) = (
        number(config, "node=\"int_indicator_shift_1\"", "cond_min").unwrap_or(7.9) * 1000.0,
        number(config, "node=\"int_indicator_limit\"", "cond_min").unwrap_or(8.9) * 1000.0,
    );
    let main = |texture: &str, mask: &str| {
        format!("[main]\nauthor = rbr-dirt-rally\natlas = textures/atlas.ini\ntexture = {texture}\nemissive_mask = textures/{mask}\n")
    };
    let [x0, y0, x1, y1] = layout.display;
    let [hx, hy] = layout.hub;
    // Three digits fill the box (the strip's cells are 64 wide), the row of them in the box's middle.
    let scale = (x1 - x0 - 4.0) / (3.0 * DIGIT[0] as f32);
    let top = y0 + ((y1 - y0) - DIGIT[1] as f32 * scale) / 2.0;
    let reach = layout.pivot[1] + 40.0;
    // The needle is drawn aimed `lean` degrees less, so that its tip, not its axis, is at the tick's angle.
    let at = |krpm: f32| layout.zero_deg + layout.deg_per_krpm * krpm - layout.lean;
    let region = format!(
        "[dirtyregion1]\ntype = DirtyRegion\nleft = {}\ntop = {}\nright = {}\nbottom = {}\n",
        (hx - reach).floor(),
        (hy - reach).floor(),
        (hx + reach).ceil(),
        (hy + reach).ceil().max(y1.ceil())
    );
    let dials = format!(
        "{}{region}\n[rpm]\ntype = Rotating\ndata = RPM\nmode = 0\nxpos = {hx}\nypos = {hy}\nscale = 1.0\nrotation = 0.0\n\
         colour = ffffffff\ntexture = Needle.png\nxpivot = {}\nypivot = {}\nrotationspeed = 10.0\nspringconstant = 30.0\n\
         dampingratio = 2.0\nrotation0deg = {:.3}\nrotation0value = 0.0\nrotation1deg = {:.3}\nrotation1value = {}\n\n\
         [speed]\ntype = Number\ndata = SpeedKPH\nmode = 0\nxpos = {}\nypos = {}\nscale = {:.4}\nrotation = 0.0\n\
         colour = ffffffff\nfont = Digits.png\nformat = Whole\njustification = Right\nprefixenabled = false\n\
         suffixenabled = false\n",
        main(DIALS, "dr_int_dials_em.dds"),
        layout.pivot[0],
        layout.pivot[1],
        at(0.0),
        at(tach_top),
        tach_top * 1000.0,
        x1 - 2.0,
        top,
        scale,
    );
    // Reverse is the drive's gear 0, neutral 1, first 2.
    let mut gear = main(GEAR, "dr_int_gear_em.dds");
    let shown = std::iter::once(("GearR".to_owned(), 0))
        .chain([("GearN".to_owned(), 1)])
        .chain((1..=9).map(|g| (format!("Gear{g}"), g + 1)));
    for (sprite, equal) in shown {
        gear += &format!(
            "\n[{sprite}]\ntype = Simple\ndata = Gear\nmode = 0\nxpos = 0\nypos = 0\nscale = 1.0\nrotation = 0.0\ncolour = ffffffff\n\
             texture = {sprite}.png\nequal = {equal}\n"
        );
    }
    // A lamp over its engine speed shows its lit half, under it its dimmed unlit one.
    let lamp = |sprite: &str, y: usize, over: f32| {
        let element = |state: &str, when: &str| {
            format!(
                "\n[{sprite}{state}]\ntype = Simple\ndata = RPM\nmode = 0\nxpos = 0\nypos = {y}\nscale = 1.0\nrotation = 0.0\n\
                 colour = ffffffff\ntexture = {sprite}{state}.png\n{when} = {over}\n"
            )
        };
        element("On", "threshold") + &element("Off", "limit")
    };
    let lamps = main(LAMPS, "dr_int_indicator_em.dds") + &lamp("Shift", 64, shift) + &lamp("Limit", 0, limit);
    [dials, gear, lamps]
}

/// Writes the pack under `root/Plugins/RBRHUD/gauges/<car>/` (the folder beside `Cars/`, where the viewer reads a car's
/// gauges from): the three configs, the sprite sheet and the glow maps.
pub fn write(root: &Path, car: &str, layout: &Layout, sources: &Sources) -> Result<(), String> {
    let folder = root.join(rbr_formats::rbrhud::GAUGES).join(car);
    let textures = folder.join("textures");
    let variant = folder.join(rbr_formats::rbrhud::DEFAULT_VARIANT);
    fs::create_dir_all(&textures).and(fs::create_dir_all(&variant)).map_err(|e| e.to_string())?;
    let write = |path: std::path::PathBuf, bytes: &[u8]| {
        fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    let atlas = atlas(layout, sources)?;
    write(textures.join("atlas.ini"), atlas.ini.as_bytes())?;
    write(textures.join("atlas.dds"), &atlas.image.dds())?;
    write(textures.join("dr_int_dials_em.dds"), &dials_glow(layout, sources).dds())?;
    write(textures.join("dr_int_indicator_em.dds"), &lamps_glow(sources).dds())?;
    write(textures.join("dr_int_gear_em.dds"), &Image::filled(GEAR_SIZE[0], GEAR_SIZE[1], [255, 255, 255, 0]).dds())?;
    for (i, text) in configs(layout, sources.config).iter().enumerate() {
        write(variant.join(format!("config{}.ini", i + 1)), text.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture::Format;
    use rbr_formats::rbrhud::{Atlas as HudAtlas, Condition, Config, Kind};

    const CONFIG: &str = r#"<interior_config>
    <displays type="analog">
        <display value="tach" node="int_needle_tach" min_value="0.0" max_value="12.0" factor="0.00955" long_way="1"/>
    </displays>
    <displays type="digital">
        <display value="gear" node="int_indicator_gear" font="int_gear" num_chars="1"/>
        <display value="speed" node="int_digi_speed" font="int_digi_speed" num_chars="3" factor="3.6" />
    </displays>
    <displays type="indicator_light">
        <display value="tach" node="int_indicator_shift_1" factor="0.00955" cond_min="7.9" cond_max="15.0"/>
        <display value="tach" node="int_indicator_limit" factor="0.00955" cond_min="8.9" cond_max="15.0"/>
    </displays>
</interior_config>"#;

    /// A texture of `width` x `height` made of one DXT block repeated: black, as a block of zeros.
    fn blank(format: Format, width: u32, height: u32) -> Texture {
        let block = if format == Format::Dxt1 { 8 } else { 16 };
        let data = vec![0; (width / 4 * (height / 4)) as usize * block];
        Texture { name: String::new(), width, height, format, levels: 1, data }
    }

    fn sources<'a>(textures: &'a [Texture; 5], font: &'a [u8]) -> Sources<'a> {
        Sources {
            dials: &textures[0],
            dials_on: &textures[1],
            indicator: &textures[2],
            indicator_on: &textures[3],
            digits: &textures[4],
            gear_font: font,
            config: CONFIG,
            tone: |x| x,
        }
    }

    fn textures() -> [Texture; 5] {
        [
            blank(Format::Dxt1, 1024, 1024),
            blank(Format::Dxt1, 512, 512),
            blank(Format::Dxt1, 128, 128),
            blank(Format::Dxt1, 128, 128),
            blank(Format::Dxt5, 64, 512),
        ]
    }

    #[test]
    fn the_displays_of_the_config_are_read() {
        assert_eq!(attribute(CONFIG, "value=\"gear\"", "font"), Some("int_gear"));
        assert_eq!(number(CONFIG, "node=\"int_indicator_shift_1\"", "cond_min"), Some(7.9));
        assert_eq!(number(CONFIG, "value=\"tach\" node=\"int_needle_tach\"", "max_value"), Some(12.0));
        assert_eq!(attribute(CONFIG, "node=\"nothing\"", "cond_min"), None);
    }

    #[test]
    fn the_pack_reads_as_an_rbrhud_pack() {
        let [dials, gear, lamps] = configs(&CME, CONFIG).map(|text| Config::parse(&text).expect("a config"));
        // The needle: 0 rpm at the zero tick and 12000 at the last, `lean` degrees less, about the hub.
        let needle = dials.elements.iter().find(|e| e.name == "rpm").unwrap();
        let Kind::Rotating(rotating) = &needle.kind else { panic!("not a needle") };
        assert_eq!((needle.x, needle.y), (282.0, 276.0));
        assert_eq!(rotating.pivot, [18.0, 186.0]);
        assert_eq!(rotating.map.len(), 2);
        assert!((rotating.map[0].1 - 63.87).abs() < 1e-3 && rotating.map[1].0 == 12000.0);
        assert!((rotating.map[1].1 - rotating.map[0].1 - 12.0 * 28.238).abs() < 1e-2);
        // The speed digits: right-justified in the display's box, three cells wide.
        let speed = dials.elements.iter().find(|e| e.name == "speed").unwrap();
        let Kind::Number(number) = &speed.kind else { panic!("not a number") };
        assert_eq!((speed.data.as_deref(), number.justification.as_str()), (Some("SpeedKPH"), "Right"));
        assert!(speed.x <= CME.display[2] && speed.x - 3.0 * 64.0 * speed.scale >= CME.display[0]);
        // The gear: reverse is the drive's gear 0, neutral 1, first 2.
        let shows = |sprite: &str| gear.elements.iter().find(|e| e.texture.as_deref() == Some(sprite)).unwrap();
        assert_eq!(shows("GearR.png").condition, Condition { equal: Some(0.0), ..Default::default() });
        assert_eq!(shows("GearN.png").condition.equal, Some(1.0));
        assert_eq!(shows("Gear1.png").condition.equal, Some(2.0));
        // The lamps: lit over the config's engine speed, dimmed under it.
        let lamp = |sprite: &str| lamps.elements.iter().find(|e| e.texture.as_deref() == Some(sprite)).unwrap();
        assert_eq!(lamp("ShiftOn.png").condition.threshold, Some(7900.0));
        assert_eq!(lamp("ShiftOff.png").condition.limit, Some(7900.0));
        assert_eq!((lamp("LimitOn.png").condition.threshold, lamp("LimitOn.png").y), (Some(8900.0), 0.0));
        assert!(!lamp("ShiftOn.png").condition.holds(7000.0) && lamp("ShiftOn.png").condition.holds(8000.0));
        assert!(lamp("ShiftOff.png").condition.holds(7000.0) && !lamp("ShiftOff.png").condition.holds(8000.0));
    }

    #[test]
    fn every_sprite_a_config_names_is_in_the_sheet() {
        let textures = textures();
        let font = vec![0; GEAR_CELL * GEAR_CELL * 10 * 4];
        let sheet = atlas(&CME, &sources(&textures, &font)).unwrap();
        let parsed = HudAtlas::parse(&sheet.ini);
        for text in configs(&CME, CONFIG) {
            for element in Config::parse(&text).unwrap().elements {
                if let Some(name) = &element.texture {
                    assert!(parsed.sprite(name).is_some(), "{name}");
                }
            }
        }
        for (name, [x, y, w, h]) in &parsed.sprites {
            assert!((x + w) as usize <= sheet.image.w && (y + h) as usize <= sheet.image.h, "{name} is off the sheet");
        }
        // No two sprites share texels.
        let boxes: Vec<[u32; 4]> = parsed.sprites.iter().map(|(_, r)| *r).collect();
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                assert!(a[0] + a[2] <= b[0] || b[0] + b[2] <= a[0] || a[1] + a[3] <= b[1] || b[1] + b[3] <= a[1]);
            }
        }
        let digits = parsed.font("Digits.png").expect("the speed font");
        assert_eq!(digits.glyph('0'), Some([0, 0, 64, 51]));
        assert_eq!(digits.glyph('9'), Some([9 * 64, 0, 64, 51]));
    }

    #[test]
    fn the_gear_font_gives_neutral_the_digits_and_a_reverse() {
        // A font whose cell `i` of the flipped strip holds a red pixel at its top left: file row 319 - 32 i.
        let mut font = vec![0; GEAR_CELL * GEAR_CELL * 10 * 4];
        for i in 0..10 {
            let row = 319 - GEAR_CELL * i;
            font[(row * GEAR_CELL + 5) * 4 + 2] = 153;
        }
        let glyphs = gear_glyphs(&font).unwrap();
        let names: Vec<&str> = glyphs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["GearN", "Gear1", "Gear2", "Gear3", "Gear4", "Gear5", "Gear6", "Gear7", "Gear8", "Gear9", "GearR"]
        );
        assert!(glyphs.iter().all(|(_, g)| (g.w, g.h) == (GEAR_SIZE[0], GEAR_SIZE[1])));
        // Every digit has its pixel, stretched, in red.
        let lit = |g: &Image| {
            g.px.chunks_exact(4).filter(|t| t[3] > 0).all(|t| t[..3] == GEAR_RED)
                && g.px.chunks_exact(4).any(|t| t[3] > 0)
        };
        assert!(glyphs[..10].iter().all(|(_, g)| lit(g)));
        // The reverse takes only the lower left of the 6: its pixel is at the top, so nothing is left.
        assert!(glyphs[10].1.px.chunks_exact(4).all(|t| t[3] == 0));
        assert!(gear_glyphs(&font[..100]).is_err());
    }

    #[test]
    fn images_crop_paste_and_stretch() {
        let mut image = Image::filled(4, 4, [0, 0, 0, 255]);
        image.paste(&Image::filled(2, 2, [9, 8, 7, 6]), 1, 1);
        assert_eq!(image.at(1, 1), [9, 8, 7, 6]);
        assert_eq!(image.crop([1, 1, 2, 2]), Image::filled(2, 2, [9, 8, 7, 6]));
        assert_eq!(image.at(0, 0), [0, 0, 0, 255]);
        // A constant stays constant at any size; a ramp stays between its ends.
        assert_eq!(Image::filled(3, 3, [10, 20, 30, 40]).resized(7, 11), Image::filled(7, 11, [10, 20, 30, 40]));
        let mut ramp = Image::filled(2, 1, [0; 4]);
        ramp.set(1, 0, [200; 4]);
        let wide = ramp.resized(8, 1);
        assert!(wide.px.chunks_exact(4).map(|t| t[0]).is_sorted() && wide.at(0, 0)[0] == 0 && wide.at(7, 0)[0] == 200);
    }

    #[test]
    fn the_lamps_glow_where_the_lit_sprite_is_drawn() {
        // A white lit map: the mask is its texels scaled, and as bright on the unlit half, where the lit sprite goes.
        let mut white = blank(Format::Dxt1, 128, 128);
        white.data = [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0].repeat(32 * 32);
        let mut textures = textures();
        textures[3] = white;
        let font = vec![0; GEAR_CELL * GEAR_CELL * 10 * 4];
        let mask = lamps_glow(&sources(&textures, &font));
        let level = (255.0 * LAMP_GLOW).round() as u8;
        assert_eq!((mask.w, mask.h), (128, 128));
        assert_eq!((mask.at(6, 10), mask.at(70, 10)), ([level, level, level, 0], [level, level, level, 0]));
    }
}
