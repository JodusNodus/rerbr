//! A DiRT Rally car added to the launcher as a car of its own: the NGP physics, sound bank, group and drive train of the
//! gvrc.de car of the same model ([`MATCHES`]; added before, through the launcher's "Download cars…"), drawn with
//! DiRT's model on the gvrc model's frame.
//!
//! The launcher's added cars are in `cars/` in the player's folder (`rbr-launcher`'s `car_packs`): one `cars.ini`
//! section per physics pack folder (`rsfdata/cars/<pack>/`), naming the car's model (`Cars/<folder>/<file>.sgc`).
//! [`add`] writes, beside the gvrc car's:
//! - `rsfdata/cars/<pack> (DiRT Rally)/`: a copy of the gvrc car's physics pack, unchanged;
//! - `Cars/DiRT_<car>/`: DiRT's model with the gvrc model as its template ([`car_pack::convert`]: its hulls, cameras
//!   and the frame its physics was fitted to), and the gauges' pack under `Plugins/RBRHUD/gauges/` where measured;
//! - the car's section: the gvrc car's, with its own name, id and model. Adding again replaces the car, keeping its id;
//!   the launcher removes it as any added car.
use crate::car_pack;
use rbr_formats::ini::{self, Section};
use std::fs;
use std::path::{Path, PathBuf};

/// A DiRT car with a gvrc.de car of its model: the car (its folder under `cars/models`), its title, its generic textures
/// (`car_pack::Options::class`) and the gvrc car's pack folder (its `cars.ini` section).
pub struct Match {
    pub car: &'static str,
    pub title: &'static str,
    pub class: &'static str,
    pub pack: &'static str,
}

impl Match {
    const fn new(car: &'static str, title: &'static str, class: &'static str, pack: &'static str) -> Match {
        Match { car, title, class, pack }
    }
}

/// The DiRT cars whose model a gvrc.de car shares (its body and wheelbase; the season or evolution may differ). The
/// others have none: 037, 6R4, Delta S4, RS200, R5 Turbo, Fulvia, Mini Cooper S, Kadett, A110 1300, Sierra, Ibiza Kit
/// Car, Focus 2001, the i20 of 2014 (gvrc's is the 2017 coupé), the Sport quattro S1 E2 and the Delta Integrale (gvrc's
/// are the earlier bodies), and the Pikes Peak and rallycross cars.
pub const MATCHES: &[Match] = &[
    Match::new("cme", "Ford Escort RS1800 Mk II", "historic", "Ford Escort Mk II RS Grp4"),
    Match::new("131", "Fiat 131 Abarth", "historic", "Fiat 131 Abarth Grp4"),
    Match::new("str", "Lancia Stratos", "historic", "Lancia Stratos HF Grp4"),
    Match::new("m3r", "BMW M3 E30", "historic", "BMW M3 E30 GrpA"),
    Match::new("mta", "Opel Manta 400", "groupb", "Opel Manta 400 GrpB"),
    Match::new("20b", "Peugeot 205 T16 Evo 2", "groupb", "Peugeot 205 T16 GrpB"),
    Match::new("ers", "Ford Escort RS Cosworth", "rally", "Ford Escort Mk V RS Cosworth GrpA"),
    Match::new("555", "Subaru Impreza 555", "rally", "Subaru Impreza GC8 555 GrpA"),
    Match::new("306", "Peugeot 306 Maxi", "rally", "Peugeot 306 Maxi Kit Car"),
    Match::new("imp", "Subaru Impreza WRC 2001", "rally", "Subaru Impreza GDA WRC2003 (S9) 2006"),
    Match::new("for", "Ford Focus RS WRC 2007", "rally", "Ford Focus Mk II RS WRC 2006"),
    Match::new("c4r", "Citroën C4 WRC", "rally", "Citroen C4 WRC 2008"),
    Match::new("mer", "Mitsubishi Lancer Evo X R4", "rally", "Mitsubishi Lancer Evo X R4 (V1.2)"),
    Match::new("sti", "Subaru WRX STI NR4", "rally", "Subaru Impreza N15 R4"),
    Match::new("mcr", "Mini Countryman WRC", "rally", "Mini JCW WRC"),
    Match::new("ffr", "Ford Fiesta RS WRC", "rally", "Ford Fiesta RS WRC 2014"),
    Match::new("pol", "Volkswagen Polo R WRC", "rally", "VW Polo R WRC 2016"),
];

/// The launcher's added cars' folder, `<player folder>/cars`.
pub fn cars_folder() -> Option<PathBuf> {
    rbr_btb::player::player_folder().map(|p| p.join("cars"))
}

