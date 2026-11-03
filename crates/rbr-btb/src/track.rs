//! The BTB stage sources in one `RX_CONTENT/Tracks/<folder>/`: the typed readers.
//!
//! Each folder holds the stage's sources: `track.ini` (`[INFO]` name, physics, author, version, splashscreen, date,
//! comment, length), `spawn.ini` (`[SPAWN]` matrix and angles), `driveline.ini` (`[DRIVELINE]` count and `K<i>`
//! points), `settings.ini` (`[SETTINGS]` sun direction), `objectlist.ini` (`[OBJECTS]` count and `[Object_<i>]`
//! placements), `materials.ini` (`[MATERIALS]` count, `MAT_<i>` names and one section per material), `mat.ini`
//! (`[MAPS]` count and `[MAP<i>]` 16x16 surface grids), `pacenotes.ini` (`[PACENOTES]` count and `[P<i>]` notes),
//! plus `Objects.zip`/`Textures.zip` (the `.x` meshes: [`crate::xfile`], [`crate::xform`]). Some folders carry a `CACHE/`
//! subfolder the RSF plugin `Plugins/rbr_rx.dll` wrote: `trk.trk`, `mat.mat`, `dls.dls` and `lbs.lbs` (the physics
//! files the game loads), `col.col`, `ksh.ksh` and minimaps. The `CACHE` files are the oracle for [`crate::classic`]:
//! the builders return bytes identical to the plugin's output. See `notes/btb.md`.
//!
//! All text files are read as bytes and decoded lossily, as the game's `GetPrivateProfileStringA` reads bytes; some
//! installs carry non-UTF-8 bytes in them. Section lookup is first-match, case-insensitive, and key lookup within a
//! section is first-match, case-insensitive, matching the Windows ini calls the plugin makes (duplicate `[MAP<i>]`
//! sections occur: the first wins, as `Sagen Sweden 2012`'s cache proves). A section header only counts at the start
//! of a line: in `Kinamba Yala` and `Olocau Gatova` a `data15` line is missing its newline (`... 69[MAP1]`), so that
//! `[MAPn]` is not a section and its map reads all zeros, and the trailing `69[MAP1]` token reads 69 (`atoi`).
//!
//! Numbers with BTB's French-locale decimal comma (`LODOut=2679,576`, `SpecularColor=0,4235294,...`) parse through
//! [`decimal_floats`]; `colBox` is read as the plugin's `sscanf` reads it ([`scan_col_box`]).

use rbr_formats::{ini, stage_files};
use std::path::{Path, PathBuf};

/// One folder's read failure, kept raw where the plugin's meaning is unknown.
pub type RawEntries = Vec<(String, String)>;

/// The numbers in `s`: each `-?\d+[.,]\d+` or integer token, in order, as `f64` (`,` is BTB's decimal comma, `.` the
/// plain point). Used for `LODOut`, `SpecularColor`, `SpecularPower`, `colBox`, `sunDirection` and `ambient`.
pub fn decimal_floats(s: &str) -> Vec<f64> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'-' || c == b'+' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b',') {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 1 {
                    i = j;
                }
            }
            let token = &s[start..i];
            let value = if token.contains(['.', ',']) {
                token.replace(',', ".").parse::<f64>().unwrap_or(0.0)
            } else {
                token.parse::<f64>().unwrap_or(0.0)
            };
            out.push(value);
        } else {
            i += 1;
        }
    }
    out
}

/// The single number in `s` (`SpecularPower`): `Some` when it holds exactly one.
pub fn decimal_float(s: &str) -> Option<f64> {
    match &decimal_floats(s)[..] {
        [v] => Some(*v),
        _ => None,
    }
}

/// One `data0..data15` row's 16 cells: whitespace-separated `atoi` tokens (no leading digits read 0, as in
/// `[MAP15]`), the first 16 kept, missing tokens zero. Mirrors the plugin's row parse, quirks included.
pub fn data_cells(value: &str) -> [u8; 16] {
    let mut cells = [0u8; 16];
    for (cell, token) in cells.iter_mut().zip(value.split_whitespace()) {
        let digits: String =
            token.trim_start_matches(['+', '-']).bytes().take_while(u8::is_ascii_digit).map(char::from).collect();
        let negative = token.starts_with('-');
        let mut v: i64 = digits.parse().unwrap_or(0);
        if negative {
            v = -v;
        }
        *cell = v as u8;
    }
    cells
}

