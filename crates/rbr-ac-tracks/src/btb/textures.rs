//! Source image decoding, opacity merging and BTB texture encoding.
//! Separate opacity maps are merged into diffuse alpha and encoded as DXT5 with mipmaps.

use rbr_formats::dds;

/// An image as RGBA8 texels.
pub type Image = (u32, u32, Vec<u8>);

/// `DDS_PIXELFORMAT` four-character code at this offset; a DX10 header's DXGI format at 128.
const FOURCC: usize = 84;
const HEADER: usize = 128;

/// The top level of a PNG, JPEG or `.dds` file (DXT1/3/5, plain texels, legacy channel-mask layouts, BC4, BC5 and DX10 BC7) as RGBA8; BC4 and BC5
/// keep their channels in red (and green).
pub fn decode(bytes: &[u8]) -> Option<Image> {
    if bytes.get(..4) != Some(b"DDS ") {
        return rbr_btb::scene::decode_image(bytes);
    }
    let word = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let (height, width) = (word(12)?, word(16)?);
    let fourcc = bytes.get(FOURCC..FOURCC + 4)?;
    let dxgi = if fourcc == b"DX10" { word(HEADER) } else { None };
    let block = match (fourcc, dxgi) {
        (b"ATI1" | b"BC4U", _) | (_, Some(80)) => Some(texpresso::Format::Bc4),
        (b"ATI2" | b"BC5U", _) | (_, Some(83)) => Some(texpresso::Format::Bc5),
        _ => None,
    };
    if let Some(format) = block {
        let start = HEADER + if dxgi.is_some() { 20 } else { 0 };
        let size = format.compressed_size(width as usize, height as usize);
        let data = bytes.get(start..start + size)?;
        let mut rgba = vec![0; (width * height * 4) as usize];
        format.decompress(data, width as usize, height as usize, &mut rgba);
        return Some((width, height, rgba));
    }
    let Ok(file) = dds::parse(bytes) else { return legacy_dds(bytes) };
    let level = file.levels().into_iter().next()?;
    let format = match file.format {
        dds::Format::Rgba8 => return Some((file.width, file.height, level.data.to_vec())),
        dds::Format::Bc7 => return bc7(level.data, file.width, file.height).map(|t| (file.width, file.height, t)),
        dds::Format::Dxt1 => texpresso::Format::Bc1,
        dds::Format::Dxt3 => texpresso::Format::Bc2,
        dds::Format::Dxt5 => texpresso::Format::Bc3,
    };
    let mut rgba = vec![0; (file.width * file.height * 4) as usize];
    format.decompress(level.data, file.width as usize, file.height as usize, &mut rgba);
    Some((file.width, file.height, rgba))
}

/// Uncompressed legacy DDS with channel masks, including X8R8G8B8 (no alpha mask),
/// BGR24, RGB565, luminance and alpha-only formats. Rows can carry pitch padding.
pub fn legacy_dds(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.get(..4)? != b"DDS " || data.len() < 128 {
        return None;
    }
    let word = |at| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    let (w, h, bits) = (word(16), word(12), word(88));
    if word(84) != 0 || !matches!(bits, 8 | 16 | 24 | 32) || w == 0 || h == 0 || w > 32768 || h > 32768 {
        return None;
    }
    let bytes = (bits / 8) as usize;
    let stride = w as usize * bytes;
    let pitch = if word(8) & 8 != 0 { (word(20) as usize).max(stride) } else { stride };
    data.get(128..128 + pitch * h as usize)?;
    let flags = word(80);
    let masks = [word(92), word(96), word(100), word(104)];
    let channel = |p: u32, mask: u32, default: u8| {
        if mask == 0 {
            return default;
        }
        let shift = mask.trailing_zeros();
        (((p & mask) >> shift) as u64 * 255 / (mask >> shift) as u64) as u8
    };
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let at = 128 + y * pitch + x * bytes;
            let mut raw = [0; 4];
            raw[..bytes].copy_from_slice(&data[at..at + bytes]);
            let p = u32::from_le_bytes(raw);
            let luminance = channel(p, masks[0], 0);
            if flags & 0x20000 != 0 {
                rgba.extend([luminance, luminance, luminance, channel(p, masks[3], 255)]);
            } else if flags & 2 != 0 && flags & 0x40 == 0 {
                rgba.extend([
                    255,
                    255,
                    255,
                    channel(p, if masks[3] == 0 { (1u64 << bits).wrapping_sub(1) as u32 } else { masks[3] }, 255),
                ]);
            } else {
                rgba.extend([luminance, channel(p, masks[1], 0), channel(p, masks[2], 0), channel(p, masks[3], 255)]);
            }
        }
    }
    Some((w, h, rgba))
}