/// Adds DiRT's `car` (in the install's `data`) to the added cars in `cars` with the physics of its [`MATCHES`] gvrc
/// car, which must be there; returns the car's name.
pub fn add(data: &Path, cars: &Path, car: &str, livery: Option<u32>) -> Result<String, String> {
    let found = MATCHES.iter().find(|m| m.car == car).ok_or_else(|| {
        let known: Vec<&str> = MATCHES.iter().map(|m| m.car).collect();
        format!("{car} has no gvrc.de car of its model; these do: {}", known.join(", "))
    })?;
    let list_path = cars.join("cars.ini");
    let list = fs::read_to_string(&list_path).unwrap_or_default();
    let sections = ini::parse(&list).map_err(|e| format!("{}: line {}: {}", list_path.display(), e.line, e.message))?;
    let source = sections.iter().find(|s| s.name == found.pack).ok_or_else(|| {
        format!("add the gvrc.de car {} first (the launcher's Settings, \"Download cars…\")", found.pack)
    })?;
    let model = source.get("model").ok_or_else(|| format!("{} has no model of its own", found.pack))?;
    let (folder, file) = model.split_once('/').ok_or_else(|| format!("{}: bad model {model}", found.pack))?;
    let physics = cars.join("rsfdata").join("cars");
    let pack = format!("{} (DiRT Rally)", found.pack);
    let name = format!("DiRT_{car}");

    let ours = cars.join("Cars").join(&name);
    remove(&ours)?;
    let options = car_pack::Options {
        car: car.to_owned(),
        class: found.class.to_owned(),
        livery,
        template: cars.join("Cars").join(folder).join(format!("{file}.sgc")),
        name: name.clone(),
    };
    if let Err(e) = car_pack::convert(data, &options, cars) {
        remove(&ours)?;
        return Err(e);
    }
    let copy = physics.join(&pack);
    remove(&copy)?;
    copy_folder(&physics.join(found.pack), &copy)?;

    let id = match sections.iter().find(|s| s.name == pack) {
        Some(known) => id_of(known),
        None => sections.iter().map(id_of).max().unwrap_or(0) + 1,
    };
    let car_name = format!("{} (DiRT Rally)", source.get("name").unwrap_or(found.pack));
    let model = format!("{name}/{name}");
    let text = ini::replace_section(&list, &pack, &section(source, &pack, &car_name, id, &model, found.title));
    let temp = cars.join(".cars.ini.new");
    fs::write(&temp, text)
        .and_then(|()| fs::rename(&temp, &list_path))
        .map_err(|e| format!("{}: {e}", list_path.display()))?;
    Ok(car_name)
}

/// A `cars.ini` section's id, 0 without one.
fn id_of(section: &Section) -> u32 {
    section.get("id").and_then(|id| id.parse().ok()).unwrap_or(0)
}

/// The added car's section: `source`'s (the gvrc car's group, drive train, year, revision and bank) under `pack`, with
/// its own name, id and model.
fn section(source: &Section, pack: &str, name: &str, id: u32, model: &str, title: &str) -> String {
    let own = ["name", "id", "model", "model_name", "standin"];
    let kept = source.entries.iter().filter(|(key, _)| !own.contains(&key.as_str()));
    let rows = [("name", name.to_owned()), ("id", id.to_string())]
        .into_iter()
        .chain(kept.map(|(key, value)| (key.as_str(), value.clone())))
        .chain([("model", model.to_owned()), ("model_name", format!("{title} (DiRT Rally)"))]);
    rows.fold(format!("[{pack}]\n"), |text, (key, value)| text + &format!("{key} = {value}\n"))
}

/// Removes the folder `path` when it is there.
fn remove(path: &Path) -> Result<(), String> {
    match path.exists() {
        true => fs::remove_dir_all(path).map_err(|e| format!("{}: {e}", path.display())),
        false => Ok(()),
    }
}

/// Copies the folder `from` to `to`, with everything in it.
fn copy_folder(from: &Path, to: &Path) -> Result<(), String> {
    let error = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    fs::create_dir_all(to).map_err(|e| error(to, e))?;
    for entry in fs::read_dir(from).map_err(|e| error(from, e))? {
        let entry = entry.map_err(|e| error(from, e))?;
        let target = to.join(entry.file_name());
        match entry.file_type().map_err(|e| error(&entry.path(), e))?.is_dir() {
            true => copy_folder(&entry.path(), &target)?,
            false => fs::copy(entry.path(), &target).map(drop).map_err(|e| error(&target, e))?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_added_car_keeps_the_gvrc_cars_physics_and_names_its_own_model() {
        let list = "[Ford Escort Mk II RS Grp4]\nname = Ford Escort Mk II RS group 4\nid = 82\ngroup = Group 4\n\
                    drive = RWD\nrevision = 1 / 2020-05-24\nbank = Escort_Mk2_Grp4\nmodel = Escort_mk_2/Escort_mk_2\n\
                    model_name = Ford Escort Mk II RS Grp4\n";
        let source = &ini::parse(list).unwrap()[0];
        let text = section(source, "Escort (DiRT Rally)", "Escort (DiRT Rally)", 103, "DiRT_cme/DiRT_cme", "Escort");
        assert_eq!(
            text,
            "[Escort (DiRT Rally)]\nname = Escort (DiRT Rally)\nid = 103\ngroup = Group 4\ndrive = RWD\n\
             revision = 1 / 2020-05-24\nbank = Escort_Mk2_Grp4\nmodel = DiRT_cme/DiRT_cme\nmodel_name = Escort (DiRT Rally)\n"
        );
    }

    #[test]
    fn each_match_names_one_car_and_a_class_of_the_games_and_the_help_lists_it() {
        let listed = crate::cli::HELP.split("added before (").nth(1).and_then(|rest| rest.split(')').next()).unwrap();
        let listed: Vec<&str> = listed.split([',', ' ', '\n']).filter(|w| !w.is_empty()).collect();
        assert_eq!(listed, MATCHES.iter().map(|m| m.car).collect::<Vec<_>>());
        for (k, m) in MATCHES.iter().enumerate() {
            assert!(["historic", "groupb", "rally", "rallyx", "trailblazer"].contains(&m.class), "{}", m.car);
            assert!(MATCHES[..k].iter().all(|n| n.car != m.car && n.pack != m.pack), "{} twice", m.car);
        }
    }
}