/// The first section named `name` (ASCII case-insensitive), matching the plugin's section lookup.
pub fn find_section<'a>(sections: &'a [ini::Section], name: &str) -> Option<&'a ini::Section> {
    sections.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// The first entry named `key` (ASCII case-insensitive) of `section`.
pub fn find_entry<'a>(section: &'a ini::Section, key: &str) -> Option<&'a str> {
    section.get(key)
}

/// Reads `file` from `folder` (matched ignoring case, as on Windows), decoded lossily.
fn read_ini(folder: &Path, file: &str) -> Result<Vec<ini::Section>, String> {
    let path = stage_files::in_dir(folder, file).ok_or_else(|| format!("{}: no {file}", folder.display()))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    ini::parse(&String::from_utf8_lossy(&bytes)).map_err(|e| format!("{}: {e}", path.display()))
}

/// A BTB stage folder: its path and its `track.ini`.
#[derive(Debug, Clone)]
pub struct Track {
    /// The `RX_CONTENT/Tracks/<folder>/` directory.
    pub folder: PathBuf,
    /// The folder's own name.
    pub name: String,
    /// The parsed `track.ini`.
    pub info: TrackInfo,
}

impl Track {
    /// Opens the folder's `track.ini`.
    pub fn open(folder: &Path) -> Result<Track, String> {
        let sections = read_ini(folder, "track.ini")?;
        let info = TrackInfo::parse(&sections).ok_or_else(|| format!("{}: no [INFO]", folder.display()))?;
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Track { folder: folder.to_owned(), name, info })
    }

    /// The parsed `spawn.ini`.
    pub fn spawn(&self) -> Result<Spawn, String> {
        Spawn::parse(&read_ini(&self.folder, "spawn.ini")?)
    }

    /// The parsed `driveline.ini`: the declared count and the `K<i>` points in index order.
    pub fn driveline(&self) -> Result<Driveline, String> {
        Driveline::parse(&read_ini(&self.folder, "driveline.ini")?)
    }

    /// The parsed `settings.ini`.
    pub fn settings(&self) -> Result<Settings, String> {
        Settings::parse(&read_ini(&self.folder, "settings.ini")?)
    }

    /// The parsed `objectlist.ini`: the `[Object_<i>]` placements in index order.
    pub fn objects(&self) -> Result<Vec<Object>, String> {
        Object::parse_all(&read_ini(&self.folder, "objectlist.ini")?)
    }

    /// The parsed `materials.ini`: the `MAT_<i>` names and their sections.
    pub fn materials(&self) -> Result<Materials, String> {
        Materials::parse(&read_ini(&self.folder, "materials.ini")?)
    }

    /// The parsed `mat.ini` surface grids: `[MAPS]` count maps of 256 cells, missing maps and rows zero.
    pub fn surface_maps(&self) -> Result<SurfaceMaps, String> {
        SurfaceMaps::parse(&read_ini(&self.folder, "mat.ini")?)
    }

    /// The parsed `pacenotes.ini`: the first `count` `[P<i>]` notes in file order. Surplus `[P<i>]` sections past
    /// `count` (as in `Kuri Bush 1`) are not notes; the plugin always reads `pacenotes.ini`, never the
    /// `pacenotes1_*` or `*TeamSumppu.ini` copies some folders keep (`Pirttikulma snow`'s copy holds a different
    /// count, 196 against the cache's 211).
    pub fn pace_notes(&self) -> Result<PaceNotes, String> {
        PaceNotes::parse(&read_ini(&self.folder, "pacenotes.ini")?)
    }
}

/// Whether `folder` is a BTB track folder: a `track.ini` with the `Objects.zip`, `Textures.zip` and
/// `objectlist.ini` of its scenery beside it (file names matched ignoring case).
pub fn is_track_folder(folder: &Path) -> bool {
    ["track.ini", "objectlist.ini", "Objects.zip", "Textures.zip"]
        .iter()
        .all(|file| stage_files::in_dir(folder, file).is_some())
}

