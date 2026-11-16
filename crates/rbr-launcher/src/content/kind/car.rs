//! NGP car packs: a physics zip (a folder holding `common.lsp`, gvrc.de's `<Car> - NGP Physics.zip`), a model
//! archive (a `.sgc`) and a sound-bank archive (`.bank`). Dropped together they are matched by name (as
//! `car_packs::pick_model` does) into one `car_packs::Download` each; a model or banks archive alone has no car to go to.

use std::path::PathBuf;

use super::{Ctx, Found, Item, Kind};
use crate::car_packs::{self, strip_end, Download, BANKS, MODEL, PHYSICS};
use crate::content::downloads::Owned;
use crate::content::source::Source;
use crate::content::{stem, Goto, Outcome, Refresh};

pub struct Car;

const PHYSICS_ROLE: &str = "physics";
const MODEL_ROLE: &str = "model";
const BANKS_ROLE: &str = "banks";

/// A car download's file name without the end gvrc.de gives.
fn car_name(file: &str, end: &str) -> String {
    strip_end(file, end).unwrap_or_else(|| stem(file)).to_owned()
}

impl Kind for Car {
    fn detect(&self, src: &dyn Source) -> Option<Found> {
        let lower = src.name().to_ascii_lowercase();
        let file =
            |e: &crate::content::source::Entry| e.path.last().map(|f| f.to_ascii_lowercase()).unwrap_or_default();
        let any = |test: &dyn Fn(&crate::content::source::Entry) -> bool| src.entries().iter().any(test);
        let (tag, label) = if src.is_dir() {
            return None;
        } else if lower.ends_with(".zip") && any(&|e| e.path.len() <= 2 && file(e) == "common.lsp") {
            (PHYSICS_ROLE, "NGP car")
        } else if lower.ends_with(".7z") && any(&|e| e.path.len() >= 2 && file(e).ends_with(".sgc")) {
            (MODEL_ROLE, "NGP car model")
        } else if lower.ends_with(".7z") && any(&|e| file(e).ends_with(".bank") && !file(e).ends_with(".strings.bank"))
        {
            (BANKS_ROLE, "NGP car sounds")
        } else {
            return None;
        };
        Some(Found { tag, ..Found::new(label) })
    }

    fn add(&self, item: &Item, _: &Ctx) -> Vec<Outcome> {
        let why = "this is a car model/sound; add it together with its NGP Physics zip";
        vec![Outcome::failed(&item.src.name(), "NGP car", why)]
    }

    fn add_all(&self, items: &[Item], ctx: &Ctx) -> Vec<Outcome> {
        let paths = |role: &str| -> Vec<PathBuf> {
            items.iter().filter(|i| i.found.tag == role).map(|i| i.src.path().to_owned()).collect()
        };
        let (downloads, left) = group(&paths(PHYSICS_ROLE), &paths(MODEL_ROLE), &paths(BANKS_ROLE));
        let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut out: Vec<Outcome> = Vec::new();
        for path in left {
            let item = items.iter().find(|i| i.src.path() == path);
            out.extend(item.map(|i| self.add(i, ctx)).unwrap_or_default());
        }
        if downloads.is_empty() {
            return out;
        }
        ctx.step("Adding cars");
        let added = car_packs::add_downloads(&ctx.home.join(car_packs::DIR), &downloads, ctx.report, ctx.cancel);
        let outcomes = match added {
            Ok(added) => added
                .into_iter()
                .map(|a| match a.result {
                    Ok((car, model)) => {
                        let place = match model {
                            Some(_) => "it's in the car picker",
                            None => "it's in the car picker, drawn with a stand-in model until its model is added",
                        };
                        Outcome::added(&a.download, "NGP car", &car, place, Goto::Settings)
                    }
                    Err(e) => Outcome::failed(&a.download, "NGP car", e),
                })
                .collect(),
            Err(e) => {
                downloads.iter().map(|d| Outcome::failed(&name(&d.physics), "NGP car", e.clone())).collect::<Vec<_>>()
            }
        };
        out.extend(outcomes.into_iter().map(|o| o.refreshing(Refresh::Cars)));
        out
    }

