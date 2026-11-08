//! `beamng-stage`: converts a BeamNG level to a BTB stage folder. `--info` prints what the level holds.

use rbr_beamng::world::{self, Detail, Grid, Triangle};
use rbr_beamng::{cdae, convert, course, level, materials, race::Race, zipfs::Archive};
use std::path::PathBuf;

fn main() {
    if let Err(e) = run() {
        eprintln!("beamng-stage: {e}");
        std::process::exit(1);
    }
}

const USAGE: &str = "usage: beamng-stage --zip LEVEL.zip --mission MISSION_FOLDER [--out DIR] [--name NAME] \
[--skip TEXT]... [--draw-corridor M] [--reach TEXT=M]... [--lod TEXT=M]... [--collision-corridor M]
       [--detail-lod M] [--prepare DIR --maps DIR]
       beamng-stage --track FOLDER --prepare DIR --maps DIR
       beamng-stage --zip LEVEL.zip --shapes | --shape PATH | --mission M --probe SHAPE_TEXT

Converts a BeamNG level mod (your own copy) and one of its time-trial missions to a BTB stage folder,
<out>/RX_CONTENT/Tracks/<name>/, which the launcher lists. The output is for your own use: level mods'
licences usually forbid sharing modified versions. --out defaults to the launcher's files folder's
`converted` (RERBR_HOME, else the platform's data folder: ~/.local/share/rerbr on Linux). --skip leaves
shapes whose path contains TEXT undrawn (people, animated rides). --draw-corridor M draws shapes within M metres of
the driveline, --reach TEXT=M those whose path contains TEXT within M instead; --lod TEXT=M draws them only up to M
metres from the eye (the last matching rule wins).";

/// The launcher's files folder (`rbr-launcher`'s `home::dir`): `$RERBR_HOME`, else the platform's user data folder.
fn launcher_home() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("RERBR_HOME") {
        return Some(dir);
    }
    match std::env::consts::OS {
        "windows" => var("APPDATA").map(|d| d.join("ReRBR")),
        "macos" => var("HOME").map(|h| h.join("Library/Application Support/ReRBR")),
        _ => var("XDG_DATA_HOME")
            .filter(|d| d.is_absolute())
            .or_else(|| var("HOME").map(|h| h.join(".local/share")))
            .map(|d| d.join("rerbr")),
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut zip = None;
    let mut shapes = false;
    let mut dump = None;
    let mut mission = None;
    let mut probe = None;
    let mut out = None;
    let mut name = None;
    let mut skip = Vec::new();
    let mut reach = Vec::new();
    let mut lod = Vec::new();
    let mut prepare_dir = None;
    let mut maps = None;
    let mut track = None;
    let mut numbers: Vec<(String, f64)> = Vec::new();
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--zip" => zip = Some(PathBuf::from(value()?)),
            "--shapes" => shapes = true,
            "--shape" => dump = Some(value()?),
            "--mission" => mission = Some(value()?),
            "--probe" => probe = Some(value()?),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--name" => name = Some(value()?),
            "--skip" => skip.push(value()?),
            "--reach" | "--lod" => {
                let v = value()?;
                let (text, m) = v.rsplit_once('=').ok_or_else(|| format!("{a} {v}: needs TEXT=METRES"))?;
                let m: f64 = m.parse().map_err(|_| format!("{a} {v}: not a number: {m}"))?;
                if a == "--reach" { &mut reach } else { &mut lod }.push((text.to_owned(), m));
            }
            "--prepare" => prepare_dir = Some(PathBuf::from(value()?)),
            "--maps" => maps = Some(PathBuf::from(value()?)),
            "--track" => track = Some(PathBuf::from(value()?)),
            "--draw-corridor" | "--collision-corridor" | "--detail-lod" => {
                let v = value()?;
                numbers.push((a.clone(), v.parse().map_err(|_| format!("{a}: not a number: {v}"))?));
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {a}\n{USAGE}")),
        }
    }
    if let Some(folder) = track {
        return prepare(&folder, prepare_dir.ok_or("--track needs --prepare DIR")?, maps);
    }
    let zip = zip.ok_or_else(|| format!("--zip LEVEL.zip is required\n{USAGE}"))?;
    if shapes || dump.is_some() || probe.is_some() {
        let mut archive = Archive::open(&zip)?;
        if shapes {
            list_shapes(&mut archive);
        }
        if let Some(visual) = probe {
            probe_road(&mut archive, mission.as_deref().ok_or("--probe needs --mission")?, &visual)?;
        }
        if let Some(path) = dump {
            dump_shape(&cdae::parse(&archive.read(&path)?)?);
        }
        return Ok(());
    }
    let mission = mission.ok_or_else(|| format!("--mission is required\n{USAGE}"))?;
    let out = match out {
        Some(o) => o,
        None => launcher_home().ok_or("no files folder (set --out or RERBR_HOME)")?.join("converted"),
    };
    let mut options = convert::Options::new(&mission, &out);
    options.name = name;
    options.skip = skip;
    options.reach = reach;
    options.lod = lod;
    for (k, v) in numbers {
        match k.as_str() {
            "--draw-corridor" => options.draw_corridor = v,
            "--collision-corridor" => options.collision_corridor = v,
            _ => options.detail_lod = v,
        }
    }
    let report = convert::convert(&zip, &options)?;
    println!("{}", report.folder.display());
    match prepare_dir {
        Some(dir) => prepare(&report.folder, dir, maps),
        None => Ok(()),
    }
}

