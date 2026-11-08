//! A material's colour texture found in the level zip and made into a file the BTB scene reads: legacy DXT1/3/5 and
//! plain-texel `.dds`, `.png` and `.jpg` as they are; a DX10-header `.dds` of BC1/BC2/BC3 blocks with the legacy header
//! of the same blocks (the texels are not touched); anything else (BC7, BC5, BC6H) as a flat texture of the
//! material's colour, which the renderer's BTB path can draw.

use crate::zipfs::Archive;
use rbr_formats::dds;

/// A texture for `Textures.zip`.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    /// The file name in the archive.
    pub file: String,
    pub bytes: Vec<u8>,
    /// How it was made: `as is`, `dx10 header`, or `flat (<reason>)`.
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

/// `DXGI_FORMAT` of BC1, BC2 and BC3 (typeless, unorm, srgb) to the legacy FourCC of the same blocks.
fn legacy_fourcc(dxgi: u32) -> Option<&'static [u8; 4]> {
    match dxgi {
        70..=72 => Some(b"DXT1"),
        73..=75 => Some(b"DXT3"),
        76..=78 => Some(b"DXT5"),
        _ => None,
    }
}

/// The legacy-header form of a DX10 `.dds` of BC1/BC2/BC3 blocks.
pub fn legacy_dds(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 148 || &bytes[..4] != b"DDS " || &bytes[84..88] != b"DX10" {
        return None;
    }
    let dxgi = u32::from_le_bytes(bytes[128..132].try_into().ok()?);
    let fourcc = legacy_fourcc(dxgi)?;
    let mut out = bytes[..128].to_vec();
    out[84..88].copy_from_slice(fourcc);
    out.extend_from_slice(&bytes[148..]);
    Some(out)
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
        "dds" => match legacy_dds(&bytes) {
            Some(legacy) if dds::parse(&legacy).is_ok() => {
                Texture { file: format!("{name}.dds"), bytes: legacy, how: "dx10 header".into() }
            }
            _ => {
                let what = if bytes.len() >= 132 && &bytes[84..88] == b"DX10" {
                    format!("dxgi {}", u32::from_le_bytes([bytes[128], bytes[129], bytes[130], bytes[131]]))
                } else {
                    String::from_utf8_lossy(bytes.get(84..88).unwrap_or_default()).into_owned()
                };
                flat_one(&what)
            }
        },
        _ => flat_one("not an image"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dx10_bc1_file_gets_the_legacy_header() {
        // 4x4 BC1: one 8-byte block, DX10 header with DXGI_FORMAT_BC1_UNORM_SRGB (72).
        let mut bytes = vec![0u8; 148 + 8];
        bytes[..4].copy_from_slice(b"DDS ");
        bytes[4..8].copy_from_slice(&124u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&0x1007u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&4u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&4u32.to_le_bytes());
        bytes[76..80].copy_from_slice(&32u32.to_le_bytes());
        bytes[80..84].copy_from_slice(&4u32.to_le_bytes());
        bytes[84..88].copy_from_slice(b"DX10");
        bytes[128..132].copy_from_slice(&72u32.to_le_bytes());
        bytes[148..].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let legacy = legacy_dds(&bytes).unwrap();
        assert_eq!(legacy.len(), 128 + 8);
        assert_eq!(&legacy[84..88], b"DXT1");
        assert_eq!(&legacy[128..], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(dds::parse(&legacy).is_ok());
        bytes[128..132].copy_from_slice(&98u32.to_le_bytes());
        assert!(legacy_dds(&bytes).is_none());
        assert!(dds::parse(&flat([0.5, 0.5, 0.5, 1.0])).is_ok());
    }
}
