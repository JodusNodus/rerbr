//! BC7 blocks decoded to RGBA8 (Direct3D 11's BC7 format: eight modes, up to three subsets, the partition and anchor
//! tables of the format), so a BC7 diffuse can take an opacity map's alpha ([`crate::cutout`]).

/// Per mode: subsets, partition bits, rotation bits, index selection bits, colour bits, alpha bits, a p-bit per
/// endpoint, a p-bit per subset, index bits, second index bits.
const MODES: [[u8; 10]; 8] = [
    [3, 4, 0, 0, 4, 0, 1, 0, 3, 0],
    [2, 6, 0, 0, 6, 0, 0, 1, 3, 0],
    [3, 6, 0, 0, 5, 0, 0, 0, 2, 0],
    [2, 6, 0, 0, 7, 0, 1, 0, 2, 0],
    [1, 0, 2, 1, 5, 6, 0, 0, 2, 3],
    [1, 0, 2, 0, 7, 8, 0, 0, 2, 2],
    [1, 0, 0, 0, 7, 7, 1, 0, 4, 0],
    [2, 6, 0, 0, 5, 5, 1, 0, 2, 0],
];
const PARTITIONS2: [u16; 64] = [
    0xcccc, 0x8888, 0xeeee, 0xecc8, 0xc880, 0xfeec, 0xfec8, 0xec80, 0xc800, 0xffec, 0xfe80, 0xe800, 0xffe8, 0xff00,
    0xfff0, 0xf000, 0xf710, 0x008e, 0x7100, 0x08ce, 0x008c, 0x7310, 0x3100, 0x8cce, 0x088c, 0x3110, 0x6666, 0x366c,
    0x17e8, 0x0ff0, 0x718e, 0x399c, 0xaaaa, 0xf0f0, 0x5a5a, 0x33cc, 0x3c3c, 0x55aa, 0x9696, 0xa55a, 0x73ce, 0x13c8,
    0x324c, 0x3bdc, 0x6996, 0xc33c, 0x9966, 0x0660, 0x0272, 0x04e4, 0x4e40, 0x2720, 0xc936, 0x936c, 0x39c6, 0x639c,
    0x9336, 0x9cc6, 0x817e, 0xe718, 0xccf0, 0x0fcc, 0x7744, 0xee22,
];
const PARTITIONS3: [u32; 64] = [
    0xaa685050, 0x6a5a5040, 0x5a5a4200, 0x5450a0a8, 0xa5a50000, 0xa0a05050, 0x5555a0a0, 0x5a5a5050, 0xaa550000,
    0xaa555500, 0xaaaa5500, 0x90909090, 0x94949494, 0xa4a4a4a4, 0xa9a59450, 0x2a0a4250, 0xa5945040, 0x0a425054,
    0xa5a5a500, 0x55a0a0a0, 0xa8a85454, 0x6a6a4040, 0xa4a45000, 0x1a1a0500, 0x0050a4a4, 0xaaa59090, 0x14696914,
    0x69691400, 0xa08585a0, 0xaa821414, 0x50a4a450, 0x6a5a0200, 0xa9a58000, 0x5090a0a8, 0xa8a09050, 0x24242424,
    0x00aa5500, 0x24924924, 0x24499224, 0x50a50a50, 0x500aa550, 0xaaaa4444, 0x66660000, 0xa5a0a5a0, 0x50a050a0,
    0x69286928, 0x44aaaa44, 0x66666600, 0xaa444444, 0x54a854a8, 0x95809580, 0x96969600, 0xa85454a8, 0x80959580,
    0xaa141414, 0x96960000, 0xaaaa1414, 0xa05050a0, 0xa0a5a5a0, 0x96000000, 0x40804080, 0xa9a8a9a8, 0xaaaaaa44,
    0x2a4a5254,
];
const ANCHORS2: [u8; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8, 2, 2, 8, 8, 2, 2,
    15, 15, 6, 8, 2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2, 2, 15, 15, 15, 15, 15, 2, 2, 15,
];
const ANCHORS3: [[u8; 2]; 64] = [
    [3, 15],
    [3, 8],
    [15, 8],
    [15, 3],
    [8, 15],
    [3, 15],
    [15, 3],
    [15, 8],
    [8, 15],
    [8, 15],
    [6, 15],
    [6, 15],
    [6, 15],
    [5, 15],
    [3, 15],
    [3, 8],
    [3, 15],
    [3, 8],
    [8, 15],
    [15, 3],
    [3, 15],
    [3, 8],
    [6, 15],
    [10, 8],
    [5, 3],
    [8, 15],
    [8, 6],
    [6, 10],
    [8, 15],
    [5, 15],
    [15, 10],
    [15, 8],
    [8, 15],
    [15, 3],
    [3, 15],
    [5, 10],
    [6, 10],
    [10, 8],
    [8, 9],
    [15, 10],
    [15, 6],
    [3, 15],
    [15, 8],
    [5, 15],
    [15, 3],
    [15, 6],
    [15, 6],
    [15, 8],
    [3, 15],
    [15, 3],
    [5, 15],
    [5, 15],
    [5, 15],
    [8, 15],
    [5, 15],
    [10, 15],
    [5, 15],
    [10, 15],
    [8, 15],
    [13, 15],
    [15, 3],
    [12, 15],
    [3, 15],
    [3, 8],
];

/// Interpolation weights of 2-, 3- and 4-bit indices, out of 64.
const WEIGHTS2: [u32; 4] = [0, 21, 43, 64];
const WEIGHTS3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const WEIGHTS4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// The bits of a block, read from the lowest.
struct Bits {
    value: u128,
    at: u32,
}

