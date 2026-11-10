//! A flat pad: a real built-in BTB stage the game generates itself, a level square of one material for pace and
//! handling measurement. A pad is an ordinary BTB track folder written by code (`track/`) and prepared by
//! [`crate::stage::prepare`] (`stage/`), so physics, drawing, particles and weather go through the existing BTB path
//! (`--btb 41`). Findings: `notes/btb.md` "The flat pad".

use crate::{stage, track, xwrite};
use rbr_formats::{dds, lsp_keys};
use std::io::Write;
use std::path::Path;

/// Raised when the generated files change: pad folders built before are built again.
pub const BUILD: u32 = 1;
/// The pad's squares (m) and their count along each side (a 2 km square).
pub const SQUARES: usize = 100;
pub const SQUARE: f32 = 20.0;
/// The texture tiles every this many metres, so speed and yaw are visible while driving.
pub const TILE: f32 = 10.0;

/// The stage number of the pad of `material`: above the launcher's synthetic BTB range 1_000_000..2_000_000.
pub fn number(material: u8) -> u32 {
    2_000_000 + u32::from(material)
}

/// The `MaterialManager` key name of surface `id`, when `lsp_keys` names one.
pub fn key_name(id: u8) -> Option<&'static str> {
    lsp_keys::KEYS
        .iter()
        .find(|k| k.class == lsp_keys::Class::MaterialManager && k.offset == u32::from(id))
        .map(|k| k.key)
}

/// The pad's `--surface` word: `tarmac` when the material's key starts with `Tarmac`, `snow` when it starts with
/// `Snow`, else `gravel` (as for any BTB stage, [`stage::surface`]).
pub fn surface(id: u8) -> &'static str {
    match key_name(id) {
        Some(name) if name.starts_with("Tarmac") => "tarmac",
        Some(name) if name.starts_with("Snow") => "snow",
        _ => "gravel",
    }
}

/// The pad's directory name of `material`: `pad-tarmac`, `pad-gravel`, `pad-snow`, else `pad-<id>`.
pub fn dir_name(material: u8) -> String {
    match material {
        37 => "pad-tarmac".to_owned(),
        17 => "pad-gravel".to_owned(),
        101 => "pad-snow".to_owned(),
        id => format!("pad-{id}"),
    }
}

/// The material of pad `name`: `Some` for `pad-tarmac`, `pad-gravel`, `pad-snow` and `pad-<id>` (1-255: NGP's ice,
/// 116, has no key in 1.02's table); `None` for anything else.
pub fn material(name: &str) -> Option<u8> {
    match name {
        "pad-tarmac" => Some(37),
        "pad-gravel" => Some(17),
        "pad-snow" => Some(101),
        rest => rest.strip_prefix("pad-")?.parse().ok().filter(|&id: &u8| id != 0),
    }
}

/// The prepared pad: the stage name to pass on as `--stage`, and its `--surface`.
pub struct Prepared {
    pub stage: String,
    pub surface: &'static str,
}

/// Writes (or reuses) the pad of `material` and prepares it: `<player>/pad/<name>-b<BUILD>/` with `track/` (the BTB
/// folder) and `stage/` (the prepared `track-<number>_M.*` files). `maps` is the install's `Maps` folder (the sky
/// stage's retail files). Preparation is safe when several processes start at once on a cold cache: the whole
/// directory is built under a temporary sibling name and renamed into place, never written in place.
pub fn prepare(material: u8, maps: &Path) -> Result<Prepared, String> {
    let home = crate::converted::player_folder().unwrap_or_else(|| std::env::temp_dir().join("rerbr"));
    prepare_in(&home.join("pad"), material, maps)
}

