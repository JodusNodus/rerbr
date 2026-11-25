//! A DiRT car's working displays as an RBRHUD gauge pack (`rbr_formats::rbrhud`, drawn by `rbr_render::dials`).
//!
//! The interior config's `displays` name the nodes DiRT animates besides the needles (`crate::needles`): `digital`
//! quads over which it scrolls a font's strip (speed, engine speed, gear), `motec` bar graphs whose bars it shows by a
//! mask's value against the engine speed, and `indicator_light` lamps whose texture it moves to the lit half. The
//! renderer redraws textures, not UVs, and the lamps share one texture, so each display node gets a texture of its own
//! ([`Display::key`]: its faces take a material named so) which a config of the pack draws on: the digits into the
//! font's cells side by side, as the quad's UVs lay them (u across the characters, v down one cell); the face with the
//! bars lit past their engine speed; the lamp's lit half or its dimmed unlit one over the unlit half its UVs show.
use crate::car::Part;
use crate::car_pack::file;
use crate::material::Material;
use crate::needles::{attribute, displays};
use crate::texture::{Format, Texture};
use rbr_formats::dds;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The game's engine speed, rad/s, to rpm.
const RPM: f32 = 30.0 / std::f32::consts::PI;
/// A font's characters: ten cells down its strip (the digits, or the gear's `N`, 1 to 8 and `R`).
const CELLS: usize = 10;
/// A character's texels in a display's own texture.
const CELL: usize = 64;
/// The sprite sheet's width.
const SHEET: usize = 1024;
/// An unlit lamp is drawn this much of its atlas colour, and glows this much of its lit state's map by night:
/// the glow is the texel times the map, so a dim lamp drawn as the atlas has it glows at night nearly as a lit one.
const LAMP_OFF: f32 = 0.4;
const LAMP_GLOW: f32 = 0.6;

/// What a digital display shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Speed,
    Revs,
    Gear,
}

pub enum Kind {
    /// `chars` characters of `font` (the node's material's `TEmissiveMap`).
    Digits { value: Value, chars: usize, font: Texture },
    /// `face` with each bar of `bars` (the texels of one `mask` value) lit past its engine speed: the mask's `alpha`
    /// range spans the `rpm` range. `glow` is the face's backlight.
    Bars { face: Texture, bars: Texture, mask: Texture, glow: Option<Texture>, alpha: [f32; 2], rpm: [f32; 2] },
    /// `texture`'s right half (plus `lit`, its emissive map) drawn over its left one past `rpm`.
    Lamp { texture: Texture, lit: Texture, rpm: f32 },
}

/// A working display: its node in the interior scene, the material of its faces there, and what it draws.
pub struct Display {
    pub node: String,
    pub material: String,
    pub kind: Kind,
}

impl Display {
    /// The car pack's key of the display's own texture (`gauge_int_digi_speed`).
    pub fn key(&self) -> String {
        format!("gauge_{}", self.node)
    }

    /// The texture's alpha is the faces' opacity: an LCD's digits, whose font has an alpha, over the face behind.
    pub fn opacity(&self) -> bool {
        matches!(&self.kind, Kind::Digits { font, .. } if font.format == Format::Dxt5)
    }

    /// The display's own texture as the car's `.dds`: the base the pack draws on.
    pub fn texture(&self) -> Vec<u8> {
        self.base().dds()
    }

    fn base(&self) -> Image {
        match &self.kind {
            Kind::Digits { chars, .. } => {
                Image::filled(chars * CELL, CELL, if self.opacity() { [0; 4] } else { [0, 0, 0, 255] })
            }
            Kind::Bars { face, .. } => Image::decode(face),
            Kind::Lamp { texture, .. } => Image::decode(texture),
        }
    }

