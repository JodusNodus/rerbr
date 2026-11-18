//! Car skins (`car_skins`): an archive or folder with a `textures` folder of `.dds` files, when no other kind takes it
//! (a car model's archive and stages hold textures too). The car it dresses: the one whose model folder its path
//! names (`Cars/<model>/Textures/`), else the one whose model's textures share the most names with it, at least half
//! of them (a skin replaces a model's textures by name). One model takes it, in every car drawn with it (RSF's and an
//! added copy); several models, or none, leave it to the car's own Import skin… on Cars.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::{Ctx, Found, Item, Kind};
use crate::car_skins;
use crate::content::downloads::Owned;
use crate::content::source::{Entry, Source};
use crate::content::{stem, Goto, Outcome};

pub struct Skin;

const KIND: &str = "car skin";

/// What to do when the car is not known.
const BY_HAND: &str = "open the car in Cars and use Import skin…";

/// A car a skin can dress (`Known::cars`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    /// Its key, the folder its skins go to.
    pub key: String,
    pub title: String,
    /// Its model folder (`Cars\<model>`).
    pub model: String,
    /// Its model's own `Textures` folder.
    pub textures: Option<PathBuf>,
}

/// The `.dds` names in the `textures` folders of the skins `roots`, lower case.
fn dds(entries: &[Entry], roots: &[Vec<String>]) -> BTreeSet<String> {
    let in_textures = |e: &&Entry| {
        roots
            .iter()
            .any(|r| e.path.starts_with(r) && e.path.get(r.len()).is_some_and(|p| p.eq_ignore_ascii_case("textures")))
    };
    let names = entries.iter().filter(in_textures).filter_map(|e| e.path.last()).map(|n| n.to_lowercase());
    names.filter(|n| n.ends_with(".dds")).collect()
}

/// The model folder a skin's path names: the part after `Cars`.
fn named_model(roots: &[Vec<String>]) -> Option<&str> {
    roots.iter().find_map(|r| {
        let at = r.iter().position(|p| p.eq_ignore_ascii_case("cars"))?;
        r.get(at + 1).map(String::as_str)
    })
}

/// The cars a skin of the textures `dds` dresses, each with its model's texture names, `named` the model folder its
/// path names: the cars of one model, or why there is none.
fn dresses<'a>(
    named: Option<&str>,
    dds: &BTreeSet<String>,
    cars: &'a [(Target, BTreeSet<String>)],
) -> Result<Vec<&'a Target>, String> {
    let by_name: Vec<&Target> = cars
        .iter()
        .map(|(car, _)| car)
        .filter(|car| named.is_some_and(|m| car.model.eq_ignore_ascii_case(m)))
        .collect();
    let picked = if by_name.is_empty() {
        let score = |names: &BTreeSet<String>| dds.intersection(names).count();
        let best = cars.iter().map(|(_, names)| score(names)).max().unwrap_or(0);
        if best == 0 || best * 2 < dds.len() {
            return Err(format!("no installed car uses its textures: {BY_HAND}"));
        }
        cars.iter().filter(|(_, names)| score(names) == best).map(|(car, _)| car).collect()
    } else {
        by_name
    };
    let models: BTreeSet<String> = picked.iter().map(|car| car.model.to_lowercase()).collect();
    if models.len() > 1 {
        let mut titles: Vec<&str> = picked.iter().map(|car| car.title.as_str()).collect();
        titles.dedup();
        let more = if titles.len() > 3 { ", …" } else { "" };
        let titles = titles[..titles.len().min(3)].join(", ");
        return Err(format!("it fits several cars ({titles}{more}): {BY_HAND}"));
    }
    Ok(picked)
}

/// The `.dds` names in `dir`, lower case.
fn names_in(dir: Option<&PathBuf>) -> BTreeSet<String> {
    let files = dir.and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten();
    files.map(|f| f.file_name().to_string_lossy().to_lowercase()).filter(|n| n.ends_with(".dds")).collect()
}

