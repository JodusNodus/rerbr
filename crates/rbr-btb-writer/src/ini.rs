//! The stage's small text files, as the converters write them: `materials.ini`, `mat.ini`, `track.ini`,
//! `driveline.ini`, `pacenotes.ini`, `spawn.ini`, and the provenance file.
use crate::bank::TextureZip;
use crate::materials;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

/// `text` as `folder/file`.
pub fn write(folder: &Path, file: &str, text: impl AsRef<[u8]>) -> Result<(), String> {
    std::fs::write(folder.join(file), text).map_err(|e| format!("{file}: {e}"))
}

/// `materials.ini`: the list of the section `names`, then the `sections`.
pub fn materials_ini(names: &[String], sections: &str) -> String {
    let mut list = format!("[MATERIALS]\ncount={}\n", names.len());
    for (i, n) in names.iter().enumerate() {
        let _ = writeln!(list, "MAT_{i}={n}");
    }
    format!("{list}\n{sections}")
}

/// The `objectlist.ini` keys of a collision mesh.
pub const COLLISION_ENTRIES: &str = "LODIn=0\nVisible=0\nShadowCaster=0\nCollisionModel=0\nCollisionOnly=1\n";

/// The material of the collision surface `id`: its name.
pub fn collision_material(id: u8) -> String {
    format!("collision\\surface_{id}")
}

/// The collision surfaces `ids` as materials (a flat texture each, the id as its grey, in `sections` and `names`) and
/// the `[MAP<n>]` entries of `mat.ini` ([`mat_ini`]), each surface under its three weathers' ids.
pub fn collision_surfaces(
    ids: &BTreeSet<u8>,
    bank: &mut TextureZip,
    sections: &mut String,
    names: &mut Vec<String>,
    weathers: impl Fn(u8) -> [u8; 3],
) -> Result<String, String> {
    let mut maps = String::new();
    for (index, id) in ids.iter().enumerate() {
        let texture = bank.put(crate::textures::flat([f32::from(*id) / 255.; 4]), "dds")?;
        let name = collision_material(*id);
        materials::section(sections, &name, &texture, false);
        names.push(name);
        let _ = writeln!(maps, "[MAP{index}]\ntexture={texture}");
        for (key, surface) in ["data", "damp", "wet"].into_iter().zip(weathers(*id)) {
            for row in 0..16 {
                let _ = writeln!(maps, "{key}{row}={}", vec![surface.to_string(); 16].join(" "));
            }
        }
    }
    Ok(maps)
}

/// `mat.ini` from the `maps` of [`collision_surfaces`], `count` surfaces.
pub fn mat_ini(count: usize, maps: &str) -> String {
    format!("[MAPS]\ncount={count}\n{maps}")
}

/// What `track.ini`'s `[INFO]` says about a stage.
pub struct TrackInfo<'a> {
    pub name: &'a str,
    pub physics: &'a str,
    pub author: &'a str,
    pub version: &'a str,
    /// The timed length, km.
    pub length_km: f64,
    pub comment: &'a str,
    /// `track.ini`'s `splashscreen`: a file of the stage's folder.
    pub splashscreen: Option<&'a str>,
    pub country: Option<&'a str>,
    /// Latitude and longitude, degrees.
    pub place: Option<[f64; 2]>,
    pub circuit: bool,
}

impl TrackInfo<'_> {
    pub fn ini(&self) -> String {
        let mut ini = format!(
            "[INFO]\nname={}\nphysics={}\nauthor={}\nversion={}\nlength={:.3}\ncomment={}\n",
            self.name, self.physics, self.author, self.version, self.length_km, self.comment
        );
        if let Some(file) = self.splashscreen {
            let _ = writeln!(ini, "splashscreen={file}");
        }
        if let Some(code) = self.country {
            let _ = writeln!(ini, "countrycode={code}");
        }
        if let Some([lat, lon]) = self.place {
            let _ = write!(ini, "latitude={lat:.4}\nlongitude={lon:.4}\n");
        }
        if self.circuit {
            ini.push_str("circuit=1\n");
        }
        ini
    }
}

/// `driveline.ini`: `points` in the stage's frame about `origin`, each with its heading and arc length `at`. A ring's
/// last point repeats its first: its neighbours wrap around the seam.
pub fn driveline_ini(points: &[[f64; 3]], at: &[f64], origin: [f64; 2], ring: bool) -> String {
    let n = points.len();
    let mut ini = format!("[DRIVELINE]\ncount={n}\n");
    for (i, p) in points.iter().enumerate() {
        let (a, b) = if ring {
            (points[(i + n - 2) % (n - 1)], points[(i + 1) % (n - 1)])
        } else {
            (points[i.saturating_sub(1)], points[(i + 1).min(n - 1)])
        };
        let length = crate::road::distance(a, b).max(1e-9);
        let t = std::array::from_fn::<_, 3, _>(|k| (b[k] - a[k]) / length);
        let _ = writeln!(
            ini,
            "K{i}={}, {}, {}, {}, {}, {}, {}, 0",
            p[0] - origin[0],
            p[1] - origin[1],
            p[2],
            t[0],
            t[1],
            t[2],
            at[i]
        );
    }
    ini
}

/// `pacenotes.ini`: `notes` as `(tag, flags, distance)`, sorted by distance.
pub fn pacenotes_ini(notes: &mut [(u32, u32, f64)]) -> String {
    notes.sort_by(|a, b| a.2.total_cmp(&b.2));
    let mut ini = format!("[PACENOTES]\ncount={}\n", notes.len());
    for (i, (tag, flag, s)) in notes.iter().enumerate() {
        let _ = writeln!(ini, "\n[P{i}]\ntype={tag}\nflag={flag}\ndistance={s:.4}");
    }
    ini
}

/// `spawn.ini`: the car at `position`, facing `tangent` (in the stage's frame about `origin`).
pub fn spawn_ini(position: [f64; 3], tangent: [f64; 3], origin: [f64; 2]) -> String {
    let mut ini = String::from("[SPAWN]\n");
    for i in 0..16 {
        let v = match i {
            0 | 5 | 10 => 1.,
            12 => position[0] - origin[0],
            13 => position[2],
            14 => position[1] - origin[1],
            _ => 0.,
        };
        let _ = writeln!(ini, "m{i:02}={v}");
    }
    let _ = writeln!(ini, "angleX=0\nangleY=0\nangleZ={}", tangent[0].atan2(tangent[1]));
    ini
}

/// The converter's provenance file `file`, `json` pretty printed.
pub fn write_provenance(folder: &Path, file: &str, json: &serde_json::Value) -> Result<(), String> {
    write(folder, file, serde_json::to_string_pretty(json).map_err(|e| e.to_string())?)
}