/// Every `RX_CONTENT/Tracks/*` folder under the RSF install `root`: the ones with a `track.ini` open, the rest are
/// `Err` with the folder. Sorted by folder name.
pub fn scan(root: &Path) -> Vec<Result<Track, (PathBuf, String)>> {
    let tracks = stage_files::in_dir(root, "RX_CONTENT\\Tracks");
    let mut folders: Vec<PathBuf> = tracks
        .and_then(|t| std::fs::read_dir(t).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    folders.into_iter().map(|f| Track::open(&f).map_err(|e| (f, e))).collect()
}

/// The stage's surface, from `track.ini` `physics` (one folder writes `Gravel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Physics {
    Tarmac,
    Gravel,
    Snow,
}

impl Physics {
    /// Parses the `physics` value, ignoring case.
    pub fn parse(s: &str) -> Option<Physics> {
        match s.trim().to_ascii_lowercase().as_str() {
            "tarmac" => Some(Physics::Tarmac),
            "gravel" => Some(Physics::Gravel),
            "snow" => Some(Physics::Snow),
            _ => None,
        }
    }
}

/// The parsed `track.ini` `[INFO]`.
#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub name: String,
    pub physics_raw: String,
    pub author: Option<String>,
    pub version: Option<String>,
    pub splashscreen: Option<String>,
    pub date: Option<String>,
    pub comment: Option<String>,
    pub length_km: Option<f64>,
    /// The whole `[INFO]` section, so unknown keys stay accessible.
    pub entries: RawEntries,
}

impl TrackInfo {
    /// The parsed surface (`physics`).
    pub fn physics(&self) -> Option<Physics> {
        Physics::parse(&self.physics_raw)
    }

    fn parse(sections: &[ini::Section]) -> Option<TrackInfo> {
        let info = find_section(sections, "INFO")?;
        let get = |key: &str| find_entry(info, key).map(str::to_owned);
        Some(TrackInfo {
            name: get("name").unwrap_or_default(),
            physics_raw: get("physics").unwrap_or_default(),
            author: get("author"),
            version: get("version"),
            splashscreen: get("splashscreen"),
            date: get("date"),
            comment: get("comment"),
            length_km: get("length").and_then(|l| l.parse().ok()),
            entries: info.entries.clone(),
        })
    }
}

/// The parsed `spawn.ini` `[SPAWN]`: the sixteen `m<i>` words and the three angles. Every folder in the install
/// keeps `m00..m11` identity, `m15` zero and `angleX`/`angleY` zero; the cache's start matrix and vectors carry
/// `m12/m13/m14` and `angleZ`.
#[derive(Debug, Clone)]
pub struct Spawn {
    pub matrix: [f64; 16],
    pub angles: [f64; 3],
}

impl Spawn {
    fn parse(sections: &[ini::Section]) -> Result<Spawn, String> {
        let spawn = find_section(sections, "SPAWN").ok_or("spawn.ini: no [SPAWN]")?;
        let number = |key: &str| {
            find_entry(spawn, key).and_then(|v| v.parse::<f64>().ok()).ok_or_else(|| format!("spawn.ini: bad {key}"))
        };
        let mut matrix = [0.0; 16];
        for (i, m) in matrix.iter_mut().enumerate() {
            *m = number(&format!("m{i:02}"))?;
        }
        let mut angles = [0.0; 3];
        for (angle, key) in angles.iter_mut().zip(["angleX", "angleY", "angleZ"]) {
            *angle = number(key)?;
        }
        Ok(Spawn { matrix, angles })
    }
}

/// One `driveline.ini` `K<i>` point: position, tangent, distance and the trailing word. The eighth value is zero
/// in every folder; the cache writes both `u16`s zero, so the builder only keeps the first seven.
#[derive(Debug, Clone)]
pub struct DrivelinePoint {
    pub position: [f64; 3],
    pub tangent: [f64; 3],
    pub distance: f64,
}