impl Bits {
    fn take(&mut self, n: u8) -> u32 {
        let v = ((self.value >> self.at) & ((1u128 << n) - 1)) as u32;
        self.at += u32::from(n);
        v
    }
}

/// The texels of a BC7 level `width` x `height` (whole blocks in `data`); `None` when `data` is short.
pub fn decode(data: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let (bw, bh) = (width.div_ceil(4) as usize, height.div_ceil(4) as usize);
    let data = data.get(..bw * bh * 16)?;
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w * h * 4];
    for (k, block) in data.chunks_exact(16).enumerate() {
        let texels = decode_block(block.try_into().ok()?);
        let (bx, by) = (k % bw * 4, k / bw * 4);
        for (i, t) in texels.iter().enumerate() {
            let (x, y) = (bx + i % 4, by + i / 4);
            if x < w && y < h {
                out[(y * w + x) * 4..][..4].copy_from_slice(t);
            }
        }
    }
    Some(out)
}

/// One 4x4 block's texels, row by row; a block of no mode is transparent black.
pub fn decode_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let mut bits = Bits { value: u128::from_le_bytes(*block), at: 0 };
    let Some(mode) = (0..8).find(|_| bits.take(1) == 1) else { return [[0; 4]; 16] };
    let [subsets, pb, rb, isb, cb, ab, epb, spb, ib, ib2] = MODES[mode];
    let partition = bits.take(pb) as usize;
    let rotation = bits.take(rb);
    let selection = bits.take(isb);
    let ends = usize::from(subsets) * 2;
    let mut endpoints = [[0u32; 4]; 6];
    for c in 0..3 {
        for e in endpoints.iter_mut().take(ends) {
            e[c] = bits.take(cb);
        }
    }
    for e in endpoints.iter_mut().take(ends) {
        e[3] = if ab > 0 { bits.take(ab) } else { 255 };
    }
    // P-bits: one per endpoint, or one per subset shared by its two endpoints.
    let mut pbits = [0u32; 6];
    if epb > 0 {
        for p in pbits.iter_mut().take(ends) {
            *p = bits.take(1);
        }
    } else if spb > 0 {
        for s in 0..usize::from(subsets) {
            let p = bits.take(1);
            pbits[2 * s] = p;
            pbits[2 * s + 1] = p;
        }
    }
    let has_p = epb > 0 || spb > 0;
    for (e, p) in endpoints.iter_mut().zip(pbits).take(ends) {
        for (c, v) in e.iter_mut().enumerate() {
            let width = if c < 3 { cb } else { ab };
            if width == 0 {
                continue;
            }
            let (value, width) = if has_p { ((*v << 1) | p, width + 1) } else { (*v, width) };
            *v = expand(value, u32::from(width));
        }
    }
    let subset_of = |i: usize| -> usize {
        match subsets {
            2 => usize::from(PARTITIONS2[partition] >> i & 1 == 1),
            3 => (PARTITIONS3[partition] >> (2 * i) & 3) as usize,
            _ => 0,
        }
    };
    let anchor = |i: usize| -> bool {
        i == 0
            || match subsets {
                2 => i == usize::from(ANCHORS2[partition]),
                3 => ANCHORS3[partition].contains(&(i as u8)),
                _ => false,
            }
    };
    let mut first = [0u32; 16];
    for (i, v) in first.iter_mut().enumerate() {
        *v = bits.take(if anchor(i) { ib - 1 } else { ib });
    }
    let mut second = [0u32; 16];
    if ib2 > 0 {
        for (i, v) in second.iter_mut().enumerate() {
            *v = bits.take(if i == 0 { ib2 - 1 } else { ib2 });
        }
    }
    let weight = |bits: u8, index: u32| match bits {
        2 => WEIGHTS2[index as usize],
        3 => WEIGHTS3[index as usize],
        _ => WEIGHTS4[index as usize],
    };
    let mut out = [[0u8; 4]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        let s = subset_of(i);
        let (e0, e1) = (endpoints[2 * s], endpoints[2 * s + 1]);
        // Colour and alpha weights; modes 4 and 5 index alpha with the second set (swapped by mode 4's selection bit).
        let (wc, wa) = if ib2 == 0 {
            let w = weight(ib, first[i]);
            (w, w)
        } else if selection == 1 {
            (weight(ib2, second[i]), weight(ib, first[i]))
        } else {
            (weight(ib, first[i]), weight(ib2, second[i]))
        };
        let mix = |c: usize, w: u32| (((64 - w) * e0[c] + w * e1[c] + 32) >> 6) as u8;
        *texel = [mix(0, wc), mix(1, wc), mix(2, wc), mix(3, wa)];
        if rotation > 0 {
            texel.swap(3, rotation as usize - 1);
        }
    }
    out
}

/// A `width`-bit endpoint value to 8 bits, its top bits repeated below.
fn expand(v: u32, width: u32) -> u32 {
    let v = v << (8 - width);
    v | v >> width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_6_block_of_one_colour() {
        // Mode 6 (bit 6 set), both endpoints (127, 0, 127, 127) with p-bits 1: every texel (255, 1, 255, 255).
        let mut value: u128 = 1 << 6;
        let mut at = 7;
        let mut put = |v: u128, n: u32| {
            value |= v << at;
            at += n;
        };
        for c in [127, 0, 127, 127] {
            put(c, 7);
            put(c, 7);
        }
        put(1, 1);
        put(1, 1);
        let texels = decode_block(&value.to_le_bytes());
        assert!(texels.iter().all(|t| *t == [255, 1, 255, 255]), "{texels:?}");
        assert_eq!(decode_block(&[0; 16]), [[0; 4]; 16]);
    }
}
