//! Custom Shaders Patch vertex ambient occlusion (`.vao-patch`): a zip with `Config.ini` and one of `Patch_v5.data`,
//! `Patch_v4.data` or the legacy `Patch.data`. The data is records back to back: u32 name length, the name, u32 kind,
//! the mesh's raw first vertex (3 × f32, tells meshes of one name apart), u32 vertex count and the payload.
//!
//! Kinds: 0 three components per vertex, 1 one component (the AO), 2 normals, 3 a secondary AO set, 4 extra samples
//! (3 × f32 and a byte each). Only kind 1 is used (kind 0 averaged, legacy only); the others are stepped over.
//! Components are a byte in v4 and v5, an IEEE half in the legacy file. Written from the format's writers (CSP's
//! CustomTracksBakery, acc-bakeryoptix) and newksEditor's reader; not checked against CSP itself.

use std::{collections::HashMap, io::Read, path::Path};

/// Which data file the patch carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Legacy,
    V4,
    V5,
}

impl Version {
    pub fn name(self) -> &'static str {
        match self {
            Version::Legacy => "Patch.data",
            Version::V4 => "Patch_v4.data",
            Version::V5 => "Patch_v5.data",
        }
    }
}

/// One mesh's AO, linear, 1 open to 0 shut.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub first: [f32; 3],
    pub ao: Vec<f32>,
    /// Decoded from a three-component record (legacy only), so a one-component record of the mesh wins.
    averaged: bool,
}

#[derive(Debug)]
pub struct Patch {
    pub version: Version,
    /// The patch's file, for the report.
    pub source: String,
    records: HashMap<String, Vec<Record>>,
}

/// CSP's `[LIGHTING]` of a legacy patch (newksEditor's defaults when the file gives none).
#[derive(Clone, Copy)]
struct Lighting {
    opacity: f32,
    brightness: f32,
    gamma: f32,
}

impl Lighting {
    fn read(config: &str) -> Self {
        let sections = rbr_formats::ini::parse(config).unwrap_or_default();
        let section = sections.iter().find(|s| s.name.eq_ignore_ascii_case("LIGHTING"));
        let value = |key: &str, default: f32| {
            section.and_then(|s| s.get(key)).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(default)
        };
        Self { opacity: value("OPACITY", 0.85), brightness: value("BRIGHTNESS", 1.1), gamma: value("GAMMA", 1.) }
    }

    fn apply(self, v: f32) -> f32 {
        ((self.opacity * v + 1. - self.opacity) * self.brightness).clamp(0., 1.).powf(self.gamma)
    }
}

fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1. } else { 1. };
    let exponent = i32::from((bits >> 10) & 31);
    let fraction = f32::from(bits & 1023);
    sign * match exponent {
        0 => fraction * 2f32.powi(-24),
        31 if fraction == 0. => f32::INFINITY,
        31 => f32::NAN,
        e => (1. + fraction / 1024.) * 2f32.powi(e - 15),
    }
}

/// Reads a `.vao-patch` file.
pub fn read(path: &Path) -> Result<Patch, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut entry = |name: &str| -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        zip.by_name(name).ok()?.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    };
    let (version, data) =
        [(Version::V5, "Patch_v5.data"), (Version::V4, "Patch_v4.data"), (Version::Legacy, "Patch.data")]
            .into_iter()
            .find_map(|(v, name)| entry(name).map(|d| (v, d)))
            .ok_or_else(|| format!("{}: no Patch data", path.display()))?;
    let config = entry("Config.ini").map(|c| String::from_utf8_lossy(&c).into_owned()).unwrap_or_default();
    let mut patch = parse(&data, version, &config).map_err(|e| format!("{}: {e}", path.display()))?;
    patch.source = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(patch)
}