/// The parsed `driveline.ini`: the declared count and the points in `K<i>` index order.
#[derive(Debug, Clone)]
pub struct Driveline {
    pub count: usize,
    pub points: Vec<DrivelinePoint>,
}

impl Driveline {
    fn parse(sections: &[ini::Section]) -> Result<Driveline, String> {
        let driveline = find_section(sections, "DRIVELINE").ok_or("driveline.ini: no [DRIVELINE]")?;
        let count: usize =
            find_entry(driveline, "count").and_then(|v| v.parse().ok()).ok_or("driveline.ini: bad count")?;
        let mut points = Vec::with_capacity(count);
        for i in 0..count {
            let key = format!("K{i}");
            let value = find_entry(driveline, &key).ok_or_else(|| format!("driveline.ini: no {key}"))?;
            let numbers: Vec<f64> = value.split(',').map(|v| v.trim().parse().unwrap_or(0.0)).collect();
            if numbers.len() != 8 {
                return Err(format!("driveline.ini: {key} has {} values, want 8", numbers.len()));
            }
            points.push(DrivelinePoint {
                position: [numbers[0], numbers[1], numbers[2]],
                tangent: [numbers[3], numbers[4], numbers[5]],
                distance: numbers[6],
            });
        }
        Ok(Driveline { count, points })
    }
}

/// The parsed `settings.ini` `[SETTINGS]`.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The `sunDirection` triple, when it holds exactly three numbers.
    pub sun_direction: Option<[f64; 3]>,
    /// The whole `[SETTINGS]` section, so unknown keys stay accessible.
    pub entries: RawEntries,
}

impl Settings {
    fn parse(sections: &[ini::Section]) -> Result<Settings, String> {
        let settings = find_section(sections, "SETTINGS").ok_or("settings.ini: no [SETTINGS]")?;
        let sun_direction = find_entry(settings, "sunDirection").and_then(|v| match &decimal_floats(v)[..] {
            &[x, y, z] => Some([x, y, z]),
            _ => None,
        });
        Ok(Settings { sun_direction, entries: settings.entries.clone() })
    }
}

/// One parsed `objectlist.ini` `[Object_<i>]` placement.
#[derive(Debug, Clone)]
pub struct Object {
    pub index: usize,
    pub file: String,
    pub xform: String,
    pub clone_count: Option<u32>,
    pub lod_in: Option<f64>,
    /// `LODOut` with its decimal comma (`2679,576`).
    pub lod_out: Option<f64>,
    pub visible: Option<bool>,
    pub draw_instanced: Option<bool>,
    pub moveable: Option<bool>,
    pub shadow_caster: Option<bool>,
    /// `CollisionModel`: 0 none, -1 unknown, 2 collision (one `(3,0x16)` mesh per such object in `trk.trk`).
    pub collision_model: Option<i32>,
    /// `colBox` as the plugin reads it ([`scan_col_box`]): `0,15, 1, 0,15` is (0, 15, 1).
    pub col_box: Option<[f32; 3]>,
    /// The whole section, so unknown keys stay accessible.
    pub entries: RawEntries,
}

/// `sscanf(value, "%f, %f, %f")` in the C locale: three numbers, each followed by a comma right after it, white
/// space free. `colBox=0,15, 1, 0,15` therefore reads (0, 15, 1) - the decimal commas are separators. `None` when
/// fewer than three read (the plugin then takes the mesh's bounding box).
pub fn scan_col_box(s: &str) -> Option<[f32; 3]> {
    let mut rest = s;
    let mut out = [0.0f32; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        if i > 0 {
            rest = rest.strip_prefix(',')?;
        }
        let rest_trimmed = rest.trim_start();
        let end = float_prefix(rest_trimmed)?;
        *slot = rest_trimmed[..end].parse().ok()?;
        rest = &rest_trimmed[end..];
    }
    Some(out)
}

/// `sscanf(s, "%f")` in the C locale, as the plugin reads `LODIn` and `LODOut`: `2679,576` is 2679.
pub fn scan_float(s: &str) -> Option<f32> {
    let s = s.trim_start();
    s[..float_prefix(s)?].parse().ok()
}