    /// The night glow map: the LED's whole texture (its black stays black), the bar graph's backlight, the lamp's lit
    /// map scaled on both halves (the lit sprite is drawn over the unlit half); none for an LCD's digits.
    fn glow(&self) -> Option<Image> {
        let base = self.base();
        match &self.kind {
            Kind::Digits { .. } => (!self.opacity()).then(|| Image::filled(base.w, base.h, [255, 255, 255, 0])),
            Kind::Bars { glow, .. } => glow.as_ref().map(|g| Image::decode(g).resized(base.w, base.h).opaque(0)),
            Kind::Lamp { lit, .. } => {
                let lit = Image::decode(lit).resized(base.w, base.h);
                let half = lit.w / 2;
                let glow = |c: u8| (f32::from(c) * LAMP_GLOW).round() as u8;
                Some(lit.map(|x, y, t| {
                    let own = if x < half { lit.at(x + half, y) } else { t };
                    [glow(own[0]), glow(own[1]), glow(own[2]), 0]
                }))
            }
        }
    }
}

/// The working displays of the interior `scene`, whose `config` names them, with their textures (`find`, by name in
/// the interior file). Displays whose node is not in the scene, or whose material lacks a texture they draw from, are
/// left out (most configs list digital displays the car does not have).
pub fn read(
    config: &str,
    scene: &[Part],
    materials: &BTreeMap<String, Material>,
    find: impl Fn(&str) -> Result<Texture, String>,
) -> Result<Vec<Display>, String> {
    let mut out = Vec::new();
    for kind in ["digital", "motec", "indicator_light"] {
        for element in displays(config, kind) {
            let attr = |name: &str| attribute(element, name);
            let number = |name: &str| attr(name)?.trim().parse::<f32>().ok();
            let Some(node) = attr("node") else { continue };
            let Some(material) = scene.iter().filter(|p| p.name == node).flat_map(|p| &p.instances).next() else {
                continue;
            };
            let Some(m) = materials.get(&material.material) else { continue };
            let bound = |input: &str| m.textures.get(input).map(|t| t.trim_start_matches('#'));
            let texture = |input: &str| bound(input).map(&find).transpose();
            let rpm = |value: f32| number("factor").filter(|f| *f != 0.0).map(|f| value / f * RPM);
            let display = match kind {
                "digital" => {
                    let value = match attr("value") {
                        Some("speed") => Value::Speed,
                        Some("tach") => Value::Revs,
                        Some("gear") => Value::Gear,
                        _ => continue,
                    };
                    let chars = number("num_chars").map_or(1, |n| n as usize).max(1);
                    texture("TEmissiveMap")?.map(|font| Kind::Digits { value, chars, font })
                }
                "motec" => {
                    let rpm = [rpm(number("min_value").unwrap_or(0.0)), rpm(number("max_value").unwrap_or(0.0))];
                    let alpha = [number("min_alpha").unwrap_or(0.0), number("max_alpha").unwrap_or(255.0)];
                    match (texture("TDiffuseAlphaMap")?, texture("TDiffuseAlphaMap_2")?, texture("TAlphaMaskMap")?, rpm)
                    {
                        (Some(face), Some(bars), Some(mask), [Some(low), Some(high)]) => {
                            let glow = texture("TEmissiveMap")?;
                            Some(Kind::Bars { face, bars, mask, glow, alpha, rpm: [low, high] })
                        }
                        _ => None,
                    }
                }
                _ => match (texture("TDiffuseAlphaMap")?, texture("TEmissiveMap")?, number("cond_min").and_then(rpm)) {
                    (Some(texture), Some(lit), Some(rpm)) if attr("value") == Some("tach") => {
                        Some(Kind::Lamp { texture, lit, rpm })
                    }
                    _ => None,
                },
            };
            if let Some(kind) = display {
                out.push(Display { node: node.to_owned(), material: material.material.clone(), kind });
            }
        }
    }
    Ok(out)
}

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

    /// The texels with their alpha set to `alpha`.
    fn opaque(&self, alpha: u8) -> Image {
        self.map(|_, _, t| [t[0], t[1], t[2], alpha])
    }

    /// Bilinear, texel centres on texel centres.
    fn resized(&self, w: usize, h: usize) -> Image {
        if (w, h) == (self.w, self.h) {
            return self.clone();
        }
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

/// A font's ten characters, each stretched to a [`CELL`] square (the quad's v spans one cell). An LCD's font (DXT5) is
/// black digits in its alpha, kept; an LED's (DXT1, a dim red on black, which DiRT adds to the dash as light) becomes its
/// hue at full strength with its brightness, against the font's brightest texel, as the alpha.
fn glyphs(font: &Texture) -> Vec<Image> {
    let image = Image::decode(font);
    let edge = |i: usize| (i as f32 * image.h as f32 / CELLS as f32).round() as usize;
    let cells = (0..CELLS).map(|i| image.crop([0, edge(i), image.w, edge(i + 1) - edge(i)]).resized(CELL, CELL));
    if font.format == Format::Dxt5 {
        return cells.collect();
    }
    let brightest = image.px.chunks_exact(4).map(|t| t[0].max(t[1]).max(t[2])).max().unwrap_or(0).max(1);
    let led = |t: [u8; 4]| {
        let peak = t[0].max(t[1]).max(t[2]);
        let full = |c: u8| (u32::from(c) * 255 / u32::from(peak.max(1))) as u8;
        [full(t[0]), full(t[1]), full(t[2]), (u32::from(peak) * 255 / u32::from(brightest)) as u8]
    };
    cells.map(|cell| cell.map(|_, _, t| led(t))).collect()
}

/// The sprite sheet: sprites in rows [`SHEET`] wide, named in its `.ini`.
struct Sheet {
    image: Image,
    at: [usize; 2],
    row: usize,
    ini: String,
}

impl Sheet {
    fn new() -> Sheet {
        Sheet { image: Image::filled(SHEET, 0, [0; 4]), at: [0, 0], row: 0, ini: String::new() }
    }

    /// Puts `image` on the sheet: its rect, `x, y, w, h`.
    fn place(&mut self, image: &Image) -> [usize; 4] {
        if self.at[0] + image.w > SHEET {
            self.at = [0, self.at[1] + self.row];
            self.row = 0;
        }
        let [x, y] = self.at;
        let h = (y + image.h).max(self.image.h).next_multiple_of(4);
        self.image.px.resize(SHEET * h * 4, 0);
        self.image.h = h;
        self.image.paste(image, x, y);
        self.at[0] += image.w;
        self.row = self.row.max(image.h);
        [x, y, image.w, image.h]
    }

    fn sprite(&mut self, name: &str, image: &Image) {
        let [x, y, w, h] = self.place(image);
        self.ini += &format!("{name}.png={x},{y},{w},{h}\n");
    }

    /// A `simplefont` strip: `rbrhud::FONT_CHARS`' 13 cells, the digits then three empty ones.
    fn font(&mut self, name: &str, digits: &[Image]) -> String {
        let mut strip = Image::filled(13 * CELL, CELL, [0; 4]);
        digits.iter().enumerate().for_each(|(i, glyph)| strip.paste(glyph, i * CELL, 0));
        let [x, y, w, h] = self.place(&strip);
        format!("[{name}.png]\nsimplefont={x},{y},{w},{h}\n")
    }
}

/// An element of a config.
fn element(name: &str, data: &str, [x, y]: [usize; 2], what: &str) -> String {
    format!("\n[{name}]\ntype = Simple\ndata = {data}\nmode = 0\nxpos = {x}\nypos = {y}\nscale = 1.0\nrotation = 0.0\ncolour = ffffffff\n{what}")
}

/// The display's config, its sprites put on `sheet`.
fn config(display: &Display, sheet: &mut Sheet, fonts: &mut String) -> String {
    let key = display.key();
    let node = &display.node;
    let mut text = format!("[main]\nauthor = rbr-dirt-rally\natlas = textures/atlas.ini\ntexture = {}\n", file(&key));
    if display.glow().is_some() {
        text += &format!("emissive_mask = textures/{}\n", glow_file(&key));
    }
    match &display.kind {
        Kind::Digits { value: Value::Gear, font, .. } => {
            // Reverse is the drive's gear 0, neutral 1, first 2; the font's cells are N, 1 to 8 and R.
            let glyphs = glyphs(font);
            let shown = [(0, CELLS - 1), (1, 0)].into_iter().chain((1..CELLS - 1).map(|g| (g + 1, g)));
            for (gear, cell) in shown {
                let name = format!("{node}_{gear}");
                sheet.sprite(&name, &glyphs[cell]);
                text += &element(&name, "Gear", [0, 0], &format!("texture = {name}.png\nequal = {gear}\n"));
            }
        }
        Kind::Digits { value, chars, font } => {
            let name = format!("{node}_digits");
            *fonts += &sheet.font(&name, &glyphs(font));
            let data = if *value == Value::Speed { "SpeedKPH" } else { "RPM" };
            text += &format!(
                "\n[{node}]\ntype = Number\ndata = {data}\nmode = 0\nxpos = {}\nypos = 0\nscale = 1.0\nrotation = 0.0\n\
                 colour = ffffffff\nfont = {name}.png\nformat = Whole\njustification = Right\nprefixenabled = false\n\
                 suffixenabled = false\n",
                chars * CELL
            );
        }
        Kind::Bars { bars, mask, alpha, rpm, .. } => {
            for (value, [x, y, ..], sprite) in bar_sprites(&Image::decode(bars), &Image::decode(mask)) {
                let name = format!("{node}_{value}");
                sheet.sprite(&name, &sprite);
                let past = rpm[0] + (f32::from(value) - alpha[0]) / (alpha[1] - alpha[0]) * (rpm[1] - rpm[0]);
                text += &element(&name, "RPM", [x, y], &format!("texture = {name}.png\nthreshold = {past}\n"));
            }
        }
        Kind::Lamp { texture, lit, rpm } => {
            let (lamp, lit) =
                (Image::decode(texture), Image::decode(lit).resized(texture.width as usize, texture.height as usize));
            let half = [0, 0, lamp.w / 2, lamp.h];
            let right = [lamp.w / 2, 0, lamp.w / 2, lamp.h];
            let mut on = lamp.crop(right);
            on.px.iter_mut().zip(&lit.crop(right).px).for_each(|(c, e)| *c = c.saturating_add(*e));
            let dim = |c: u8| (f32::from(c) * LAMP_OFF).round() as u8;
            let off = lamp.crop(half).map(|_, _, t| [dim(t[0]), dim(t[1]), dim(t[2]), 255]);
            for (state, sprite, when) in [("on", on.opaque(255), "threshold"), ("off", off, "limit")] {
                let name = format!("{node}_{state}");
                sheet.sprite(&name, &sprite);
                text += &element(&name, "RPM", [0, 0], &format!("texture = {name}.png\n{when} = {rpm}\n"));
            }
        }
    }
    text
}

/// The bar graph's bars: each value of `mask` under 255 with its rect and the texels of `bars` it covers (the rest
/// clear). The mask is stretched to the bars' size.
fn bar_sprites(bars: &Image, mask: &Image) -> Vec<(u8, [usize; 4], Image)> {
    let mask = mask.resized(bars.w, bars.h);
    let mut rects: BTreeMap<u8, [usize; 4]> = BTreeMap::new();
    for y in 0..bars.h {
        for x in 0..bars.w {
            let value = mask.at(x, y)[0];
            if value < 255 {
                let r = rects.entry(value).or_insert([x, y, x, y]);
                *r = [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)];
            }
        }
    }
    rects
        .into_iter()
        .map(|(value, [x0, y0, x1, y1])| {
            let rect = [x0, y0, x1 - x0 + 1, y1 - y0 + 1];
            let sprite = bars.crop(rect).map(|i, j, t| {
                let own = mask.at(x0 + i, y0 + j)[0] == value;
                [t[0], t[1], t[2], if own { 255 } else { 0 }]
            });
            (value, rect, sprite)
        })
        .collect()
}