/// [`prepare`] with the pads' folder given.
pub fn prepare_in(base: &Path, material: u8, maps: &Path) -> Result<Prepared, String> {
    let name = dir_name(material);
    let number = number(material);
    let surface = self::surface(material);
    let final_dir = base.join(format!("{name}-b{BUILD}"));
    let stage = stage::stage_name(&final_dir.join("stage"), number);
    // A directory in place is complete: it only ever arrives by the rename below.
    if final_dir.is_dir() {
        return Ok(Prepared { stage, surface });
    }
    std::fs::create_dir_all(base).map_err(|e| format!("{}: {e}", base.display()))?;
    // Unique per process and call: threads of one process and several processes may race.
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = base.join(format!(".tmp-{name}-b{BUILD}-{}-{call}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("track")).map_err(|e| format!("{}: {e}", tmp.display()))?;
    write_track(&tmp.join("track"), material)?;
    let track = track::Track::open(&tmp.join("track"))?;
    stage::prepare(&track, number, maps, &tmp.join("stage"))?;
    // `stage::prepare` names the temporary track folder in `btb.ini`: point it at the final one before the
    // rename (the stamp hashes only names, sizes and times, which the rename keeps).
    let btb = tmp.join("stage").join(stage::SOURCE_FILE);
    let text = std::fs::read_to_string(&btb).map_err(|e| format!("{}: {e}", btb.display()))?;
    let mut rebuilt = String::new();
    for line in text.lines() {
        if line.starts_with("folder=") {
            use std::fmt::Write as _;
            let _ = writeln!(rebuilt, "folder={}", final_dir.join("track").display());
        } else {
            rebuilt.push_str(line);
            rebuilt.push('\n');
        }
    }
    std::fs::write(&btb, rebuilt).map_err(|e| format!("{}: {e}", btb.display()))?;
    // Another process may have finished first: use its directory and drop ours.
    match std::fs::rename(&tmp, &final_dir) {
        Ok(()) => Ok(Prepared { stage, surface }),
        Err(_) if final_dir.is_dir() => {
            let _ = std::fs::remove_dir_all(&tmp);
            Ok(Prepared { stage, surface })
        }
        Err(e) => Err(format!("{} -> {}: {e}", tmp.display(), final_dir.display())),
    }
}

/// The pad's start in the stage frame (y up): on the centreline 500 m in from the `-z` edge, facing `+z`.
const START: [f32; 3] = [0.0, 0.0, -500.0];
/// The driveline runs from this far behind the start to this far ahead of it.
const LEAD: f32 = 50.0;
const AHEAD: f32 = 1450.0;
/// Pace-note distances from the driveline's first point (m): the start, two splits, the finish and the end.
const NOTE_START: f32 = LEAD;
const NOTE_SPLITS: [f32; 2] = [LEAD + 400.0, LEAD + 800.0];
const NOTE_FINISH: f32 = LEAD + 1300.0;
const NOTE_END: f32 = LEAD + 1350.0;

/// Writes the BTB track folder of `material` into `dir`.
fn write_track(dir: &Path, material: u8) -> Result<(), String> {
    let key = key_name(material).unwrap_or("unnamed");
    let physics = surface(material);
    let write = |file: &str, text: String| std::fs::write(dir.join(file), text).map_err(|e| format!("{file}: {e}"));
    write(
        "track.ini",
        format!(
            "[INFO]\nname = Flat pad ({key})\nphysics = {physics}\nauthor = ReRBR (generated)\nversion = {BUILD}\ndate = \ncomment = A level 2 km square of material {material} ({key})\nlength = 2.0\n"
        ),
    )?;
    write(
        "spawn.ini",
        format!(
            "[SPAWN]\nm00=1\nm01=0\nm02=0\nm03=0\nm04=0\nm05=1\nm06=0\nm07=0\nm08=0\nm09=0\nm10=1\nm11=0\nm12={}\nm13={}\nm14={}\nm15=0\nangleX=0\nangleY=0\nangleZ=0\n",
            START[0], START[1], START[2]
        ),
    )?;
    // A straight driveline along +z through the start, every 10 m.
    let first = START[2] - LEAD;
    let last = START[2] + AHEAD;
    let count = ((last - first) / 10.0).round() as usize + 1;
    let mut driveline = format!("[DRIVELINE]\ncount={count}\n");
    for i in 0..count {
        let z = first + 10.0 * i as f32;
        let d = 10.0 * i as f32;
        use std::fmt::Write as _;
        let _ = writeln!(driveline, "K{i}=0, {z:.1}, 0, 0, 1, 0, {d:.1}, 0",);
    }
    write("driveline.ini", driveline)?;
    write("settings.ini", "[SETTINGS]\nsunDirection\t= -0.843359,-0.503774,0.186968\n".to_owned())?;
    write(
        "objectlist.ini",
        "[OBJECTS]\ncount=1\n\n[Object_0]\nfile=ground\nxform=ground\ncloneCount=1\nLODIn=0\nVisible=1\nDrawInstanced=0\nMoveable=0\nShadowCaster=0\nCollisionModel=0\n".to_owned(),
    )?;
    let section = "pad\\ground";
    write(
        "materials.ini",
        format!(
            "[MATERIALS]\ncount=1\nMAT_0={section}\n\n[{section}]\neffectName=BTB_Diffuse\ntechniqueName=Default\nambient=0.4,0.4,0.4,1.0\nSpecularPower=4\nSpecularColor=0,0,0\ndiffuseTexture=pad.dds\n"
        ),
    )?;
    // One map, every cell the id; a `_Dry` id's damp and wet rows hold id+1 and id+2.
    let mut mat = "[MAPS]\ncount=1\n[MAP0]\ntexture=pad.dds\n".to_owned();
    {
        use std::fmt::Write as _;
        let row = |id: u8| vec![id.to_string(); 16].join(" ");
        let dry = key.ends_with("_Dry");
        for r in 0..16 {
            let _ = writeln!(mat, "data{r}={}", row(material));
        }
        if dry {
            for r in 0..16 {
                let _ = writeln!(mat, "damp{r}={}", row(material + 1));
            }
            for r in 0..16 {
                let _ = writeln!(mat, "wet{r}={}", row(material + 2));
            }
        }
    }
    write("mat.ini", mat)?;
    // Pace notes: start, two splits, finish, end.
    let mut notes = "[PACENOTES]\ncount=5\n".to_owned();
    {
        use std::fmt::Write as _;
        for (i, (tag, d)) in
            [(0x15, NOTE_START), (0x17, NOTE_SPLITS[0]), (0x17, NOTE_SPLITS[1]), (0x16, NOTE_FINISH), (0x18, NOTE_END)]
                .into_iter()
                .enumerate()
        {
            let _ = writeln!(notes, "\n[P{i}]\ntype={tag}\ndistance={d:.3}\nflag=0");
        }
    }
    write("pacenotes.ini", notes)?;
    write_objects(dir)?;
    write_textures(dir, material)?;
    Ok(())
}

/// The ground mesh: a level 2 km square at height 0, two triangles per 20 m square, with UVs tiling every 10 m.
/// One object with `CollisionModel=0` and `Visible=1` is both drawn and in `col.col` (`col::gather`); the pad has
/// no `ksh` meshes (no `CollisionModel` 1/2 object), so the `(3,0x16)` chunk is empty.
type Ground = (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u32; 3]>);
fn ground() -> Ground {
    let half = SQUARES as f32 * SQUARE / 2.0;
    let n = SQUARES + 1;
    let mut positions = Vec::with_capacity(n * n);
    let mut uvs = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let x = -half + i as f32 * SQUARE;
            let z = -half + j as f32 * SQUARE;
            positions.push([x, 0.0, z]);
            uvs.push([(x + half) / TILE, (z + half) / TILE]);
        }
    }
    // File faces point up (`(b-a)x(c-a)` is +y), as the converter's ground does; `gather` records them `(v0,v2,v1)`.
    let mut triangles = Vec::with_capacity(SQUARES * SQUARES * 2);
    for j in 0..SQUARES {
        for i in 0..SQUARES {
            let a = (j * n + i) as u32;
            let b = (j * n + i + 1) as u32;
            let c = ((j + 1) * n + i) as u32;
            let d = ((j + 1) * n + i + 1) as u32;
            triangles.push([a, c, d]);
            triangles.push([a, d, b]);
        }
    }
    (positions, uvs, triangles)
}