    fn offer(&self, src: &dyn Source, found: &Found, owned: &Owned) -> Option<&'static str> {
        let car = car_packs::words(&car_name(&src.name(), PHYSICS));
        (found.tag == PHYSICS_ROLE && !owned.cars.contains(&car)).then_some(found.label)
    }
}

/// The physics zips of `physics` with the model and the sound banks that go with each, by name; the model and banks
/// archives that go with none.
pub fn group(physics: &[PathBuf], models: &[PathBuf], banks: &[PathBuf]) -> (Vec<Download>, Vec<PathBuf>) {
    let file = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut models: Vec<(String, &PathBuf)> = models.iter().map(|p| (car_name(&file(p), MODEL), p)).collect();
    let mut banks: Vec<(String, &PathBuf)> = banks.iter().map(|p| (car_name(&file(p), BANKS), p)).collect();
    let take = |pool: &mut Vec<(String, &PathBuf)>, car: &str| {
        let names: Vec<&str> = pool.iter().map(|(n, _)| n.as_str()).collect();
        let at = car_packs::pick_model(&[car], &names)?;
        Some(pool.remove(at).1.clone())
    };
    let downloads = physics
        .iter()
        .map(|p| {
            let car = car_name(&file(p), PHYSICS);
            Download {
                physics: p.clone(),
                model: take(&mut models, &car),
                banks: take(&mut banks, &car),
                group: None,
                drive_train: None,
                year: None,
            }
        })
        .collect();
    let left = models.into_iter().chain(banks).map(|(_, p)| p.clone()).collect();
    (downloads, left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::source::Fake;

    #[test]
    fn the_three_pieces_are_told_apart() {
        let role = |name: &str, files: &[&str]| Car.detect(&Fake::new(name, false, files)).map(|f| f.tag);
        assert_eq!(role("Ford Fiesta R5 - NGP Physics.zip", &["common.lsp", "engine.lsp"]), Some("physics"));
        assert_eq!(role("x.zip", &["Ford_Fiesta_R5/common.lsp"]), Some("physics"));
        let model = ["Cars/Ford Fiesta/Ford Fiesta.sgc", "Cars/Ford Fiesta/a.dds"];
        assert_eq!(role("Ford Fiesta - Model.7z", &model), Some("model"));
        let banks = ["AudioFMOD/fiesta.bank", "AudioFMOD/fiesta.strings.bank"];
        assert_eq!(role("Ford Fiesta R5 - banks.7z", &banks), Some("banks"));
        assert_eq!(role("a.zip", &["readme.txt"]), None);
        assert_eq!(car_name("Ford Fiesta R5 - NGP Physics.zip", PHYSICS), "Ford Fiesta R5");
    }

    #[test]
    fn cars_are_grouped_by_their_names() {
        let p = |s: &str| PathBuf::from(s);
        let physics = [p("/d/Ford Fiesta R5 - NGP Physics.zip"), p("/d/Audi quattro - NGP Physics.zip")];
        let models = [p("/d/Ford Fiesta - Model.7z"), p("/d/Skoda Fabia - Model.7z")];
        let banks = [p("/d/Audi quattro - banks.7z")];
        let (downloads, left) = group(&physics, &models, &banks);
        assert_eq!(downloads[0].model, Some(p("/d/Ford Fiesta - Model.7z")));
        assert_eq!(downloads[0].banks, None);
        let second = (downloads[1].model.clone(), downloads[1].banks.clone());
        assert_eq!(second, (None, Some(p("/d/Audi quattro - banks.7z"))));
        assert_eq!(left, [p("/d/Skoda Fabia - Model.7z")]);
    }
}
