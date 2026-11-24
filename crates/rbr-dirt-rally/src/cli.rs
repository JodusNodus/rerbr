//! Command dispatch of the `rbr-dirt-rally` executable.
use crate::install::Install;
use std::path::Path;

const HELP: &str = "rbr-dirt-rally: convert stages of a locally installed DiRT Rally to BTB stages for ReRBR

Usage:
  rbr-dirt-rally list [INSTALL]
  rbr-dirt-rally car CAR --class CLASS --template CARFOLDER --out DIR [--livery N] [--name NAME] [--install INSTALL]
  rbr-dirt-rally import LOCATION/AREA/route_N [--install INSTALL] [--name NAME] [--out DIR] [--replace]
         [--draw-corridor M] [--collision-corridor M] [--lod M]

INSTALL is the game folder (or its data folder); without it, the Steam libraries are searched.
Source game files are read in place. CAR is the folder under cars/models (cme), CLASS its generic textures under
cars/generics (historic, rally, groupb, rallyx, trailblazer), CARFOLDER a car of the player's of the same model; the
livery is the car's first without --livery.

Developer: xml FILE, pssg-tree FILE [DEPTH], car-textures PSSG, car-texture PSSG NAME OUT.dds, car-info PSSG [-v].";

pub fn main_entry(args: impl IntoIterator<Item = String>) {
    if let Err(error) = run(args) {
        eprintln!("rbr-dirt-rally: {error}");
        std::process::exit(1);
    }
}

pub fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None | Some("help" | "-h" | "--help") => {
            println!("{HELP}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("rbr-dirt-rally {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("list") => {
            let install = install(args.next().as_deref())?;
            for route in install.routes() {
                let kind = match (route.is_rally(), route.is_rallycross()) {
                    (true, _) => "",
                    (_, true) => " (rallycross circuit)",
                    _ => " (not a rally stage)",
                };
                println!("{}{kind}", route.id());
            }
            Ok(())
        }
        Some("import") => import(args),
        Some("car") => car(args),
        Some(other) => developer(other, args),
    }
}

/// The commands for reading a car's or a stage's files by hand, and the unknown ones.
fn developer(command: &str, mut args: impl Iterator<Item = String>) -> Result<(), String> {
    match command {
        "xml" => {
            let path = args.next().ok_or("xml needs a binary XML file")?;
            let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            fn show(e: &crate::binxml::Element, depth: usize) {
                println!("{}{} {:?} {}", "  ".repeat(depth), e.name, e.attrs, e.text.as_deref().unwrap_or(""));
                e.children.iter().for_each(|c| show(c, depth + 1));
            }
            show(&crate::binxml::parse(&bytes)?, 0);
            Ok(())
        }
        "pssg-tree" => {
            let path = args.next().ok_or("pssg-tree needs a .pssg")?;
            let depth: usize = args.next().map_or(Ok(3), |d| d.parse().map_err(|e| format!("depth: {e}")))?;
            fn show(n: crate::pssg::Node, depth: usize, left: usize) {
                let attrs: Vec<String> = n
                    .attrs()
                    .filter(|(k, _)| !matches!(*k, "size" | "id"))
                    .map(|(k, v)| match crate::pssg::decode_str(v) {
                        Some(s) => format!("{k}={s}"),
                        None if v.len() == 4 => format!("{k}={}", u32::from_be_bytes([v[0], v[1], v[2], v[3]])),
                        None => format!("{k}=[{} bytes]", v.len()),
                    })
                    .collect();
                println!("{}{} {} {}", "  ".repeat(depth), n.name(), n.id().unwrap_or(""), attrs.join(" "));
                if left > 0 {
                    n.children().for_each(|c| show(c, depth + 1, left - 1));
                }
            }
            crate::pssg::Pssg::read(Path::new(&path))?.roots().for_each(|n| show(n, 0, depth));
            Ok(())
        }
        "car-textures" => {
            let path = args.next().ok_or("car-textures needs a livery .pssg")?;
            for t in crate::texture::read_car(&crate::pssg::Pssg::read(Path::new(&path))?) {
                println!("{:<40} {}x{} {:?} {} levels", t.name, t.width, t.height, t.format, t.levels);
            }
            Ok(())
        }
        "car-texture" => {
            let (path, name, out) = (args.next(), args.next(), args.next());
            let (Some(path), Some(name), Some(out)) = (path, name, out) else {
                return Err("car-texture needs a .pssg, a texture name and an output .dds".into());
            };
            let textures = crate::texture::read_car(&crate::pssg::Pssg::read(Path::new(&path))?);
            let texture = textures.iter().find(|t| t.name == name).ok_or("no such texture")?;
            std::fs::write(out, texture.dds()).map_err(|e| e.to_string())
        }
        "car-info" => {
            let path = args.next().ok_or("car-info needs a <car>_highlod.pssg")?;
            let pssg = crate::pssg::Pssg::read(Path::new(&path))?;
            let parts =
                if pssg.by_id("LOD0_").is_some() { crate::car::parts(&pssg)? } else { crate::car::interior(&pssg)? };
            print!("{}", crate::car::info(&parts, args.next().as_deref() == Some("-v")));
            print!("{}", crate::car::materials(&pssg));
            Ok(())
        }
        other => Err(format!("unknown command {other}; see --help")),
    }
}

fn install(path: Option<&str>) -> Result<Install, String> {
    match path {
        Some(path) => Install::at(Path::new(path)),
        None => {
            Install::find().ok_or_else(|| "no DiRT Rally install found in the Steam libraries; pass its folder".into())
        }
    }
}

fn car(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let car = args.next().filter(|a| !a.starts_with("--")).ok_or("car needs the car's folder (cme)")?;
    let (mut path, mut out, mut template, mut class, mut livery, mut name) = (None, None, None, None, None, None);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--install" => path = Some(value),
            "--out" => out = Some(value),
            "--template" => template = Some(value),
            "--class" => class = Some(value),
            "--livery" => livery = Some(value.parse().map_err(|e| format!("{flag}: {e}"))?),
            "--name" => name = Some(value),
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    let options = crate::car_pack::Options {
        name: name.unwrap_or_else(|| format!("DiRT_{car}")),
        car,
        class: class.ok_or("car needs --class, its generic textures under cars/generics (historic, rally, ...)")?,
        livery,
        template: template.ok_or("car needs --template, the folder of a car of the same model")?.into(),
    };
    let out = out.ok_or("car needs --out")?;
    let model = crate::car_pack::convert(&install(path.as_deref())?.data, &options, Path::new(&out))?;
    println!("{}", model.display());
    Ok(())
}

fn import(args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut args = args.peekable();
    let id = args.next().filter(|a| !a.starts_with("--")).ok_or("import needs LOCATION/AREA/route_N")?;
    let log = |m: &str| eprintln!("rbr-dirt-rally: {m}");
    let control = rbr_ac_tracks::btb::control::Control::new(log, Default::default());
    let mut options = crate::import::Options::new(control);
    let mut path = None;
    while let Some(flag) = args.next() {
        if flag == "--replace" {
            options.replace = true;
            continue;
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let metres = || value.parse::<f64>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--install" => path = Some(value.clone()),
            "--name" => options.name = Some(value.clone()),
            "--out" => options.out = Some(value.clone().into()),
            "--draw-corridor" => options.draw_corridor = metres()?,
            "--collision-corridor" => options.collision_corridor = metres()?,
            "--lod" => options.lod = metres()?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    let report = crate::import::import(&install(path.as_deref())?, &id, &options)?;
    println!("{}", report.folder.display());
    println!("{report:?}");
    Ok(())
}