impl Kind for Skin {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        let file = |e: &Entry| e.path.last().map(|f| f.to_ascii_lowercase()).unwrap_or_default();
        if src.entries().iter().any(|e| file(e).ends_with(".sgc") || file(e) == "common.lsp") {
            return None;
        }
        let roots = car_skins::roots(src);
        (!dds(src.entries(), &roots).is_empty()).then(|| Found::new(KIND))
    }

    fn add(&self, item: &Item, ctx: &Ctx) -> Vec<Outcome> {
        let name = item.src.name();
        let roots = car_skins::roots(item.src);
        let cars: Vec<(Target, BTreeSet<String>)> =
            ctx.known.cars.iter().map(|car| (car.clone(), names_in(car.textures.as_ref()))).collect();
        let targets = match dresses(named_model(&roots), &dds(item.src.entries(), &roots), &cars) {
            Ok(targets) => targets,
            Err(why) => return vec![Outcome::failed(&name, KIND, why).going(Goto::Cars)],
        };
        ctx.step("Adding car skin");
        let add = |car: &Target| {
            let dest = ctx.home.join(car_skins::DIR).join(&car.key);
            match car_skins::import(item.src, &dest, ctx.report, ctx.cancel) {
                Ok(skins) => {
                    let first = skins.first().cloned().unwrap_or_default();
                    let worn = if skins.len() == 1 { "it".to_owned() } else { first.clone() };
                    let place = format!("the {} wears {worn} now", car.title);
                    Outcome::added(
                        &name,
                        KIND,
                        &skins.join(", "),
                        &place,
                        Goto::Skin { car: car.key.clone(), skin: first },
                    )
                }
                Err(e) => Outcome::failed(&name, KIND, e),
            }
        };
        targets.into_iter().map(add).collect()
    }

    fn fallback(&self) -> bool {
        true
    }

    fn offer(&self, src: &dyn Source, found: &Found, owned: &Owned) -> Option<&'static str> {
        let name = car_skins::clean(stem(&src.name())).to_lowercase();
        (!owned.skins.contains(&name)).then_some(found.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::source::Fake;

    fn target(key: &str, model: &str) -> Target {
        Target { key: key.into(), title: format!("{model} car"), model: model.into(), textures: None }
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn keys(found: Result<Vec<&Target>, String>) -> Result<Vec<&str>, String> {
        found.map(|cars| cars.iter().map(|c| c.key.as_str()).collect())
    }

    #[test]
    fn a_skin_is_found_by_its_textures_folder_unless_it_is_a_car_or_a_stage() {
        let kind = |files: &[&str]| Skin.detect(&Fake::new("x.zip", false, files)).map(|f| f.label);
        assert_eq!(kind(&["Red/textures/fiesta_external.dds", "Red/readme.txt"]), Some(KIND));
        assert_eq!(kind(&["Cars/Fiesta/Textures/a.DDS"]), Some(KIND));
        assert_eq!(kind(&["Cars/Fiesta/Textures/a.dds", "Cars/Fiesta/Fiesta.sgc"]), None);
        assert_eq!(kind(&["textures/readme.txt"]), None);
        assert_eq!(kind(&["Fiesta/a.dds"]), None);
        // Dropped with a stage it is the stage's.
        let stage = ["Lappi/track.ini", "Lappi/objectlist.ini", "Lappi/Objects.zip", "Lappi/Textures.zip"];
        let mut files = stage.to_vec();
        files.push("Lappi/textures/a.dds");
        let found = crate::content::detect(&Fake::new("lappi.zip", false, &files));
        assert_eq!(found.iter().map(|(_, f)| f.label).collect::<Vec<_>>(), ["BTB stage"]);
    }

    #[test]
    fn the_car_is_the_one_its_path_or_its_textures_name() {
        let fiesta = ["fiesta_rally3_external.dds", "fiesta_rally3_internal.dds", "light.dds"];
        let cars = vec![
            (target("rsf-1", "Fiesta_rally3"), set(&fiesta)),
            (target("pack-fiesta", "fiesta_rally3"), set(&fiesta)),
            (target("XSARA", "XSARA"), set(&["xsara_external.dds", "light.dds"])),
            (target("IMPREZA03", "IMPREZA03"), set(&["impreza03_external.dds", "light.dds"])),
        ];
        let skin = set(&["fiesta_rally3_external.dds", "fiesta_rally3_internal_ext.dds"]);
        // One model in two cars: both take it.
        assert_eq!(keys(dresses(None, &skin, &cars)), Ok(vec!["rsf-1", "pack-fiesta"]));
        assert_eq!(keys(dresses(Some("XSARA"), &skin, &cars)), Ok(vec!["XSARA"]), "the path wins");
        assert_eq!(keys(dresses(Some("gone"), &set(&["xsara_external.dds"]), &cars)), Ok(vec!["XSARA"]));
        let several = dresses(None, &set(&["light.dds"]), &cars).unwrap_err();
        assert!(several.starts_with("it fits several cars (Fiesta_rally3 car, fiesta_rally3 car, XSARA car, …)"));
        let few = set(&["light.dds", "a.dds", "b.dds"]);
        assert!(dresses(None, &few, &cars).unwrap_err().starts_with("no installed car uses its textures"));
        assert!(dresses(None, &set(&["other.dds"]), &cars).is_err());
    }

    #[test]
    fn a_dropped_skin_goes_to_its_car_and_is_worn() {
        let dir = crate::test_dir("content-skin");
        let model = dir.join("game/Cars/XSARA/Textures");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("XSARA_external.dds"), "car").unwrap();
        let skin = dir.join("drop/Martini/textures");
        std::fs::create_dir_all(&skin).unwrap();
        std::fs::write(skin.join("xsara_external.dds"), "skin").unwrap();
        let known = super::super::Known {
            cars: vec![
                Target { textures: Some(model.clone()), ..target("XSARA", "XSARA") },
                target("IMPREZA03", "IMPREZA03"),
            ],
            ..Default::default()
        };
        let home = dir.join("home");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let done = crate::content::run(&home, &known, &[dir.join("drop/Martini")], &|_| {}, &cancel);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].line(), "Added Martini (car skin): the XSARA car wears it now");
        assert_eq!(done[0].goto, Some(Goto::Skin { car: "XSARA".into(), skin: "Martini".into() }));
        assert_eq!(std::fs::read(home.join("skins/XSARA/Martini/textures/xsara_external.dds")).unwrap(), b"skin");
        // Offered until it is added.
        let src = crate::content::source::open(&dir.join("drop/Martini")).unwrap();
        let found = Found::new(KIND);
        assert_eq!(Skin.offer(&*src, &found, &Owned::default()), Some(KIND));
        let owned = Owned { skins: car_skins::imported(&home), ..Owned::default() };
        assert_eq!(Skin.offer(&*src, &found, &owned), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
