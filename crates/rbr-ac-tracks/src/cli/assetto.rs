use crate::assetto::{
    kn5,
    stage::{self, Input, Options},
};
use std::collections::BTreeMap;
use std::path::PathBuf;

const HELP: &str = "rbr-ac-tracks ac --track AC_TRACK_FOLDER [--layout LAYOUT] [--info]
         [--out DIR] [--name NAME] [--replace] [--remesh]
         [--draw-corridor M] [--collision-corridor M] [--lod M] [--surface KEY=RBR_ID]...
         [--prepare DIR --maps RBR_MAPS_FOLDER]
         [--ai-file PATH] [--reverse-ai] [--circuit]

Convert your local copy of a point-to-point Assetto Corsa track to a BTB stage for ReRBR.
Output defaults to the player's tracks/AC_<layout> folder; --out DIR writes DIR/<name>.
Without --layout, reads models.ini or the track's same-named KN5 and root AI/data/ui folders.
Requires KN5 v5/6, AI v7 and AC_AB_START/FINISH timing markers, unless --circuit.
--ai-file supplies an existing AI line, e.g. another layout's. --reverse-ai reverses it before gate checks.
--circuit writes a closed AI loop (ends within 5 m) as a ReRBR circuit: `circuit=1`, a closed driveline ring, and the
lap line and spawn at AC_AB_START when it lies on the loop, else at the AI line's first point. The source's finish
and sector gates are ignored and the splits are thirds.
--replace only replaces an existing rbr-ac-tracks ac output. Source game files are read in place.
Materials keep diffuse textures and alpha, with multilayer and detail textures baked in; normal maps and the sun
highlight are written for ReRBR's renderer; CSP scripts are not reproduced.
--remesh smooths the road collision with the shared BTB road re-mesher.
A surface's name gives its RBR class and its FRICTION in surfaces.ini the surface of that class nearest in grip
(0.98 is RBR's grippiest tarmac, 37). A road that dirties the tyres and declares 0.90 or less, or CSP's
_EXT_SURFACE_TYPE=GRAVEL, is gravel; `physics=` is gravel when most of the drive is. --surface overrides one, e.g. ROAD=37.
";

pub fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut track = None;
    let mut layout = None;
    let mut name = None;
    let mut out = None;
    let mut prepare = None;
    let mut maps = None;
    let mut info = false;
    let mut replace = false;
    let mut remesh = false;
    let mut ai_file = None;
    let mut reverse_ai = false;
    let mut circuit = false;
    let mut draw = 300.;
    let mut collision = 75.;
    let mut lod = 1000.;
    let mut overrides = BTreeMap::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if matches!(arg.as_str(), "--help" | "-h") {
            println!("{HELP}");
            return Ok(());
        }
        match arg.as_str() {
            "--info" => info = true,
            "--replace" => replace = true,
            "--remesh" => remesh = true,
            "--reverse-ai" => reverse_ai = true,
            "--circuit" => circuit = true,
            _ => {
                let v = args.next().ok_or_else(|| format!("{arg} needs a value\n{HELP}"))?;
                match arg.as_str() {
                    "--track" => track = Some(PathBuf::from(v)),
                    "--layout" => layout = Some(v),
                    "--ai-file" => ai_file = Some(PathBuf::from(v)),
                    "--name" => name = Some(v),
                    "--out" => out = Some(PathBuf::from(v)),
                    "--prepare" => prepare = Some(PathBuf::from(v)),
                    "--maps" => maps = Some(PathBuf::from(v)),
                    "--draw-corridor" => draw = v.parse::<f64>().map_err(|e| e.to_string())?,
                    "--collision-corridor" => collision = v.parse::<f64>().map_err(|e| e.to_string())?,
                    "--lod" => lod = v.parse::<f64>().map_err(|e| e.to_string())?,
                    "--surface" => {
                        let (key, id) = v.split_once('=').ok_or("--surface expects KEY=ID")?;
                        let id = id.parse::<u8>().map_err(|e| e.to_string())?;
                        overrides.insert(key.to_uppercase(), id);
                    }
                    _ => return Err(format!("unknown argument {arg}\n{HELP}")),
                }
            }
        }
    }
    let track = track.ok_or(HELP)?;
    let layout = layout.unwrap_or_else(|| ".".into());
    let input = Input::read_with_ai(&track, &layout, ai_file.as_deref(), reverse_ai, circuit)?;
    println!(
        "{} [{}]: {:.1} m timed, {} AI points, {} models",
        input.title,
        input.layout,
        input.finish - input.start,
        input.line.points.len(),
        input.models.len()
    );
    if info {
        for (path, transform) in &input.models {
            let model = kn5::read(path, false, false, transform)?;
            println!(
                "{}: KN5 v{}, {} meshes, {} triangles, {} materials",
                path.file_name().unwrap().to_string_lossy(),
                model.version,
                model.meshes.len(),
                model.meshes.iter().map(|m| m.triangle_count).sum::<usize>(),
                model.materials.len()
            );
        }
        return Ok(());
    }
    let out = out.or_else(rbr_btb::player::tracks).ok_or("--out required without a player folder")?;
    let name = name.unwrap_or_else(|| {
        format!(
            "AC_{}",
            if layout == "." { input.root.file_name().unwrap().to_string_lossy().into_owned() } else { layout }
        )
    });
    let options = Options {
        out,
        name,
        draw_corridor: draw,
        collision_corridor: collision,
        lod,
        replace,
        remesh,
        surface_overrides: overrides,
        control: Default::default(),
    };
    let report = stage::convert(&input, &options)?;
    println!("{}\n{report:?}", report.folder.display());
    if let Some(dir) = prepare {
        let name = crate::btb::stage::prepare(&report.folder, &dir, maps.as_deref())?;
        println!("Prepared: {name}");
    }
    Ok(())
}
