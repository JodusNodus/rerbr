//! The `CACHE/ksh.ksh` collision meshes the RSF plugin `rbr_rx.dll` writes (`WriteKSH` 0x1001dee0), which become the
//! `(3, 0x16)` chunk of `trk.trk`. Rules and their evidence: `notes/btb.md` "Collision meshes (ksh)".
//!
//! One mesh per `objectlist.ini` object with `CollisionModel` 1 or 2: the name `.ksh`, six flag bytes (the first is
//! `CollisionModel == 1`), a zero `u32`, eight box corners, twelve triangles, then one placement per clone (the
//! `xform` file's matrix count). The box is the object's `colBox`, or the `.x` mesh's bounding box without one.
//!
//! The plugin computes with D3DX (`d3dx9_30.dll`) in `f32`, and the library is not the same everywhere: a cache built
//! under Windows uses the native DLL (SSE2 `rsqrtss` quaternion normalisation), one built under Wine uses Wine's
//! own (plain division). [`Math`] selects which one the placement rotation reproduces.

use crate::track::Track;
use crate::{xfile, xform};
use rbr_formats::rbz;
use std::io::Read;

/// Which `d3dx9_30.dll` the cache was built with; only the placement quaternion differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Math {
    /// The native Windows DLL: x87 `D3DXQuaternionRotationMatrix`, SSE2 `D3DXQuaternionNormalize` (`rsqrtss` with one
    /// Newton step). Bit-exact where the CPU's `rsqrtss` is Intel's (see [`rsqrtss`]).
    Native,
    /// Wine's builtin DLL: `D3DXQuaternionRotationMatrix` of its `math.c`, normalised by dividing by the length.
    Wine,
}

/// The 12 triangles of the box, as indexes into its 8 corners (corner bit 0 is x, bit 1 y, bit 2 z), in the order
/// the plugin writes them (`DAT_1005e5a8`); each is written `a, c, b` after a zero tag byte.
const FACES: [[u32; 3]; 12] = [
    [0, 2, 3],
    [3, 1, 0],
    [4, 5, 7],
    [7, 6, 4],
    [0, 1, 5],
    [5, 4, 0],
    [1, 3, 7],
    [7, 5, 1],
    [3, 2, 6],
    [6, 7, 3],
    [2, 0, 4],
    [4, 6, 2],
];

/// The `ksh.ksh` bytes and their mesh count (`CollisionModel` 1 or 2 objects), for [`crate::classic::trk`].
///
/// The first word of each placement is the address of its matrix in the plugin's heap, different on every run: it is
/// written as 0 here (the game keeps no use of it, and no byte of the file depends on it).
pub fn build(track: &Track, math: Math) -> Result<(u32, Vec<u8>), String> {
    let objects = track.objects()?;
    let mut zip = None;
    let mut out = Vec::new();
    let mut count = 0;
    for object in objects.iter().filter(|o| matches!(o.collision_model, Some(1 | 2))) {
        if zip.is_none() {
            let path = rbr_formats::stage_files::in_dir(&track.folder, "Objects.zip")
                .ok_or_else(|| format!("{}: no Objects.zip", track.folder.display()))?;
            zip = Some(rbz::open(&path).map_err(|e| format!("{}: {e}", path.display()))?);
        }
        let zip = zip.as_mut().expect("opened above");
        let placements = match read(zip, &format!("Objects/{}.xform", object.xform)) {
            Ok(bytes) => xform::parse(&bytes).map_err(|e| format!("{}.xform: {e}", object.xform))?,
            // `ReadClones`: a missing file is one clone with the identity matrix.
            Err(_) => vec![IDENTITY],
        };
        let corners = match object.col_box {
            Some(extents) => box_corners(extents),
            None => {
                let bytes = read(zip, &format!("Objects/{}.x", object.file))?;
                mesh_corners(&object.file, &bytes)?
            }
        };
        mesh(&mut out, object.collision_model == Some(1), &corners, &placements, math);
        count += 1;
    }
    Ok((count, out))
}