/// The file of a display's night glow map.
fn glow_file(key: &str) -> String {
    file(&format!("{key}_em"))
}

/// Writes the pack of `displays` under `root/Plugins/RBRHUD/gauges/<car>/` (the folder beside `Cars/`, where the
/// viewer reads a car's gauges from): a config per display, the sprite sheet and the glow maps.
pub fn write(root: &Path, car: &str, displays: &[Display]) -> Result<(), String> {
    let folder = root.join(rbr_formats::rbrhud::GAUGES).join(car);
    let textures = folder.join("textures");
    let variant = folder.join(rbr_formats::rbrhud::DEFAULT_VARIANT);
    if displays.is_empty() && !variant.exists() {
        return Ok(());
    }
    fs::create_dir_all(&textures).and(fs::create_dir_all(&variant)).map_err(|e| e.to_string())?;
    // An earlier conversion's configs, which may name textures the car no longer has.
    for entry in fs::read_dir(&variant).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with("config") && name.ends_with(".ini") {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    if displays.is_empty() {
        return Ok(());
    }
    let write = |path: PathBuf, bytes: &[u8]| fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()));
    let (mut sheet, mut fonts) = (Sheet::new(), String::new());
    for (i, display) in displays.iter().enumerate() {
        write(variant.join(format!("config{}.ini", i + 1)), config(display, &mut sheet, &mut fonts).as_bytes())?;
        if let Some(glow) = display.glow() {
            write(textures.join(glow_file(&display.key())), &glow.dds())?;
        }
    }
    let ini = format!("[MAIN]\ntexture=textures/atlas.dds\n[Objects]\n{}{fonts}", sheet.ini);
    write(textures.join("atlas.ini"), ini.as_bytes())?;
    write(textures.join("atlas.dds"), &sheet.image.dds())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::car::Instance;
    use rbr_formats::rbrhud::{Atlas as HudAtlas, Config, Kind as HudKind};

    const CONFIG: &str = r#"<interior_config>
    <displays type="digital">
        <display value="tach" node="int_digi_revs" font="int_digi_speed" num_chars="4" factor="9.550" />
        <display value="speed" node="int_digi_speed" font="int_digi_speed" num_chars="3" factor="3.6" />
        <display value="gear" node="int_indicator_gear" font="int_gear" num_chars="1" />
    </displays>
    <displays type="motec">
        <display value="tach" node="int_digi_tach" factor="0.00955" min_alpha="198" max_alpha="239" min_value="0.0" max_value="11.0"/>
    </displays>
    <displays type="indicator_light">
        <display value="tach" node="int_indicator_limit" factor="0.00955" cond_min="8.9" cond_max="15.0"/>
    </displays>
