//! Cut-outs: BeamNG keeps a material's opacity in its own map (`opacityMap`), BTB takes it from the diffuse texture's
//! alpha. [`merge`] decodes both, puts the opacity map's first channel into the colour's alpha and encodes DXT5 with
//! its mip chain (texpresso, as `rbr_render::upscale::encode`).

use rbr_formats::dds;

/// An image as RGBA8 texels.
pub type Image = (u32, u32, Vec<u8>);

/// `DDS_PIXELFORMAT` four-character code at this offset; a DX10 header's DXGI format at 128.
const FOURCC: usize = 84;
const HEADER: usize = 128;

/// The top level of a PNG, JPEG or `.dds` file (DXT1/3/5, plain texels, BC4, BC5 and DX10 BC7) as RGBA8; BC4 and BC5
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
    let file = dds::parse(bytes).ok()?;
    let level = file.levels().into_iter().next()?;
    let format = match file.format {
        dds::Format::Rgba8 => return Some((file.width, file.height, level.data.to_vec())),
        dds::Format::Bc7 => {
            return crate::bc7::decode(level.data, file.width, file.height).map(|t| (file.width, file.height, t))
        }
        dds::Format::Dxt1 => texpresso::Format::Bc1,
        dds::Format::Dxt3 => texpresso::Format::Bc2,
        dds::Format::Dxt5 => texpresso::Format::Bc3,
    };
    let mut rgba = vec![0; (file.width * file.height * 4) as usize];
    format.decompress(level.data, file.width as usize, file.height as usize, &mut rgba);
    Some((file.width, file.height, rgba))
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
    encode(width, height, rgba)
}

/// A DXT5 `.dds` of `rgba` with its mip chain, each level the 2x2 box filter of the one above.
fn encode(mut width: u32, mut height: u32, mut rgba: Vec<u8>) -> Vec<u8> {
    let (w0, h0) = (width, height);
    let bc3 = texpresso::Format::Bc3;
    let mut levels = Vec::new();
    loop {
        let mut block = vec![0; bc3.compressed_size(width as usize, height as usize)];
        let params = texpresso::Params { algorithm: texpresso::Algorithm::RangeFit, ..Default::default() };
        bc3.compress(&rgba, width as usize, height as usize, params, &mut block);
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
    header[21] = u32::from_le_bytes(*b"DXT5");
    // Texture, complex, mipmap.
    header[27] = 0x1000 | 0x8 | 0x40_0000;
    let mut file: Vec<u8> = header.iter().flat_map(|w| w.to_le_bytes()).collect();
    for block in &levels {
        file.extend_from_slice(block);
    }
    file
}

fn half(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