const IDENTITY: [f32; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

fn read<R: Read + std::io::Seek>(zip: &mut rbz::ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    rbz::by_name(zip, name)
        .map_err(|e| format!("{name}: {e}"))?
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(bytes)
}

/// The eight corners of a `colBox` `(a, b, c)`: half extents `a/2` (x), `c/2` (y) and a z from `-0.1 * b/2` (the
/// f32 of 0.1) to `b` (`2 * b/2`).
pub fn box_corners([a, b, c]: [f32; 3]) -> [[f32; 3]; 8] {
    let (ex, ey, ez) = (a * 0.5, c * 0.5, b * 0.5);
    let low = -ez * 0.1f32;
    let high = ez + ez;
    std::array::from_fn(|i| {
        [if i & 1 == 0 { -ex } else { ex }, if i & 2 == 0 { -ey } else { ey }, if i & 4 == 0 { low } else { high }]
    })
}

/// The corners of the `.x` mesh's bounding box, centred (`corner - 0.5 * (min + max)`, corner bit 0 x, 1 y, 2 z of the
/// file's frame) and written with y and z swapped, after the loader's shift of every nonzero float by one ulp.
fn mesh_corners(name: &str, bytes: &[u8]) -> Result<[[f32; 3]; 8], String> {
    let fail = |e: xfile::Error| format!("{name}.x: {e}");
    let file = xfile::parse(bytes).map_err(fail)?;
    let root = *file.objects_of_type("Mesh").first().ok_or_else(|| format!("{name}.x: no Mesh"))?;
    let mesh = xfile::Mesh::decode(root, &file).map_err(fail)?;
    let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &mesh.positions {
        for a in 0..3 {
            let v = d3dx_load(p[a]);
            min[a] = min[a].min(v);
            max[a] = max[a].max(v);
        }
    }
    let centre: [f32; 3] = std::array::from_fn(|a| 0.5 * (min[a] + max[a]));
    Ok(std::array::from_fn(|i| {
        let corner: [f32; 3] = std::array::from_fn(|a| (if i >> a & 1 == 0 { min[a] } else { max[a] }) - centre[a]);
        [corner[0], corner[2], corner[1]]
    }))
}

/// A position as `D3DXLoadMeshFromX` returns it: one unit of the bit pattern below the file's value (towards zero),
/// zero kept (`notes/btb.md`).
fn d3dx_load(v: f32) -> f32 {
    if v == 0.0 {
        v
    } else {
        f32::from_bits(v.to_bits() - 1)
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32s(out: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn mesh(out: &mut Vec<u8>, dynamic: bool, corners: &[[f32; 3]; 8], placements: &[[f32; 16]], math: Math) {
    out.extend_from_slice(b".ksh\0");
    out.extend_from_slice(&[u8::from(dynamic), 0, 0, 0, 0, 0]);
    put_u32(out, 0);
    put_u32(out, 8);
    for corner in corners {
        put_f32s(out, corner);
    }
    put_u32(out, FACES.len() as u32);
    for [a, b, c] in FACES {
        out.push(0);
        put_u32(out, a);
        put_u32(out, c);
        put_u32(out, b);
    }
    put_u32(out, placements.len() as u32);
    for m in placements {
        put_u32(out, 0);
        // y and z swap: z-up BTB frame to the classic y-up one.
        put_f32s(out, &[m[12], m[14], m[13]]);
        put_f32s(out, &[length(&m[0..3]), length(&m[8..11]), length(&m[4..7])]);
        let [x, y, z, w] = quaternion(m, math);
        put_f32s(out, &[x, z, y, -w]);
    }
}

/// `sqrt(x*x + y*y + z*z)` in `f32` steps (the x87 runs at 24-bit precision under Direct3D).
fn length(v: &[f32]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// `D3DXQuaternionRotationMatrix` of the row-major matrix (scales not removed) then `D3DXQuaternionNormalize`.
pub fn quaternion(m: &[f32; 16], math: Math) -> [f32; 4] {
    match math {
        Math::Native => normalize_native(rotation_native(m)),
        Math::Wine => normalize_wine(rotation_wine(m)),
    }
}

/// Native `d3dx9_30` x87 routine (0x443258): quarter-trace form; on a trace `<= 0` the largest of `m00 - m11 - m22`,
/// `m11 - m00 - m22`, `m22 - (m11 + m00)` (first wins ties) picks the axis.
fn rotation_native(m: &[f32; 16]) -> [f32; 4] {
    let at = |r: usize, c: usize| m[r * 4 + c];
    let trace = (at(1, 1) + at(0, 0)) + at(2, 2);
    let mut q = [0.0f32; 4];
    if trace > 0.0 {
        let r = (trace + 1.0).sqrt() * 0.5;
        let inv = 0.25 / r;
        q[3] = r;
        q[0] = (at(1, 2) - at(2, 1)) * inv;
        q[1] = (at(2, 0) - at(0, 2)) * inv;
        q[2] = (at(0, 1) - at(1, 0)) * inv;
    } else {
        let d = [(at(0, 0) - at(1, 1)) - at(2, 2), (at(1, 1) - at(0, 0)) - at(2, 2), at(2, 2) - (at(1, 1) + at(0, 0))];
        let mut i = usize::from(d[0] < d[1]);
        if d[i] < d[2] {
            i = 2;
        }
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        let r = (d[i] + 1.0).sqrt() * 0.5;
        let inv = 0.25 / r;
        q[i] = r;
        q[j] = (at(j, i) + at(i, j)) * inv;
        q[k] = (at(k, i) + at(i, k)) * inv;
        q[3] = (at(j, k) - at(k, j)) * inv;
    }
    q
}

/// Native SSE2 `D3DXQuaternionNormalize` (0x6117fd): `rsqrtss` refined by one Newton step, components multiplied by it.
fn normalize_native(q: [f32; 4]) -> [f32; 4] {
    let l = (q[0] * q[0] + q[1] * q[1]) + (q[2] * q[2] + q[3] * q[3]);
    let r0 = rsqrtss(l);
    let s = (3.0 - l * (r0 * r0)) * 0.5 * r0;
    q.map(|v| v * s)
}

/// Intel's `rsqrtss` for a normal positive `x`: a table over the exponent's parity and the top 10 mantissa bits, whose
/// entries are `1 / sqrt` of the interval's centre rounded to 12 mantissa bits (checked against the instruction on an
/// i9-13900HX for 200000 random inputs and every table entry; the caches built on Windows agree with it, AMD's table
/// differs).
pub fn rsqrtss(x: f32) -> f32 {
    let bits = x.to_bits();
    let exponent = (bits >> 23) as i32 - 127;
    let top = f64::from((bits & 0x007f_ffff) >> 13);
    let centre = (1.0 + (top + 0.5) / 1024.0) * 2f64.powi(exponent);
    let y = 1.0 / centre.sqrt();
    let e = y.log2().floor() as i32;
    let mantissa = ((y / 2f64.powi(e) - 1.0) * 4096.0).round_ties_even();
    ((1.0 + mantissa / 4096.0) * 2f64.powi(e)) as f32
}

/// Wine's `D3DXQuaternionRotationMatrix` (`dlls/d3dx9_36/math.c`).
fn rotation_wine(m: &[f32; 16]) -> [f32; 4] {
    let at = |r: usize, c: usize| m[r * 4 + c];
    let trace = at(0, 0) + at(1, 1) + at(2, 2) + 1.0;
    if trace > 1.0 {
        let s = 2.0 * trace.sqrt();
        return [(at(1, 2) - at(2, 1)) / s, (at(2, 0) - at(0, 2)) / s, (at(0, 1) - at(1, 0)) / s, 0.25 * s];
    }
    let mut max = 0;
    for i in 1..3 {
        if at(i, i) > at(max, max) {
            max = i;
        }
    }
    match max {
        0 => {
            let s = 2.0 * (1.0 + at(0, 0) - at(1, 1) - at(2, 2)).sqrt();
            [0.25 * s, (at(0, 1) + at(1, 0)) / s, (at(0, 2) + at(2, 0)) / s, (at(1, 2) - at(2, 1)) / s]
        }
        1 => {
            let s = 2.0 * (1.0 + at(1, 1) - at(0, 0) - at(2, 2)).sqrt();
            [(at(0, 1) + at(1, 0)) / s, 0.25 * s, (at(1, 2) + at(2, 1)) / s, (at(2, 0) - at(0, 2)) / s]
        }
        _ => {
            let s = 2.0 * (1.0 + at(2, 2) - at(0, 0) - at(1, 1)).sqrt();
            [(at(0, 2) + at(2, 0)) / s, (at(1, 2) + at(2, 1)) / s, 0.25 * s, (at(0, 1) - at(1, 0)) / s]
        }
    }
}

/// Wine's `D3DXQuaternionNormalize`: each component divided by the `f32` length.
fn normalize_wine(q: [f32; 4]) -> [f32; 4] {
    let length = (((q[0] * q[0] + q[1] * q[1]) + q[2] * q[2]) + q[3] * q[3]).sqrt();
    q.map(|v| v / length)
}