/// BC7 blocks as RGBA8 (bcdec).
fn bc7(data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    let bw = w.div_ceil(4);
    let data = data.get(..bw * h.div_ceil(4) * 16)?;
    let mut out = vec![0u8; w * h * 4];
    let mut texels = [0u8; 64];
    for (k, block) in data.chunks_exact(16).enumerate() {
        bcdec_rs::bc7(block, &mut texels, 16);
        let (bx, by) = (k % bw * 4, k / bw * 4);
        for row in 0..4.min(h - by) {
            let n = 4.min(w - bx) * 4;
            out[((by + row) * w + bx) * 4..][..n].copy_from_slice(&texels[row * 16..][..n]);
        }
    }
    Some(out)
}

/// The colour (`None`: `factor` all over, at the opacity map's size) with the opacity map's red channel, resampled to
/// the colour's size, as its alpha: a DXT5 `.dds` with its mip chain.
pub fn merge(color: Option<Image>, factor: [f32; 4], opacity: &Image) -> Vec<u8> {
    let (width, height, mut rgba) = color.unwrap_or_else(|| {
        let texel = factor.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        (opacity.0, opacity.1, texel.repeat((opacity.0 * opacity.1) as usize))
    });
    let (ow, oh, o) = opacity;
    for y in 0..height {
        let oy = (u64::from(y) * u64::from(*oh) / u64::from(height)) as u32;
        for x in 0..width {
            let ox = (u64::from(x) * u64::from(*ow) / u64::from(width)) as u32;
            rgba[((y * width + x) * 4 + 3) as usize] = o[((oy * ow + ox) * 4) as usize];
        }
    }
    encode(true, width, height, rgba)
}

/// A DXT5 (`alpha`, else DXT1) `.dds` of `rgba` with its mip chain, each level the 2x2 box filter of the one above.
/// Without `alpha` the alpha is ignored: DXT1 would otherwise turn texels under half alpha into transparent black.
pub fn encode(alpha: bool, mut width: u32, mut height: u32, mut rgba: Vec<u8>) -> Vec<u8> {
    if !alpha {
        rgba.chunks_exact_mut(4).for_each(|p| p[3] = 255);
    }
    let (w0, h0) = (width, height);
    let format = if alpha { texpresso::Format::Bc3 } else { texpresso::Format::Bc1 };
    let mut levels = Vec::new();
    loop {
        let mut block = vec![0; format.compressed_size(width as usize, height as usize)];
        let params = texpresso::Params { algorithm: texpresso::Algorithm::RangeFit, ..Default::default() };
        format.compress(&rgba, width as usize, height as usize, params, &mut block);
        levels.push(block);
        if width == 1 && height == 1 {
            break;
        }
        (width, height, rgba) = half(width, height, &rgba);
    }
    let mut header = [0u32; 32];
    header[0] = u32::from_le_bytes(*b"DDS ");
    header[1] = 124;
    // Caps, height, width, pixel format, mip count and linear size are set.
    header[2] = 0x1 | 0x2 | 0x4 | 0x1000 | 0x2_0000 | 0x8_0000;
    header[3] = h0;
    header[4] = w0;
    header[5] = levels[0].len() as u32;
    header[7] = levels.len() as u32;
    header[19] = 32;
    header[20] = 0x4;
    header[21] = u32::from_le_bytes(if alpha { *b"DXT5" } else { *b"DXT1" });
    // Texture, complex, mipmap.
    header[27] = 0x1000 | 0x8 | 0x40_0000;
    let mut file: Vec<u8> = header.iter().flat_map(|w| w.to_le_bytes()).collect();
    for block in &levels {
        file.extend_from_slice(block);
    }
    file
}