/// The length of the longest `[+-]digits[.digits][e[+-]digits]` prefix of `s`.
fn float_prefix(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let digits = |from: usize| b[from.min(b.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int = digits(i);
    i += int;
    let mut frac = 0;
    if b.get(i) == Some(&b'.') {
        frac = digits(i + 1);
        if int > 0 || frac > 0 {
            i += 1 + frac;
        }
    }
    if int + frac == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(i + 1), Some(b'+' | b'-')));
        let exp = digits(i + 1 + sign);
        if exp > 0 {
            i += 1 + sign + exp;
        }
    }
    Some(i)
}

impl Object {
    fn flag(entries: &ini::Section, key: &str) -> Option<bool> {
        match find_entry(entries, key)? {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        }
    }

    fn parse(index: usize, section: &ini::Section) -> Object {
        let col_box = find_entry(section, "colBox").and_then(scan_col_box);
        Object {
            index,
            file: find_entry(section, "file").unwrap_or_default().to_owned(),
            xform: find_entry(section, "xform").unwrap_or_default().to_owned(),
            clone_count: find_entry(section, "cloneCount").and_then(|v| v.parse().ok()),
            lod_in: find_entry(section, "LODIn").and_then(scan_float).map(f64::from),
            lod_out: find_entry(section, "LODOut").and_then(scan_float).map(f64::from),
            visible: Self::flag(section, "Visible"),
            draw_instanced: Self::flag(section, "DrawInstanced"),
            moveable: Self::flag(section, "Moveable"),
            shadow_caster: Self::flag(section, "ShadowCaster"),
            collision_model: find_entry(section, "CollisionModel").and_then(|v| v.parse().ok()),
            col_box,
            entries: section.entries.clone(),
        }
    }

    fn parse_all(sections: &[ini::Section]) -> Result<Vec<Object>, String> {
        let objects = find_section(sections, "OBJECTS").ok_or("objectlist.ini: no [OBJECTS]")?;
        let count: usize =
            find_entry(objects, "count").and_then(|v| v.parse().ok()).ok_or("objectlist.ini: bad count")?;
        (0..count)
            .map(|i| {
                find_section(sections, &format!("Object_{i}"))
                    .map(|s| Self::parse(i, s))
                    .ok_or_else(|| format!("objectlist.ini: no [Object_{i}]"))
            })
            .collect()
    }
}

/// One `materials.ini` material section: the parsed lighting numbers plus every raw entry.
#[derive(Debug, Clone)]
pub struct Material {
    pub name: String,
    /// `effectName` (`BTB_Diffuse`, `BTB_DiffuseSpecular`, `BTB_DiffuseVertLerp`, ...).
    pub effect: Option<String>,
    /// `techniqueName` (`Default`, `DefaultUp`, `Transparent`, `TransparentUp`).
    pub technique: Option<String>,
    /// `ambient` floats (`0.4,0.4,0.4,1.0` in every folder).
    pub ambient: Vec<f64>,
    /// `SpecularPower` with its decimal comma (`2,7` reads 2.7).
    pub specular_power: Option<f64>,
    /// `SpecularColor` with decimal commas (`0,4235294,0,4078431,0,2156863` reads 0.4235294, ...).
    pub specular_color: Vec<f64>,
    /// The whole section, so texture paths (`diffuseTexture`, `secondDiffuseTexture`, `normalTexture`, ...,
    /// absolute author paths the game never reads) and unknown keys stay accessible.
    pub entries: RawEntries,
}