/// Parses the data of `version`. A truncated payload ends the records read so far (the file's tail is extra samples).
pub fn parse(data: &[u8], version: Version, config: &str) -> Result<Patch, String> {
    let lighting = Lighting::read(config);
    let mut records: HashMap<String, Vec<Record>> = HashMap::new();
    let u32_at = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let f32_at = |at: usize| u32_at(at).map(f32::from_bits);
    let mut at = 0;
    while at < data.len() {
        let len = u32_at(at).ok_or("truncated record")? as usize;
        if len == 0 || len > 4096 {
            return Err(format!("bad record name length {len} at {at}"));
        }
        let name = data.get(at + 4..at + 4 + len).ok_or("truncated record name")?;
        at += 4 + len;
        let (Some(kind), Some(x), Some(y), Some(z), Some(count)) =
            (u32_at(at), f32_at(at + 4), f32_at(at + 8), f32_at(at + 12), u32_at(at + 16))
        else {
            return Err("truncated record header".into());
        };
        at += 20;
        let count = count as usize;
        let components = match kind {
            0 | 2 => 3,
            1 | 3 => 1,
            4 => 0,
            other => return Err(format!("unknown record kind {other}")),
        };
        let width = if kind == 4 {
            13
        } else if version == Version::Legacy {
            components * 2
        } else {
            components
        };
        let payload = data.get(at..at.saturating_add(count.saturating_mul(width))).ok_or("truncated record payload")?;
        at += payload.len();
        let name = String::from_utf8_lossy(name);
        if !matches!(kind, 0 | 1) || (kind == 0 && version != Version::Legacy) || name.starts_with("@@") {
            continue;
        }
        let ao = match version {
            Version::V5 => payload.iter().map(|&b| (f32::from(b) / 255.).powi(2)).collect(),
            Version::V4 => payload.iter().map(|&b| f32::from(b) / 255.).collect(),
            Version::Legacy => payload
                .chunks_exact(components * 2)
                .map(|c| {
                    let v = c.chunks_exact(2).map(|h| half(u16::from_le_bytes([h[0], h[1]]))).sum::<f32>()
                        / components as f32;
                    lighting.apply(v)
                })
                .collect(),
        };
        records.entry(name.into_owned()).or_default().push(Record { first: [x, y, z], ao, averaged: kind == 0 });
    }
    Ok(Patch { version, source: String::new(), records })
}

/// A patch and the model it belongs to (`None`: the whole layout).
pub struct Found {
    pub model: Option<std::path::PathBuf>,
    pub patch: Patch,
}

/// The patches of a layout of the track in `root`: the layout's own `models_<layout>.vao-patch` (`models.vao-patch`
/// for the default layout) and a `<kn5 stem>.vao-patch` beside each model; CSP's own set
/// (`<AC root>/extension/vao-patches/<track folder>.vao-patch`) only when the track has none. Unreadable files are
/// skipped with a message in the second part.
pub fn discover(root: &Path, layout: &str, models: &[std::path::PathBuf]) -> (Vec<Found>, Vec<String>) {
    let mut out = (Vec::new(), Vec::new());
    let load = |out: &mut (Vec<Found>, Vec<String>), path: std::path::PathBuf, model: Option<std::path::PathBuf>| {
        if !path.is_file() {
            return;
        }
        match read(&path) {
            Ok(patch) => out.0.push(Found { model, patch }),
            Err(e) => out.1.push(e),
        }
    };
    let own = if layout.is_empty() || layout == "." { "models".to_owned() } else { format!("models_{layout}") };
    load(&mut out, root.join(format!("{own}.vao-patch")), None);
    for model in models {
        load(&mut out, model.with_extension("vao-patch"), Some(model.clone()));
    }
    if out.0.is_empty() && out.1.is_empty() {
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(ac) = root.ancestors().nth(3) {
            load(&mut out, ac.join("extension/vao-patches").join(format!("{name}.vao-patch")), None);
        }
    }
    out
}

impl Patch {
    /// The record for a mesh `name` with `count` vertices starting at raw vertex `first`: the exact one, else the only
    /// one of that name and count. A three-component record only stands in when no one-component record fits.
    pub fn find(&self, name: &str, count: usize, first: [f32; 3]) -> Option<&Record> {
        let candidates: Vec<&Record> = self.records.get(name)?.iter().filter(|r| r.ao.len() == count).collect();
        let near: Vec<&Record> = candidates
            .iter()
            .copied()
            .filter(|r| r.first.iter().zip(first).all(|(a, b)| (a - b).abs() < 1e-3))
            .collect();
        near.iter().copied().find(|r| !r.averaged).or(near.first().copied()).or_else(|| {
            let ones: Vec<&Record> = candidates.iter().copied().filter(|r| !r.averaged).collect();
            if ones.len() == 1 {
                ones.first().copied()
            } else {
                (candidates.len() == 1).then(|| candidates[0])
            }
        })
    }