/// A test aid: the stage files the launcher's Start builds from `folder`, under a fixed number, into `dir`.
fn prepare(folder: &std::path::Path, dir: PathBuf, maps: Option<PathBuf>) -> Result<(), String> {
    let maps = maps.ok_or("--prepare needs --maps (the install's Maps folder)")?;
    let track = rbr_btb::track::Track::open(folder)?;
    let started = std::time::Instant::now();
    let name = rbr_btb::stage::prepare(&track, 1_999_999, &maps, &dir)?;
    println!("{name} ({:.1} s)", started.elapsed().as_secs_f64());
    Ok(())
}

fn list_shapes(archive: &mut Archive) {
    let (mut ok, mut failed) = (0, 0);
    for name in archive.names().into_iter().filter(|n| n.to_lowercase().ends_with(".cdae")) {
        match archive.read(&name).and_then(|b| cdae::parse(&b)) {
            Ok(s) => {
                ok += 1;
                let tris: usize = s.meshes.iter().map(|m| m.indices.len() / 3).sum();
                println!(
                    "{name}: {} objects, {} meshes, {} details, {} materials, ~{tris} triangles",
                    s.objects.len(),
                    s.meshes.len(),
                    s.details.len(),
                    s.materials.len()
                );
            }
            Err(e) => {
                failed += 1;
                println!("{name}: FAILED {e}");
            }
        }
    }
    println!("{ok} read, {failed} failed");
}