fn write_objects(dir: &Path) -> Result<(), String> {
    let (positions, uvs, triangles) = ground();
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    let bytes = xwrite::write(&xwrite::XMesh {
        positions: &positions,
        normals: &normals,
        uvs: &uvs,
        triangles: &triangles,
        material: "pad\\ground",
    });
    let identity: Vec<u8> = [1f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let path = dir.join("Objects.zip");
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("Objects/ground.x", options).map_err(|e| format!("Objects.zip: {e}"))?;
    zip.write_all(&bytes).map_err(|e| e.to_string())?;
    zip.start_file("Objects/ground.xform", options).map_err(|e| format!("Objects.zip: {e}"))?;
    zip.write_all(&identity).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| format!("Objects.zip: {e}"))?;
    Ok(())
}

/// One `BTB_Diffuse` texture: a flat base colour by surface with thin darker grid lines at the tile edges.
fn write_textures(dir: &Path, material: u8) -> Result<(), String> {
    let (base, line) = match surface(material) {
        "tarmac" => ([58, 58, 58, 255], [38, 38, 38, 255]),
        "gravel" => ([184, 154, 106, 255], [140, 115, 75, 255]),
        "snow" => ([242, 242, 245, 255], [200, 200, 210, 255]),
        _ => ([128, 128, 128, 255], [90, 90, 90, 255]),
    };
    const SIZE: usize = 64;
    let mut pixels = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let edge = x < 2 || y < 2 || x >= SIZE - 2 || y >= SIZE - 2;
            pixels.extend(if edge { line } else { base });
        }
    }
    let bytes = dds::rgba8_file(SIZE as u32, SIZE as u32, &pixels);
    let path = dir.join("Textures.zip");
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("Textures/pad.dds", options).map_err(|e| format!("Textures.zip: {e}"))?;
    zip.write_all(&bytes).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| format!("Textures.zip: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_tmp(material: u8, tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rbr-pad-{material}-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_track(&dir, material).unwrap();
        dir
    }

    #[test]
    fn names_map_to_materials() {
        assert_eq!(material("pad-tarmac"), Some(37));
        assert_eq!(material("pad-gravel"), Some(17));
        assert_eq!(material("pad-snow"), Some(101));
        assert_eq!(material("pad-37"), Some(37));
        assert_eq!(material("pad-101"), Some(101));
        assert_eq!(material("pad-1"), Some(1));
        assert_eq!(material("pad-0"), None);
        assert_eq!(material("pad-"), None);
        assert_eq!(material("pad-tarmacx"), None);
        assert_eq!(material("track-85_M"), None);
        assert_eq!(material("pad-256"), None);
        // NGP's ice has no key in 1.02's table and runs on the gravel tyre unless `--surface` says otherwise.
        assert_eq!((key_name(116), material("pad-116"), surface(116)), (None, Some(116), "gravel"));
        assert_eq!(material("pad-65"), Some(65));
    }

    #[test]
    fn numbers_stay_above_the_launchers_synthetic_range() {
        assert_eq!(number(37), 2_000_037);
        assert_eq!(number(255), 2_000_255);
        for id in [1u8, 17, 37, 101, 255] {
            let (n, l) = rbr_formats::stage_files::parts(&format!("track-{}_M", number(id))).unwrap();
            assert_eq!((n, l), (number(id), 'M'));
        }
    }

    #[test]
    fn surfaces_follow_the_key_prefix() {
        assert_eq!(surface(37), "tarmac");
        assert_eq!(surface(17), "gravel");
        assert_eq!(surface(101), "snow");
        assert_eq!(surface(65), "tarmac");
        assert_eq!(surface(1), "gravel");
        assert_eq!(key_name(37), Some("Tarmac_Fine_Clean_Dry"));
        assert_eq!(key_name(17), Some("Gravel_Fine_Shallow_Dry"));
        assert_eq!(key_name(101), Some("SnowOnGravel_Shallow"));
    }

    #[test]
    fn the_written_folder_opens_as_a_track() {
        for id in [37u8, 17, 101] {
            let dir = write_tmp(id, "open");
            assert!(track::is_track_folder(&dir), "{id}");
            let t = track::Track::open(&dir).unwrap();
            assert_eq!(surface(id), stage::surface(&t).unwrap());
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn the_pad_draws_and_collides_on_one_mesh() {
        let dir = write_tmp(37, "mesh");
        let t = track::Track::open(&dir).unwrap();
        let objects = t.objects().unwrap();
        assert_eq!(objects.len(), 1);
        let o = &objects[0];
        assert_eq!((o.visible, o.collision_model), (Some(true), Some(0)));
        let scene = crate::scene::Scene::load(&dir).unwrap();
        assert_eq!(scene.meshes.len(), 1);
        assert_eq!(scene.objects.len(), 1);
        assert!(scene.objects[0].visible);
        let material = &scene.materials[scene.meshes[0].material.unwrap()];
        assert_eq!(
            material.effect,
            crate::scene::Effect { bump: false, specular: false, multiply_add: false, vert_lerp: false, sky: false }
        );
        let bytes = scene.textures.read(material.diffuse.as_deref().unwrap()).unwrap();
        assert!(rbr_formats::dds::parse(&bytes).is_ok());
        assert_eq!(crate::scene::alpha(&bytes), crate::scene::Alpha::Opaque);
        // The tiling UVs stay above 1 and still build: every cell reads the same id.
        assert!(scene.meshes[0].uvs[0].iter().any(|uv| uv[0] > 1.0));
        let bytes = crate::col::build(&t).unwrap();
        let file = rbr_formats::col::parse(&bytes).unwrap();
        let faces: Vec<_> = file
            .cells
            .iter()
            .flat_map(|c| c.tree.triangles.iter().map(|t| t.vertices.map(|i| c.vertices[i as usize])))
            .collect();
        assert_eq!(faces.len(), 20000);
        assert!(faces.iter().flatten().all(|v| v[2] == 0.0));
        let (ksh_count, ksh) = crate::ksh::build(&t, crate::D3dx::Windows).unwrap();
        assert_eq!((ksh_count, ksh.len()), (0, 0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn surface_maps_carry_the_id_with_damp_and_wet_for_dry() {
        for (id, damp) in [(37u8, true), (101, false)] {
            let dir = write_tmp(id, "maps");
            let t = track::Track::open(&dir).unwrap();
            let maps = t.surface_maps().unwrap();
            assert_eq!(maps.count, 1);
            assert!(maps.maps[0].iter().all(|&c| c == id));
            let want = if damp { id + 1 } else { id };
            assert!(maps.damp[0].iter().all(|&c| c == want), "{id}");
            let want = if damp { id + 2 } else { id };
            assert!(maps.wet[0].iter().all(|&c| c == want), "{id}");
            let file = rbr_formats::mat::parse(&crate::classic::mat(&maps)).unwrap();
            let first: Vec<u8> = file.entries.iter().map(|e| e.runs[0].cells[0]).collect();
            assert_eq!(
                first,
                if damp { vec![id, id, id, id + 1, id + 1, id + 1, id + 2, id + 2, id + 2] } else { vec![id; 9] }
            );
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn pace_notes_carry_start_splits_finish_and_end() {
        let dir = write_tmp(37, "notes");
        let t = track::Track::open(&dir).unwrap();
        let notes = t.pace_notes().unwrap();
        assert_eq!(notes.count, 5);
        let tags: Vec<u32> = notes.notes.iter().map(|n| n.tag).collect();
        assert_eq!(tags, [0x15, 0x17, 0x17, 0x16, 0x18]);
        let ds: Vec<f32> = notes.notes.iter().map(|n| n.distance as f32).collect();
        assert_eq!(ds, [NOTE_START, NOTE_SPLITS[0], NOTE_SPLITS[1], NOTE_FINISH, NOTE_END]);
        // The driveline runs from 50 m behind the start to 1450 m ahead of it.
        let line = t.driveline().unwrap();
        assert_eq!(line.points.len(), 151);
        assert!((line.points[0].position[1] - f64::from(START[2] - LEAD)).abs() < 1e-6);
        assert!((line.points.last().unwrap().position[1] - f64::from(START[2] + AHEAD)).abs() < 1e-6);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