impl Material {
    /// The first `*Texture` entry named `key`.
    pub fn texture(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

/// The parsed `materials.ini`: the `MAT_<i>` names in order and their sections.
#[derive(Debug, Clone)]
pub struct Materials {
    pub names: Vec<String>,
    pub sections: Vec<Material>,
}

impl Materials {
    fn parse(sections: &[ini::Section]) -> Result<Materials, String> {
        let materials = find_section(sections, "MATERIALS").ok_or("materials.ini: no [MATERIALS]")?;
        let count: usize =
            find_entry(materials, "count").and_then(|v| v.parse().ok()).ok_or("materials.ini: bad count")?;
        let mut names = Vec::with_capacity(count);
        for i in 0..count {
            names.push(
                find_entry(materials, &format!("MAT_{i}"))
                    .map(str::to_owned)
                    .ok_or_else(|| format!("materials.ini: no MAT_{i}"))?,
            );
        }
        let sections = names
            .iter()
            .map(|name| {
                find_section(sections, name)
                    .map(|s| Material {
                        name: name.clone(),
                        effect: find_entry(s, "effectName").map(str::to_owned),
                        technique: find_entry(s, "techniqueName").map(str::to_owned),
                        ambient: find_entry(s, "ambient").map(decimal_floats).unwrap_or_default(),
                        specular_power: find_entry(s, "SpecularPower").and_then(decimal_float),
                        specular_color: find_entry(s, "SpecularColor").map(decimal_floats).unwrap_or_default(),
                        entries: s.entries.clone(),
                    })
                    .ok_or_else(|| format!("materials.ini: no [{name}]"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Materials { names, sections })
    }
}

/// The parsed `mat.ini` `[MAP<i>]` grids: `[MAPS]` count maps of 256 cells in `data0..data15` row order. A missing
/// `[MAP<i>]` section (the `69[MAP1]` missing-newline quirk) or a missing `data<j>` row reads all zeros.
#[derive(Debug, Clone)]
pub struct SurfaceMaps {
    pub count: usize,
    pub maps: Vec<[u8; 256]>,
}

impl SurfaceMaps {
    fn parse(sections: &[ini::Section]) -> Result<SurfaceMaps, String> {
        let header = find_section(sections, "MAPS").ok_or("mat.ini: no [MAPS]")?;
        let count: usize = find_entry(header, "count").and_then(|v| v.parse().ok()).ok_or("mat.ini: bad count")?;
        let mut maps = Vec::with_capacity(count);
        for i in 0..count {
            let mut cells = [0u8; 256];
            if let Some(map) = find_section(sections, &format!("MAP{i}")) {
                for row in 0..16 {
                    if let Some(value) = find_entry(map, &format!("data{row}")) {
                        cells[row * 16..(row + 1) * 16].copy_from_slice(&data_cells(value));
                    }
                }
            }
            maps.push(cells);
        }
        Ok(SurfaceMaps { count, maps })
    }
}

/// One `pacenotes.ini` `[P<i>]` note.
#[derive(Debug, Clone)]
pub struct PaceNote {
    /// The note call id.
    pub tag: u32,
    /// The driveline distance in metres.
    pub distance: f64,
    /// The note flags.
    pub flag: u32,
}

/// The parsed `pacenotes.ini`: the first `count` notes in file order.
#[derive(Debug, Clone)]
pub struct PaceNotes {
    pub count: usize,
    pub notes: Vec<PaceNote>,
}

impl PaceNotes {
    fn parse(sections: &[ini::Section]) -> Result<PaceNotes, String> {
        let header = find_section(sections, "PACENOTES").ok_or("pacenotes.ini: no [PACENOTES]")?;
        let count: usize =
            find_entry(header, "count").and_then(|v| v.parse().ok()).ok_or("pacenotes.ini: bad count")?;
        let mut notes = Vec::with_capacity(count);
        for i in 0..count {
            let section =
                find_section(sections, &format!("P{i}")).ok_or_else(|| format!("pacenotes.ini: no [P{i}]"))?;
            let integer = |key: &str| {
                find_entry(section, key)
                    .and_then(|v| v.parse::<i64>().ok())
                    .map(|v| v as u32)
                    .ok_or_else(|| format!("pacenotes.ini: [P{i}] bad {key}"))
            };
            notes.push(PaceNote {
                tag: integer("type")?,
                distance: find_entry(section, "distance")
                    .and_then(|v| v.parse::<f64>().ok())
                    .ok_or_else(|| format!("pacenotes.ini: [P{i}] bad distance"))?,
                flag: integer("flag")?,
            });
        }
        Ok(PaceNotes { count, notes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_commas_and_points_read_as_floats() {
        assert_eq!(decimal_floats("2679,576"), vec![2679.576]);
        assert_eq!(decimal_floats("0,4235294,0,4078431,0,2156863"), vec![0.4235294, 0.4078431, 0.2156863]);
        assert_eq!(decimal_floats("-0.843359,-0.503774,0.186968"), vec![-0.843359, -0.503774, 0.186968]);
        assert_eq!(decimal_floats("2,7"), vec![2.7]);
        assert_eq!(decimal_floats("16"), vec![16.0]);
        assert!(decimal_floats("").is_empty());
        assert_eq!(decimal_float("2679,576"), Some(2679.576));
        assert_eq!(decimal_float("2"), Some(2.0));
        assert_eq!(decimal_float("0,15, 1"), None);
        assert_eq!(scan_float("2679,576"), Some(2679.0));
        assert_eq!(scan_float(" 12.5"), Some(12.5));
    }

    #[test]
    fn data_rows_read_atoi_style() {
        let cells = data_cells("69 69 69[MAP1]");
        assert_eq!(cells[..3], [69, 69, 69]);
        assert_eq!(data_cells("[MAP15] 45")[..2], [0, 45]);
        assert_eq!(data_cells("34[MAP110]")[0], 34);
        assert_eq!(data_cells(""), [0; 16]);
        let cells = data_cells("1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17");
        assert_eq!(cells, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(data_cells("-3")[0], 253);
    }

    #[test]
    fn glued_section_headers_are_not_sections() {
        // `data15=...69[MAP1]` (no newline): `[MAP1]` is not a section, the row keeps its 69.
        let sections = ini::parse("[MAPS]\ncount=2\n[MAP0]\ndata0=1\ndata15=69 69[MAP1]\ntexture=x\n").unwrap();
        assert_eq!(sections.len(), 2);
        let maps = SurfaceMaps::parse(&sections).unwrap();
        assert_eq!(maps.count, 2);
        assert_eq!(&maps.maps[0][240..], &[69, 69, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0][..]);
        assert_eq!(maps.maps[1], [0u8; 256]);
    }

    #[test]
    fn first_section_and_first_key_win() {
        let sections = ini::parse("[MAPS]\ncount=1\n[MAP0]\ndata0=1 1\ndata0=2 2\n[MAP0]\ndata0=3 3\n").unwrap();
        let maps = SurfaceMaps::parse(&sections).unwrap();
        assert_eq!(maps.maps[0][0], 1);
    }

    #[test]
    fn ini_sections_are_first_match_case_insensitive() {
        let sections = ini::parse("[SpAwN]\nM00=1\n").unwrap();
        assert!(find_section(&sections, "spawn").is_some());
        assert_eq!(find_entry(&sections[0], "m00"), Some("1"));
    }

    #[test]
    fn pace_notes_stop_at_count() {
        let sections = ini::parse(
            "[PACENOTES]\ncount=1\n[P0]\ntype=21\ndistance=204.0017\nflag=0\n[P1]\ntype=13\ndistance=1\nflag=0\n",
        )
        .unwrap();
        let notes = PaceNotes::parse(&sections).unwrap();
        assert_eq!(notes.count, 1);
        assert_eq!(notes.notes.len(), 1);
        assert_eq!((notes.notes[0].tag, notes.notes[0].flag), (21, 0));
        assert!((notes.notes[0].distance - 204.0017).abs() < 1e-9);
    }

    #[test]
    fn col_box_reads_like_sscanf() {
        assert_eq!(scan_col_box("0,15, 1, 0,15"), Some([0.0, 15.0, 1.0]));
        assert_eq!(scan_col_box("0.3, 3, 0.3"), Some([0.3, 3.0, 0.3]));
        assert_eq!(scan_col_box("1e1, 2 , 3"), None);
        assert_eq!(scan_col_box("1, 2"), None);
        let section =
            ini::Section { name: "Object_0".into(), entries: vec![("colBox".into(), "0,15, 1, 0,15".into())] };
        assert_eq!(Object::parse(0, &section).col_box, Some([0.0, 15.0, 1.0]));
    }
}