    /// The number of records kept.
    pub fn record_count(&self) -> usize {
        self.records.values().map(Vec::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(out: &mut Vec<u8>, name: &str, kind: u32, first: [f32; 3], count: u32, payload: &[u8]) {
        out.extend((name.len() as u32).to_le_bytes());
        out.extend(name.as_bytes());
        out.extend(kind.to_le_bytes());
        for v in first {
            out.extend(v.to_le_bytes());
        }
        out.extend(count.to_le_bytes());
        out.extend(payload);
    }

    #[test]
    fn v5_squares_the_byte_and_steps_over_other_kinds() {
        let mut d = Vec::new();
        record(&mut d, "a", 2, [0.; 3], 2, &[1; 6]);
        record(&mut d, "a", 1, [1., 2., 3.], 2, &[255, 128]);
        record(&mut d, "@@__ALT@:a", 1, [1., 2., 3.], 2, &[0, 0]);
        record(&mut d, "a", 3, [1., 2., 3.], 2, &[9, 9]);
        record(&mut d, "@@__EXTRA_AO@", 4, [0.; 3], 1, &[0; 13]);
        let p = parse(&d, Version::V5, "").unwrap();
        let r = p.find("a", 2, [1., 2., 3.]).unwrap();
        assert_eq!(r.ao[0], 1.);
        assert!((r.ao[1] - (128f32 / 255.).powi(2)).abs() < 1e-6);
        assert_eq!(p.record_count(), 1);
    }

    #[test]
    fn v4_is_linear() {
        let mut d = Vec::new();
        record(&mut d, "a", 1, [0.; 3], 2, &[51, 255]);
        let p = parse(&d, Version::V4, "").unwrap();
        let r = p.find("a", 2, [0.; 3]).unwrap();
        assert!((r.ao[0] - 0.2).abs() < 1e-6 && r.ao[1] == 1.);
    }

    #[test]
    fn legacy_halves_go_through_the_lighting_of_the_config() {
        let one = 0x3c00u16.to_le_bytes(); // 1.0
        let half_ = 0x3800u16.to_le_bytes(); // 0.5
        let mut d = Vec::new();
        record(&mut d, "a", 1, [0.; 3], 2, &[one, half_].concat());
        // kind 0, mean of (1, 0.5, 0) = 0.5
        record(&mut d, "b", 0, [0.; 3], 1, &[one, half_, [0, 0]].concat());
        let p = parse(&d, Version::Legacy, "[LIGHTING]\nOPACITY=0.4\nBRIGHTNESS=1\n").unwrap();
        let a = p.find("a", 2, [0.; 3]).unwrap();
        assert!((a.ao[0] - 1.).abs() < 1e-6);
        assert!((a.ao[1] - (0.4 * 0.5 + 0.6)).abs() < 1e-6);
        let b = p.find("b", 1, [0.; 3]).unwrap();
        assert!((b.ao[0] - (0.4 * 0.5 + 0.6)).abs() < 1e-6);
        // defaults when the config is silent: (0.85 * 0.5 + 0.15) * 1.1
        let p = parse(&d, Version::Legacy, "").unwrap();
        assert!((p.find("a", 2, [0.; 3]).unwrap().ao[1] - 0.575 * 1.1).abs() < 1e-6);
    }

    #[test]
    fn duplicate_names_are_told_apart_by_the_first_vertex() {
        let mut d = Vec::new();
        record(&mut d, "a", 1, [0.; 3], 1, &[255]);
        record(&mut d, "a", 1, [5., 0., 0.], 1, &[0]);
        let p = parse(&d, Version::V4, "").unwrap();
        assert_eq!(p.find("a", 1, [5., 0., 0.]).unwrap().ao, vec![0.]);
        assert_eq!(p.find("a", 1, [0.; 3]).unwrap().ao, vec![1.]);
        // Neither is near and two fit: no guess.
        assert!(p.find("a", 1, [9., 9., 9.]).is_none());
        assert!(p.find("a", 2, [0.; 3]).is_none());
    }

    #[test]
    fn truncated_data_is_an_error() {
        let mut d = Vec::new();
        record(&mut d, "a", 1, [0.; 3], 4, &[1, 2]);
        assert!(parse(&d, Version::V5, "").is_err());
    }
}