/// The 2x2 box filter of `rgba`: the next mip level.
pub fn half(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let texel =
        |x: u32, y: u32, c: usize| u32::from(rgba[((y.min(height - 1) * width + x.min(width - 1)) * 4) as usize + c]);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let sum = texel(2 * x, 2 * y, c)
                    + texel(2 * x + 1, 2 * y, c)
                    + texel(2 * x, 2 * y + 1, c)
                    + texel(2 * x + 1, 2 * y + 1, c);
                out.push(((sum + 2) / 4) as u8);
            }
        }
    }
    (w, h, out)
}

/// A flat 4x4 texture of `color` (RGBA, 0..1).
pub fn flat(color: [f32; 4]) -> Vec<u8> {
    let texel = color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    dds::rgba8_file(4, 4, &texel.repeat(16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_xrgb_decodes_channel_masks_and_ignores_unused_alpha() {
        let mut data = vec![0; 136];
        data[..4].copy_from_slice(b"DDS ");
        for (at, value) in [(12, 1u32), (16, 2), (80, 0x40), (88, 32), (92, 0xff0000), (96, 0xff00), (100, 0xff)] {
            data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        data[128..].copy_from_slice(&[30, 20, 10, 0, 60, 50, 40, 0]);
        assert_eq!(legacy_dds(&data).unwrap().2, [10, 20, 30, 255, 40, 50, 60, 255]);
        assert!(legacy_dds(&data[..135]).is_none());
        assert_eq!(decode(&data).unwrap().2, [10, 20, 30, 255, 40, 50, 60, 255], "decode reads it too");
    }

    #[test]
    fn the_opacity_becomes_the_alpha() {
        // A 4x4 opacity map, clear on the left half, over an 8x8 grey colour.
        let opacity: Vec<u8> =
            (0..16).flat_map(|i| if i % 4 < 2 { [0, 0, 0, 255] } else { [255, 0, 0, 255] }).collect();
        let file = merge(Some((8, 8, vec![128; 256])), [1.0; 4], &(4, 4, opacity));
        let dds = dds::parse(&file).unwrap();
        assert_eq!((dds.width, dds.height, dds.format, dds.mip_count), (8, 8, dds::Format::Dxt5, 4));
        let (_, _, rgba) = decode(&file).unwrap();
        assert_eq!((rgba[3], rgba[7 * 4 + 3], rgba[1]), (0, 255, 128));
    }

    #[test]
    fn an_opaque_encode_keeps_the_colour_under_zero_alpha() {
        // An AC detail mask in the diffuse's alpha must not become DXT1's transparent black.
        let (_, _, rgba) = decode(&encode(false, 4, 4, [200, 100, 50, 0].repeat(16))).unwrap();
        assert!(rgba.chunks(4).all(|t| t[3] == 255 && t[0] > 190), "{rgba:?}");
    }

    #[test]
    fn a_bc7_block_of_one_colour() {
        // Mode 6 (bit 6 set), both endpoints (127, 0, 127, 127) with p-bits 1: every texel (255, 1, 255, 255).
        let mut value: u128 = 1 << 6;
        let mut at = 7;
        for c in [127u128, 0, 127, 127] {
            value |= (c | c << 7) << at;
            at += 14;
        }
        value |= 0b11 << at;
        let texels = bc7(&value.to_le_bytes(), 4, 4).unwrap();
        assert!(texels.chunks(4).all(|t| t == [255, 1, 255, 255]), "{texels:?}");
    }
}