fn dump_shape(s: &cdae::Shape) {
    for d in &s.details {
        println!("detail {} sub {} slot {} size {}", s.name(d.name), d.sub_shape, d.object_detail, d.size);
    }
    for (i, m) in s.materials.iter().enumerate() {
        println!("material {i} {m}");
    }
    for (i, o) in s.objects.iter().enumerate() {
        let t = s.node_transform(o.node as usize);
        let moved = t != cdae::IDENTITY;
        for (mesh, d) in s.object_meshes(i) {
            let v = s.mesh_vertices(mesh);
            let m = &s.meshes[mesh];
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            for p in &v.verts {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            let mut mats: Vec<usize> = m.primitives.iter().filter_map(|p| p.material_index()).collect();
            mats.sort();
            mats.dedup();
            println!(
                "object {} detail {} mesh {mesh} kind {} verts {} tris {} mats {:?} moved {moved} lo {:?} hi {:?}",
                s.name(o.name),
                s.name(d.name),
                m.kind,
                v.verts.len(),
                m.triangles().len(),
                mats,
                lo.map(|x| x.round()),
                hi.map(|x| x.round())
            );
        }
    }
}

fn percentiles(mut v: Vec<f64>) -> String {
    if v.is_empty() {
        return "none".into();
    }
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    format!("n {} p5 {:.3} p50 {:.3} p95 {:.3} max {:.3}", v.len(), at(0.05), at(0.5), at(0.95), at(1.0))
}

/// Phase-1 numbers: the course, the collision road's spacing, the visual road against it.
fn probe_road(archive: &mut Archive, mission: &str, visual: &str) -> Result<(), String> {
    let level = level::names(archive).into_iter().next().ok_or("no level in the zip")?;
    let scene = level::Scene::read(archive, &level)?;
    let mats = materials::read(archive, &level)?;
    let grounds = materials::ground_models(archive, &level)?;
    let race = Race::parse(&archive.read_text(&format!("{}/race.race.json", mission.trim_end_matches('/')))?)?;
    let c = course::course(&scene, &race)?;
    println!("level {level}: {} statics, {} roads", scene.statics.len(), scene.roads.len());
    println!(
        "course: road {} fit {:.1} m, length {:.1} m, {} points, splits {:?}",
        c.road,
        c.fit,
        c.line.length(),
        c.line.points.len(),
        c.splits.iter().map(|s| s.round()).collect::<Vec<_>>()
    );
    let ext = |k: usize| {
        (
            c.line.points.iter().map(|p| p[k]).fold(f64::MAX, f64::min),
            c.line.points.iter().map(|p| p[k]).fold(f64::MIN, f64::max),
        )
    };
    println!("extents x {:?} y {:?} z {:?}", ext(0), ext(1), ext(2));

    let mut col_parts = Vec::new();
    let mut vis_parts = Vec::new();
    for st in &scene.statics {
        let path = st.cdae_path();
        if scene.in_hidden_group(&st.parent) || !archive.contains(&path) {
            continue;
        }
        let shape = cdae::parse(&archive.read(&path)?)?;
        let has_col = shape.details.iter().any(|d| d.size < 0.0);
        if has_col && st.collision_type.as_deref() != Some("None") {
            col_parts.extend(world::parts(&shape, st, Detail::Collision));
        }
        if path.to_lowercase().contains(&visual.to_lowercase()) {
            vis_parts.extend(world::parts(&shape, st, Detail::Visible));
        }
    }
    let ground_of = |part: &world::Part, t: &Triangle| -> String {
        let name = t.material.and_then(|m| part.materials.get(m as usize)).cloned().unwrap_or_default();
        let g = mats.get(&name.to_lowercase()).and_then(|m| m.ground.clone()).unwrap_or_default().to_uppercase();
        let base = grounds.get(&g).cloned().unwrap_or(g.clone());
        format!("{name}:{g}:{base}")
    };
    let mut by_mat = std::collections::BTreeMap::<String, usize>::new();
    for p in &col_parts {
        for t in &p.triangles {
            *by_mat.entry(format!("{} {}", p.object, ground_of(p, t))).or_default() += 1;
        }
    }
    println!("collision triangles by object and material: {by_mat:?}");
    let col: Vec<&Triangle> = col_parts.iter().flat_map(|p| &p.triangles).collect();
    let steep = col.iter().filter(|t| t.normal()[2].abs() < 0.3).count();
    println!("collision: {} triangles, {} steep (walls)", col.len(), steep);
    let mut vis_mat = std::collections::BTreeMap::<String, usize>::new();
    for p in &vis_parts {
        for t in &p.triangles {
            let name = t.material.and_then(|m| p.materials.get(m as usize)).cloned().unwrap_or_default();
            *vis_mat.entry(name).or_default() += 1;
        }
    }
    println!("visual triangles by material: {vis_mat:?}");
    let colg = Grid::new(col.clone(), 8.0);
    let vis: Vec<&Triangle> = vis_parts.iter().flat_map(|p| &p.triangles).collect();
    let visg = Grid::new(vis, 8.0);
    // Heights along the centre line and across it.
    let mut diff = Vec::new();
    let mut missing_col = 0;
    let mut center_mat = std::collections::BTreeMap::<String, usize>::new();
    let mut s = 0.0;
    while s < c.line.length() {
        let (p, tan) = c.line.sample(s);
        let side = [-tan[1], tan[0]];
        for off in [-3.0, 0.0, 3.0] {
            let (x, y) = (p[0] + side[0] * off, p[1] + side[1] * off);
            let cg = colg.ground(x, y, p[2]);
            let vg = visg.ground(x, y, p[2]);
            match (cg, vg) {
                (Some((hc, _)), Some((hv, _))) => diff.push(hv - hc),
                (None, _) => missing_col += 1,
                _ => {}
            }
            if off == 0.0 {
                if let Some((_, t)) = cg {
                    let part = col_parts
                        .iter()
                        .find(|p| p.triangles.as_ptr_range().contains(&(t as *const Triangle)))
                        .unwrap();
                    *center_mat.entry(ground_of(part, t)).or_default() += 1;
                }
            }
        }
        s += 5.0;
    }
    println!("visual minus collision height on the road: {}", percentiles(diff.clone()));
    println!("  |diff| {}", percentiles(diff.iter().map(|d| d.abs()).collect()));
    println!("samples without collision under them: {missing_col}");
    println!("collision material at the centre line: {center_mat:?}");
    // Collision road spacing: edges of the road triangles near the line, split into along and across.
    let mut along = Vec::new();
    let mut across = Vec::new();
    let line = &c.line;
    for t in col.iter().step_by(7) {
        let mid = [
            (t.pos[0][0] + t.pos[1][0] + t.pos[2][0]) / 3.0,
            (t.pos[0][1] + t.pos[1][1] + t.pos[2][1]) / 3.0,
            (t.pos[0][2] + t.pos[1][2] + t.pos[2][2]) / 3.0,
        ];
        let (sa, d) = line.project(mid);
        if d > 4.0 || t.normal()[2].abs() < 0.3 {
            continue;
        }
        let (_, tan) = line.sample(sa);
        for k in 0..3 {
            let (a, b) = (t.pos[k], t.pos[(k + 1) % 3]);
            let e = [b[0] - a[0], b[1] - a[1]];
            let len = (e[0] * e[0] + e[1] * e[1]).sqrt();
            let cos = ((e[0] * tan[0] + e[1] * tan[1]) / len.max(1e-9)).abs();
            if cos > 0.9 {
                along.push(len);
            } else if cos < 0.3 {
                across.push(len);
            }
        }
        if along.len() > 20000 {
            break;
        }
    }
    if let Ok(points) = std::env::var("BEAMNG_PROBE_AT") {
        for p in points.split(';') {
            let v: Vec<f64> = p.split(',').filter_map(|t| t.parse().ok()).collect();
            let hits: Vec<(f64, String)> =
                colg.hits(v[0], v[1]).into_iter().map(|(h, t)| (h, format!("{:.2}", t.normal()[2]))).collect();
            let (s, d) = c.line.project([v[0], v[1], v[2]]);
            println!("at {p}: collision heights {hits:?}, driveline {d:.1} m away at {s:.0} m");
        }
    }
    println!("collision road edges along the road: {}", percentiles(along));
    println!("collision road edges across the road: {}", percentiles(across));
    Ok(())
}
