//! A material's colour texture found in the level zip and made into a file the BTB scene reads: every `.dds` that
//! [`dds::parse`] reads (DXT1/3/5, plain texels, DX10 BC1/2/3/7), `.png` and `.jpg` as they are; anything else (BC5,
//! BC6H) as a flat texture of the material's colour.

use crate::zipfs::Archive;
use rbr_formats::dds;

/// A texture for `Textures.zip`.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// The file name in the archive.
    pub file: String,
    pub bytes: Vec<u8>,
    /// How it was made: `as is` or `flat (<reason>)`.
    pub how: String,
}

/// The zip entry of a material texture path: as written, with `.dds`/`.png`/`.jpg` added or in place of its
/// extension, else the one file of that name (any extension of those) under `levels/<level>/`.
pub fn find(archive: &Archive, level: &str, path: &str) -> Option<String> {
    let p = path.trim().trim_start_matches('/').replace('\\', "/");
    if p.is_empty() {
        return None;
    }
    let stem = match p.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') && ext.len() <= 4 => stem.to_owned(),
        _ => p.clone(),
    };
    let mut candidates = vec![p.clone()];
    for ext in ["dds", "png", "jpg"] {
        candidates.push(format!("{p}.{ext}"));
        candidates.push(format!("{stem}.{ext}"));
    }
    if let Some(found) = candidates.iter().find_map(|c| archive.find(c)) {
        return Some(found.to_owned());
    }
    let base = stem.rsplit('/').next()?.to_lowercase();
    let prefix = format!("levels/{level}/").to_lowercase();
    let matches: Vec<String> = archive
        .names()
        .into_iter()
        .filter(|n| {
            let l = n.to_lowercase();
            let file = l.rsplit('/').next().unwrap_or("");
            l.starts_with(&prefix)
                && ["dds", "png", "jpg"]
                    .iter()
                    .any(|e| file.strip_suffix(e).and_then(|f| f.strip_suffix('.')) == Some(&base))
        })
        .collect();
    (matches.len() == 1).then(|| matches[0].clone())
}

/// A flat 4x4 texture of `color` (RGBA, 0..1).
pub fn flat(color: [f32; 4]) -> Vec<u8> {
    let texel = color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    dds::rgba8_file(4, 4, &texel.repeat(16))
}

/// The texture for a material: the file at `entry` converted as the module says, named `name` plus its extension.
pub fn make(archive: &mut Archive, entry: Option<&str>, name: &str, color: [f32; 4]) -> Texture {
    let flat_one =
        |why: &str| Texture { file: format!("{name}.dds"), bytes: flat(color), how: format!("flat ({why})") };
    let Some(entry) = entry else { return flat_one("no file") };
    let Ok(bytes) = archive.read(entry) else { return flat_one("unreadable") };
    let ext = entry.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" => Texture { file: format!("{name}.{ext}"), bytes, how: "as is".into() },
        "dds" if dds::parse(&bytes).is_ok() => Texture { file: format!("{name}.dds"), bytes, how: "as is".into() },
        "dds" => {
            let what = if bytes.len() >= 132 && &bytes[84..88] == b"DX10" {
                format!("dxgi {}", u32::from_le_bytes([bytes[128], bytes[129], bytes[130], bytes[131]]))
            } else {
                String::from_utf8_lossy(bytes.get(84..88).unwrap_or_default()).into_owned()
            };
            flat_one(&what)
        }
        _ => flat_one("not an image"),
    }
}

/// The texture for a material with an opacity map at `opacity`: the colour at `entry` (else `color` all over) with
/// the opacity as its alpha ([`crate::cutout::merge`]), `how` `cut out`; the plain [`make`] when either does not
/// decode (a BC7 colour), its `how` saying so.
pub fn make_cut_out(archive: &mut Archive, entry: Option<&str>, opacity: &str, name: &str, color: [f32; 4]) -> Texture {
    let decoded = |archive: &mut Archive, e: &str| archive.read(e).ok().and_then(|b| crate::cutout::decode(&b));
    let Some(alpha) = decoded(archive, opacity) else {
        let mut t = make(archive, entry, name, color);
        t.how = format!("{} (opacity map not decoded)", t.how);
        return t;
    };
    let base = match entry {
        Some(e) => match decoded(archive, e) {
            Some(image) => Some(image),
            None => {
                let mut t = make(archive, entry, name, color);
                t.how = format!("{} (colour not decoded, no cut-out)", t.how);
                return t;
            }
        },
        None => None,
    };
    Texture { file: format!("{name}.dds"), bytes: crate::cutout::merge(base, color, &alpha), how: "cut out".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_texture_reads() {
        assert!(dds::parse(&flat([0.5, 0.5, 0.5, 1.0])).is_ok());
    }
}
