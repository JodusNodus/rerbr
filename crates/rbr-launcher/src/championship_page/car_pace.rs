//! The field's car models as quick as their physics: each model's specs from its physics scripts (a pack's own, or
//! the NGP archive's slot), its pace in the class (`rbr_season::car_pace`), and each stage's time factor for it, the
//! speed plan with the car's pace over the plan with the field's mean car.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use rbr_formats::car_specs::CarSpecs;
use rbr_formats::{car_pack, rbz};
use rbr_season::car_pace::{self, CarPace};
use rbr_season::config::StageRef;
use rbr_season::pace::ClassPace;

use super::pool;
use crate::catalog::{Car, Physics, PHYSICS_DIRS};

/// The car's specs from its tarmac scripts: a pack's files, or the archive slot's in NGP's `physics.rbz` at `archive`.
pub fn specs(car: &Car, archive: &Path) -> Option<CarSpecs> {
    let (common, surface) = match &car.physics {
        Physics::Pack(path) => {
            let pack = car_pack::Pack::open(path).ok()?;
            (pack.read("common.lsp").ok()?, pack.read("r_tarmac.lsp").ok()?)
        }
        Physics::Archive => {
            let mut zip = rbz::open(archive).ok()?;
            let dir = PHYSICS_DIRS.get(car.slot as usize)?;
            let mut read = |name: &str| {
                let mut bytes = Vec::new();
                let mut file = rbz::by_name(&mut zip, &format!("Physics/{dir}/{name}")).ok()?;
                file.read_to_end(&mut bytes).ok().map(|_| bytes)
            };
            (read("common.lsp")?, read("r_tarmac.lsp")?)
        }
    };
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    CarSpecs::parse(&text(&common), &text(&surface))
}

/// Each model's time factor on a stage: `factor(stage, car key)`, 1 for a model whose specs do not read.
pub fn factors(
    cache: &Path,
    class: ClassPace,
    models: &[String],
    cars: &[Car],
    archive: &Path,
) -> impl Fn(&StageRef, &str) -> f32 {
    let cars: HashMap<String, CarPace> = models
        .iter()
        .filter_map(|key| {
            let car = cars.iter().find(|c| c.key == *key)?;
            Some((key.clone(), CarPace::of(&specs(car, archive)?)))
        })
        .collect();
    let mean = car_pace::mean(&cars.values().copied().collect::<Vec<_>>());
    let cache = cache.to_path_buf();
    let known: RefCell<HashMap<(String, String), f32>> = RefCell::default();
    move |stage: &StageRef, car: &str| {
        let (Some(own), Some(mean)) = (cars.get(car), mean) else { return 1.0 };
        let key = (stage.key.clone(), car.to_owned());
        *known.borrow_mut().entry(key).or_insert_with(|| {
            let pace = |car| car_pace::for_car(class, car, mean);
            pool::profile(&cache, stage, &pace(*own)).dry / pool::profile(&cache, stage, &pace(mean)).dry
        })
    }
}