</interior_config>"#;

    /// A texture of `width` x `height` made of one DXT block repeated: black, as a block of zeros.
    fn blank(format: Format, width: u32, height: u32) -> Texture {
        let size = match format {
            Format::Dxt1 => width * height / 2,
            Format::Dxt5 | Format::U8 => width * height,
        };
        Texture { name: String::new(), width, height, format, levels: 1, data: vec![0; size as usize] }
    }

    fn part(node: &str, material: &str) -> Part {
        let instance = Instance {
            material: material.into(),
            joint: None,
            source: String::new(),
            positions: Vec::new(),
            normals: Vec::new(),
            uv: Vec::new(),
            uv2: Vec::new(),
            triangles: Vec::new(),
        };
        Part { name: node.into(), transform: crate::mesh::IDENTITY, instances: vec![instance] }
    }

    fn material(id: &str, textures: &[(&str, &str)]) -> (String, Material) {
        let textures = textures.iter().map(|(k, v)| (k.to_string(), format!("#{v}"))).collect();
        (id.into(), Material { id: id.into(), shader: String::new(), textures, constants: BTreeMap::new() })
    }

    /// The scene of a 306-like dash (no `int_digi_revs` node) and its displays' textures: a mask whose values 200 and
    /// 230 are two bars.
    fn displays() -> Vec<Display> {
        let scene = [
            part("int_digi_speed", "digital speedometer"),
            part("int_indicator_gear", "gear indicator"),
            part("int_digi_tach", "digital tachometer"),
            part("int_indicator_limit", "limit lights"),
        ];
        let materials = BTreeMap::from([
            material("digital speedometer", &[("TEmissiveMap", "int_digi_speed.tga")]),
            material("gear indicator", &[("TEmissiveMap", "int_gear.tga")]),
            material(
                "digital tachometer",
                &[
                    ("TDiffuseAlphaMap", "int_digi_tach.tga"),
                    ("TDiffuseAlphaMap_2", "int_digi_tach_bars.tga"),
                    ("TAlphaMaskMap", "int_digi_tach_bars_alpha_mask.tga"),
                    ("TEmissiveMap", "int_digi_tach_on.tga"),
                ],
            ),
            material(
                "limit lights",
                &[("TDiffuseAlphaMap", "int_indicator.tga"), ("TEmissiveMap", "int_indicator_on.tga")],
            ),
        ]);
        let find = |name: &str| {
            Ok(match name {
                "int_digi_speed.tga" => blank(Format::Dxt5, 64, 512),
                "int_gear.tga" => blank(Format::Dxt1, 64, 512),
                "int_digi_tach_bars_alpha_mask.tga" => {
                    let mut mask = blank(Format::U8, 16, 8);
                    mask.data.fill(255);
                    mask.data[2 * 16 + 3..2 * 16 + 6].fill(200);
                    mask.data[5 * 16 + 10] = 230;
                    mask
                }
                "int_digi_tach_on.tga" => blank(Format::Dxt1, 8, 4),
                "int_indicator.tga" | "int_indicator_on.tga" => blank(Format::Dxt1, 16, 16),
                _ => blank(Format::Dxt1, 16, 8),
            })
        };
        read(CONFIG, &scene, &materials, find).unwrap()
    }

    #[test]
    fn the_displays_on_nodes_of_the_scene_are_read() {
        let displays = displays();
        let nodes: Vec<&str> = displays.iter().map(|d| d.node.as_str()).collect();
        // `int_digi_revs` has no node in the scene.
        assert_eq!(nodes, ["int_digi_speed", "int_indicator_gear", "int_digi_tach", "int_indicator_limit"]);
        assert!(matches!(displays[0].kind, Kind::Digits { value: Value::Speed, chars: 3, .. }));
        assert!(matches!(displays[1].kind, Kind::Digits { value: Value::Gear, chars: 1, .. }));
        assert_eq!(
            (displays[2].material.as_str(), displays[2].key().as_str()),
            ("digital tachometer", "gauge_int_digi_tach")
        );
        // The game's thousands of rpm, over `factor`, are rad/s.
        let Kind::Bars { rpm, alpha, .. } = &displays[2].kind else { panic!("not bars") };
        assert!(rpm[0] == 0.0 && (rpm[1] - 11_000.0).abs() < 2.0 && *alpha == [198.0, 239.0], "{rpm:?}");
        let Kind::Lamp { rpm, .. } = &displays[3].kind else { panic!("not a lamp") };
        assert!((rpm - 8900.0).abs() < 2.0, "{rpm}");
        // Only the LCD's digits (a font with an alpha) show the face behind them.
        assert_eq!(displays.iter().map(Display::opacity).collect::<Vec<_>>(), [true, false, false, false]);
        assert_eq!(Image::decode(&blank(Format::Dxt1, 4, 4)).w, 4);
        assert_eq!(displays[0].base().w, 3 * CELL);
    }

    #[test]
    fn the_pack_reads_as_an_rbrhud_pack() {
        let displays = displays();
        let (mut sheet, mut fonts) = (Sheet::new(), String::new());
        let configs: Vec<Config> =
            displays.iter().map(|d| Config::parse(&config(d, &mut sheet, &mut fonts)).expect("a config")).collect();
        let atlas = HudAtlas::parse(&format!("[MAIN]\ntexture=textures/atlas.dds\n[Objects]\n{}{fonts}", sheet.ini));
        for config in &configs {
            for element in &config.elements {
                if let Some(name) = &element.texture {
                    let [x, y, w, h] = atlas.sprite(name).unwrap_or_else(|| panic!("{name} is not on the sheet"));
                    assert!((x + w) as usize <= sheet.image.w && (y + h) as usize <= sheet.image.h, "{name}");
                }
            }
        }
        // The speed: right-justified at the texture's right edge, in the font's digits.
        let speed = &configs[0].elements[0];
        let HudKind::Number(number) = &speed.kind else { panic!("not a number") };
        assert_eq!((speed.data.as_deref(), number.justification.as_str(), speed.x), (Some("SpeedKPH"), "Right", 192.0));
        assert_eq!(atlas.font(&number.font).and_then(|f| f.glyph('9')).map(|g| g[2..].to_vec()), Some(vec![64, 64]));
        // The gear: reverse is the drive's gear 0 (the font's last cell), neutral 1 (its first), first 2.
        let gear: Vec<(Option<&str>, Option<f32>)> =
            configs[1].elements.iter().map(|e| (e.texture.as_deref(), e.condition.equal)).collect();
        assert_eq!(gear.len(), 10);
        assert_eq!(
            &gear[..3],
            [
                (Some("int_indicator_gear_0.png"), Some(0.0)),
                (Some("int_indicator_gear_1.png"), Some(1.0)),
                (Some("int_indicator_gear_2.png"), Some(2.0))
            ]
        );
        // The bars: each at its rect, lit past its share of the range.
        let bars = &configs[2].elements;
        assert_eq!(bars.len(), 2);
        assert_eq!((bars[0].x, bars[0].y, bars[1].x, bars[1].y), (3.0, 2.0, 10.0, 5.0));
        let past = |e: &rbr_formats::rbrhud::Element| e.condition.threshold.unwrap();
        assert!((past(&bars[0]) - 2.0 / 41.0 * 11_000.0).abs() < 5.0 && past(&bars[1]) > past(&bars[0]));
        // The lamp: lit over its engine speed, dimmed under it.
        let lamp = &configs[3].elements;
        assert!(lamp[0].condition.holds(9000.0) && !lamp[0].condition.holds(8000.0));
        assert!(lamp[1].condition.holds(8000.0) && !lamp[1].condition.holds(9000.0));
        // An LCD has no glow map, the others do.
        assert_eq!(configs.iter().map(|c| c.emissive_mask.is_some()).collect::<Vec<_>>(), [false, true, true, true]);
    }

    #[test]
    fn a_bar_is_the_texels_of_one_mask_value() {
        let bars = Image::filled(4, 2, [10, 20, 30, 255]);
        let mut mask = Image::filled(4, 2, [255; 4]);
        mask.set(1, 0, [7; 4]);
        mask.set(2, 1, [7; 4]);
        let sprites = bar_sprites(&bars, &mask);
        assert_eq!(sprites.len(), 1);
        let (value, rect, sprite) = &sprites[0];
        assert_eq!((*value, *rect), (7, [1, 0, 2, 2]));
        assert_eq!((sprite.at(0, 0), sprite.at(1, 0)), ([10, 20, 30, 255], [10, 20, 30, 0]));
    }

    #[test]
    fn an_led_font_keeps_its_hue_at_full_strength() {
        // A DXT1 block of one colour: red 0x9800 (152, 0, 0) in both end points.
        let mut font = blank(Format::Dxt1, 4, 40);
        font.data = [0x00, 0x98, 0x00, 0x98, 0, 0, 0, 0].repeat(10);
        let glyphs = glyphs(&font);
        assert_eq!(glyphs.len(), CELLS);
        assert!(glyphs.iter().all(|g| (g.w, g.h) == (CELL, CELL) && g.at(5, 5) == [255, 0, 0, 255]));
    }

    #[test]
    fn the_sheet_wraps_its_rows_without_overlap() {
        let mut sheet = Sheet::new();
        let rects: Vec<[usize; 4]> = (0..5).map(|_| sheet.place(&Image::filled(400, 30, [1; 4]))).collect();
        assert_eq!(rects[..3], [[0, 0, 400, 30], [400, 0, 400, 30], [0, 30, 400, 30]]);
        assert_eq!((sheet.image.w, sheet.image.h), (SHEET, 92));
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
}
