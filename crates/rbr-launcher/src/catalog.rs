//! The launcher's cars and maps. A map's good or bad quick-race weather, from the 1.02 exe's season table, chooses
//! its stage variant and the wetness used by the viewer. Maps without a good or bad row cannot start.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use rbr_formats::{car_pack, cars, rbz, rsf, stage_files};
use rbr_gameplay::weather::{self, Preset, Season, Weather};

use crate::car_packs::{self, StoredModel};

/// Each car number's folder in `physics.rbz` (the exe's table at 0x7c18d4).
const PHYSICS_DIRS: [&str; 8] = ["c_xsara", "h_accent", "mg_zr", "m_lancer", "p_206", "s_i2003", "t_coroll", "s_i2000"];

/// The car numbers NGP's `physics.rbz` at `archive` has NGP cars in: the slots whose `common.lsp` sets NGP's keys
/// (`*_NGP`); the others keep the stock data, which NGP's physics does not drive. NGP 7.4.778's own package has none
/// (its stock car files are retail's); RallySimFans' archive and our test archive (`ngp-data/physics.rbz`, NGP cars
/// installed into slots 2, 4, 5 and 6) have the slots an NGP car was installed into.
pub fn ngp_cars(archive: &Path) -> Vec<u32> {
    let Ok(mut zip) = rbz::open(archive) else { return Vec::new() };
    (0..8u32)
        .filter(|&car| {
            let mut text = String::new();
            let name = format!("Physics/{}/common.lsp", PHYSICS_DIRS[car as usize]);
            rbz::by_name(&mut zip, &name).is_ok_and(|mut file| file.read_to_string(&mut text).is_ok())
                && is_ngp_car(&text)
        })
        .collect()
}

/// Whether a car's `common.lsp` is NGP's: it sets NGP's keys.
fn is_ngp_car(common: &str) -> bool {
    common.contains("_NGP")
}

/// The cars NGP drives, of `cars` (the archive ones, the RSF ones and the added ones): the archive slots `ngp_cars`
/// has NGP data for, and every pack car. With RSF's cars, the archive slot 5 is left out: in RallySimFans' archive it
/// is the RSF car last picked, which the list already has.
pub fn ngp_offered(cars: &[Car], ngp_cars: &[u32]) -> Vec<Car> {
    let rsf = cars.iter().any(|c| matches!(c.physics, Physics::Pack(_)) && !c.added);
    let offered = |c: &&Car| {
        matches!(c.physics, Physics::Pack(_)) || ngp_cars.contains(&c.slot) && !(rsf && c.slot == car_pack::SLOT)
    };
    cars.iter().filter(offered).cloned().collect()
}

/// Where NGP reads a car's physics: the game's archive slot, or an NGP physics pack folder raced in slot 5.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Physics {
    #[default]
    Archive,
    Pack(PathBuf),
}

/// A car NGP drives: an archive slot's, or a pack car's raced in slot 5. The only real difference is where NGP reads
/// its physics; the game-side differences of slot 5 (NGP's `_Car5` keys, Pirelli tyres, the slot 5 repair rules, the
/// shared ghosts) ride along as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Car {
    /// Its name: an archive car's folder, upper case (`viewer --car`); a pack car's name.
    pub name: String,
    /// Its game's car number (`rbr-standalone --car`): an archive car's slot, a pack car's slot 5.
    pub slot: u32,
    pub physics: Physics,
    /// The model it is drawn and collided with: its `Cars.ini` entry, a retail stand-in applied with the reason when
    /// its own folder is not installed; or why there is none. An archive car's is its own entry.
    pub model: Result<car_pack::Model, String>,
    /// Where the model's `Cars\` files resolve: the RSF install, or the added cars' folder. None for an archive car.
    pub model_root: Option<PathBuf>,
    /// The key its setups, camera nudges, FFB sensitivities and ghosts are saved under: an archive car's folder,
    /// upper case; a pack car's `rsf-<id>` (`camera_prefs::car_key`).
    pub key: String,
    pub group: Option<String>,
    /// The words for its drive train (`FWD`, `AWD with torque split`), when the source names them.
    pub drive_train: Option<String>,
    /// Its FMOD bank, when it has one and it is installed. None for an archive car.
    pub bank: Option<PathBuf>,
    /// An added pack, not RSF's cache (display only).
    pub added: bool,
}

impl Default for Car {
    fn default() -> Car {
        Car {
            name: String::new(),
            slot: 0,
            physics: Physics::Archive,
            model: Err(String::new()),
            model_root: None,
            key: String::new(),
            group: None,
            drive_train: None,
            bank: None,
            added: false,
        }
    }
}

impl Car {
    /// The keys its setups are saved under (the game's slot files and the `Physics/<key>/Setups` folder), its own
    /// first. An archive car's is its number; a pack car's is its own key, then `5`: the setups saved before pack
    /// cars had keys of their own, offered to every pack car.
    pub fn setup_keys(&self) -> Vec<String> {
        match &self.physics {
            Physics::Pack(_) => vec![self.key.clone(), car_pack::SLOT.to_string()],
            Physics::Archive => vec![self.slot.to_string()],
        }
    }

    /// Why the car can't start (a pack car needs its pack and a model), or `None`.
    pub fn unavailable(&self) -> Option<String> {
        match &self.physics {
            Physics::Pack(_) => self.model.as_ref().err().cloned(),
            Physics::Archive => None,
        }
    }

    /// Its pack folder, when it is a pack car.
    pub fn pack(&self) -> Option<&Path> {
        match &self.physics {
            Physics::Pack(path) => Some(path),
            Physics::Archive => None,
        }
    }
}

/// The retail entries of the install at `install` (`Cars/Cars.ini`), for the models' retail stand-ins.
fn retail_entries(install: &Path) -> Result<Vec<cars::CarEntry>, String> {
    let cars_ini = install.join("Cars").join("Cars.ini");
    let text = std::fs::read(&cars_ini).map_err(|e| format!("{}: {e}", cars_ini.display()))?;
    cars::parse(&String::from_utf8_lossy(&text)).map_err(|e| e.to_string())
}

/// The pack cars of RSF's cache at `root` (an RSF install), raced in slot 5, their models checked against the
/// install's `Cars.ini` (`install`) for the retail stand-ins.
fn rsf_cars(install: &Path, root: &Path) -> Result<Vec<Car>, String> {
    let rsf = rsf::Install::open(root)?;
    let retail = retail_entries(install)?;
    Ok(rsf
        .cars
        .iter()
        .map(|car| {
            // The pack's files must be plain: `common.lsp` stands for them (we never decrypt RSF-protected files).
            let (path, model) = match rsf.pack(car) {
                Ok(pack) => {
                    let path = pack.path().to_owned();
                    let model = pack.read("common.lsp").and_then(|_| rsf.model_entry(car, &retail));
                    (path, model)
                }
                Err(_) => {
                    (root.join("rsfdata").join("cars").join(&car.pack), Err("not downloaded in RallySimFans".into()))
                }
            };
            Car {
                name: car.name.clone(),
                slot: car_pack::SLOT,
                physics: Physics::Pack(path),
                model,
                model_root: Some(root.to_owned()),
                key: format!("rsf-{}", car.id),
                group: car.group.clone(),
                drive_train: car.drive_train.clone(),
                bank: rsf.audio_bank(car),
                added: false,
            }
        })
        .collect())
}

/// The pack car the added row `stored` names, raced in slot 5, its model checked against the added cars' folder
/// (`cars`) for its own model and the install's `Cars.ini` (`retail`) for the retail stand-in.
fn added_car(cars: &Path, retail: &[cars::CarEntry], stored: &car_packs::StoredCar) -> Car {
    let root = cars.join("rsfdata").join("cars");
    let found = rbr_formats::cars::resolve(&root, &stored.pack)
        .or_else(|| rbr_formats::cars::resolve(&root, &format!("{}.zip", stored.pack)));
    let (path, model) = match found {
        Some(path) => {
            let model = car_pack::Pack::open(&path)
                .and_then(|pack| pack.read("common.lsp").map(|_| ()))
                .and_then(|_| added_model(cars, retail, stored));
            (path, model)
        }
        None => (root.join(&stored.pack), Err("not downloaded".into())),
    };
    Car {
        name: stored.name.clone(),
        slot: car_pack::SLOT,
        physics: Physics::Pack(path),
        model,
        model_root: Some(cars.to_owned()),
        key: format!("rsf-{}", stored.id),
        group: stored.group.clone(),
        drive_train: stored.drive_train.clone(),
        bank: stored.audio.as_deref().and_then(|a| rbr_formats::cars::resolve(cars, &format!("AudioFMOD\\{a}.bank"))),
        added: true,
    }
}

/// The model an added car is drawn and collided with: its own folder's entry, or the retail stand-in's with the
/// reason when its own folder is not installed.
fn added_model(
    cars: &Path,
    retail: &[cars::CarEntry],
    stored: &car_packs::StoredCar,
) -> Result<car_pack::Model, String> {
    let (folder, file) = match &stored.model {
        StoredModel::Files { folder, file, .. } => (folder.clone(), file.clone()),
        StoredModel::StandIn { slot } => {
            let retail =
                retail.iter().find(|e| e.index == *slot as usize).ok_or(format!("Cars.ini: no [Car{slot:02}]"))?;
            return Ok(car_pack::Model { entry: pack_entry(retail), fallback: Some(format!("retail car {slot}")) });
        }
    };
    let entry = car_pack::model_entry(&folder, &file);
    match rbr_formats::cars::resolve(cars, &entry.file_name) {
        Some(_) => Ok(car_pack::Model { entry, fallback: None }),
        None => {
            let retail =
                retail.iter().find(|e| e.index == car_pack::SLOT as usize).ok_or("Cars.ini: no [Car05]".to_owned())?;
            Ok(car_pack::Model {
                entry: pack_entry(retail),
                fallback: Some(format!("Cars/{folder} not installed, retail car {}", car_pack::SLOT)),
            })
        }
    }
}

/// The retail entry as the pack car's slot 5 entry.
fn pack_entry(retail: &cars::CarEntry) -> cars::CarEntry {
    cars::CarEntry { index: car_pack::SLOT as usize, ..retail.clone() }
}

/// A stage variant, `track-NN_X` (a community stage's: the path of its files, `rbr_formats::stage_files`), loaded for
/// the weather selected by the game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stage {
    pub name: String,
    pub map: u32,
    pub letter: char,
}

/// Where a selectable map comes from: the game, a RallySimFans stage folder
/// ([`Catalog::set_community_stages`]), or a BTB stage ([`crate::btb`], a stage of its own number).
#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    Retail,
    Community(rsf::Stage),
    Btb(crate::btb::Btb),
}

/// One selectable map and the stage variants installed for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Map {
    pub number: u32,
    pub stages: Vec<Stage>,
    pub origin: Origin,
}

impl Map {
    /// The community stage, when the map is one's.
    pub fn community(&self) -> Option<&rsf::Stage> {
        match &self.origin {
            Origin::Community(stage) => Some(stage),
            _ => None,
        }
    }

    /// The BTB stage, when the map is one's.
    pub fn btb(&self) -> Option<&crate::btb::Btb> {
        match &self.origin {
            Origin::Btb(btb) => Some(btb),
            _ => None,
        }
    }

    /// The retail map whose presentation the map takes (`rbr_render::relight::site`): a BTB stage's is
    /// `rbr_btb::stage::SKY_MAP`, any other map's its own number.
    pub fn sky_map(&self) -> u32 {
        match self.origin {
            Origin::Btb(_) => rbr_btb::stage::SKY_MAP,
            _ => self.number,
        }
    }

    /// Why the community or BTB stage cannot start.
    pub fn problem(&self) -> Option<&String> {
        match &self.origin {
            Origin::Community(stage) => stage.problem.as_ref(),
            Origin::Btb(btb) => btb.problem.as_ref(),
            Origin::Retail => None,
        }
    }

    /// The name the menus show: the game's, or the community stage's.
    pub fn name(&self) -> String {
        match &self.origin {
            Origin::Community(stage) => stage.name.clone(),
            Origin::Btb(btb) => btb.name.clone(),
            Origin::Retail => title(self.number).map_or_else(|| format!("Map {}", self.number), str::to_owned),
        }
    }

    /// The season that gives the map's weathers: the game's, a community stage's own (`Season::of_rsf`), or a BTB
    /// stage's (`Season::of_btb`: the rows of its sky map).
    pub fn season<'a>(&self, season: &'a Season) -> Cow<'a, Season> {
        match &self.origin {
            Origin::Community(stage) => Cow::Owned(Season::of_rsf(stage, None)),
            Origin::Btb(_) => Cow::Owned(Season::of_btb(season, self.number, self.sky_map())),
            Origin::Retail => Cow::Borrowed(season),
        }
    }

    /// The game's surface number of the map (0 gravel, 1 tarmac, 2 snow): [`surface_kind`], or the community stage's.
    pub fn surface(&self) -> u32 {
        match &self.origin {
            Origin::Btb(btb) => btb.surface_kind(),
            Origin::Community(stage) => match stage.surface {
                Some(rsf::Surface::Gravel) => 0,
                Some(rsf::Surface::Tarmac) => 1,
                Some(rsf::Surface::Snow) => 2,
                None => surface_kind(self.number),
            },
            Origin::Retail => surface_kind(self.number),
        }
    }
}

/// The tyre wear's names, new to worn (the settings file, `rbr-standalone --wear`, the Drive page).
pub const WEARS: [&str; 3] = ["new", "normal", "worn"];

/// Whether to use the game's computed tyre, or a tyre selected by the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TyreChoice {
    Auto,
    Fixed(u32),
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    /// The cars offered: those of `all_cars` NGP drives ([`Catalog::offer`]).
    pub cars: Vec<Car>,
    /// The retail cars and the RSF ones.
    pub all_cars: Vec<Car>,
    pub maps: Vec<Map>,
}

impl Catalog {
    /// Reads the install's `Cars/Cars.ini` and groups its `Maps/track-NN_X.lbs` files by map number.
    pub fn scan(install: &Path) -> Result<Catalog, String> {
        let cars_ini = install.join("Cars").join("Cars.ini");
        let text = std::fs::read(&cars_ini).map_err(|e| format!("{}: {e}", cars_ini.display()))?;
        let cars = cars::parse(&String::from_utf8_lossy(&text))
            .map_err(|e| e.to_string())?
            .iter()
            .filter_map(|e| {
                Some(Car {
                    name: car_folder(&e.file_name)?.to_uppercase(),
                    slot: e.index as u32,
                    model: Ok(car_pack::Model { entry: e.clone(), fallback: None }),
                    key: car_folder(&e.file_name)?.to_uppercase(),
                    ..Car::default()
                })
            })
            .collect();

        let maps_dir = install.join("Maps");
        let mut maps: BTreeMap<u32, Vec<Stage>> = BTreeMap::new();
        for file in file_names(&maps_dir) {
            let Some(name) = file.strip_suffix(".lbs") else {
                continue;
            };
            let Some((number, letter)) = stage_parts(name) else {
                continue;
            };
            maps.entry(number).or_default().push(Stage { name: name.to_owned(), map: number, letter });
        }
        let maps = maps
            .into_iter()
            .map(|(number, mut stages)| {
                stages.sort_by_key(|s| s.letter);
                Map { number, stages, origin: Origin::Retail }
            })
            .collect();
        Ok(Catalog { cars: Vec::new(), all_cars: cars, maps })
    }

    /// Replaces the RSF stages by the RallySimFans stage folders of `root` (`<root>/Maps/<id>-<name>/`,
    /// `rbr_formats::rsf`; none when it is empty), after the retail maps; one that does not load stays listed with its
    /// reason ([`Catalog::availability`]). A folder whose `Tracks<id>.ini` does not read, or whose id a retail map has,
    /// is reported and left out.
    pub fn set_community_stages(&mut self, root: &str) {
        self.maps.retain(|m| matches!(m.origin, Origin::Retail));
        if root.trim().is_empty() {
            return;
        }
        for stage in rsf::scan(Path::new(root.trim())) {
            let stage = match stage {
                Ok(stage) => stage,
                Err((folder, e)) => {
                    eprintln!("launcher: RSF {}: {e}", folder.display());
                    continue;
                }
            };
            if self.maps.iter().any(|m| m.number == stage.id) {
                eprintln!("launcher: RSF stage {} has a retail map's number; left out", stage.id);
                continue;
            }
            let stages =
                stage.variants.iter().map(|&l| Stage { name: stage.stage(l), map: stage.id, letter: l }).collect();
            self.maps.push(Map { number: stage.id, stages, origin: Origin::Community(stage) });
        }
        let taken: Vec<u32> = self.maps.iter().map(|m| m.number).collect();
        self.maps.extend(crate::btb::maps(Path::new(root.trim()), |n| taken.contains(&n)));
    }

    /// Replaces the pack cars by those of the RSF folder `dir` (none, with the reason on stderr, when it doesn't
    /// read; none for an empty `dir`), then the added cars of `added` (`car_packs::DIR`). A car both have (by pack
    /// folder or name) is the added one: the player downloaded it over RSF's, which is back once it is removed.
    pub fn set_community_cars(&mut self, install: &Path, dir: &str, added: &Path) {
        self.all_cars.retain(|c| matches!(c.physics, Physics::Archive));
        if !dir.trim().is_empty() {
            match rsf_cars(install, Path::new(dir.trim())) {
                Ok(cars) => self.all_cars.extend(cars),
                Err(e) => eprintln!("launcher: RSF cars: {e}"),
            }
        }
        let pack_name = |c: &Car| -> Option<String> {
            Some(c.pack()?.file_name()?.to_string_lossy().trim_end_matches(".zip").to_lowercase())
        };
        match retail_entries(install) {
            Ok(retail) => {
                let cars: Vec<Car> =
                    car_packs::list(added).into_iter().map(|s| added_car(added, &retail, &s)).collect();
                let added: Vec<(Option<String>, String)> =
                    cars.iter().map(|c| (pack_name(c), c.name.to_lowercase())).collect();
                let kept = |c: &Car| {
                    let (pack, name) = (pack_name(c), c.name.to_lowercase());
                    matches!(c.physics, Physics::Archive)
                        || !added.iter().any(|(p, n)| (p.is_some() && *p == pack) || *n == name)
                };
                self.all_cars.retain(kept);
                self.all_cars.extend(cars);
            }
            Err(e) => eprintln!("launcher: added cars: {e}"),
        }
    }

    /// Offers the cars NGP drives, with the archive's NGP slots `ngp_cars` ([`ngp_offered`]).
    pub fn offer(&mut self, ngp_cars: &[u32]) {
        self.cars = ngp_offered(&self.all_cars, ngp_cars);
    }

    /// The installed map with `number`.
    pub fn map(&self, number: u32) -> Option<&Map> {
        self.maps.iter().find(|m| m.number == number)
    }

    /// The maps list's row of `number`.
    pub fn map_index(&self, number: u32) -> Option<usize> {
        self.maps.iter().position(|m| m.number == number)
    }

    /// Resolves the game's selected row (the random weather's from `draws`) to the installed stage variant and its
    /// wetness.
    pub fn stage_weather(
        &self,
        season: &Season,
        map: u32,
        preset: Preset,
        draws: weather::Draws,
    ) -> Result<(Stage, Weather), String> {
        let entry = self.map(map);
        if let Some(problem) = entry.and_then(Map::problem) {
            return Err(problem.clone());
        }
        let season = entry.map_or(Cow::Borrowed(season), |m| m.season(season));
        let weather = weather::quick_race(&season, map, preset, draws)?;
        let stage = self.stage_of(map, &weather).ok_or_else(|| {
            format!("{} selected by {} weather has no installed stage file", weather.stage(), weather::name(preset))
        })?;
        Ok((stage, weather))
    }

    /// The stage and weather a saved ghost's weather `words` bring on `map` (`E18_ConnGhostMemory::Dispatch` sets the
    /// weather from the file; `Season::ghost_weather`), or `None` when the map's season has no row for them or the
    /// stage is not installed.
    pub fn ghost_stage_weather(
        &self,
        season: &Season,
        map: u32,
        words: &rbr_formats::ghost::Weather,
    ) -> Option<(Stage, Weather)> {
        let entry = self.map(map)?;
        if entry.community().is_some_and(|r| r.problem.is_some()) {
            return None;
        }
        let weather = entry.season(season).ghost_weather(map, words)?;
        Some((self.stage_of(map, &weather)?, weather))
    }

    /// The installed stage of `map` that `weather` loads (its letter).
    pub fn stage_of(&self, map: u32, weather: &Weather) -> Option<Stage> {
        let entry = self.map(map);
        let name = weather.stage();
        entry
            .and_then(|m| {
                m.stages
                    .iter()
                    .find(|s| s.map == map && s.letter == weather.letter_char() && stage_files::name(&s.name) == name)
            })
            .cloned()
    }

    /// The map is startable only if both original presets select installed stage variants.
    pub fn availability(&self, season: &Season, map: &Map) -> Result<(), String> {
        let mut reasons = Vec::new();
        for preset in [Preset::Good, Preset::Bad] {
            if let Err(reason) = self.stage_weather(season, map.number, preset, weather::Draws::default()) {
                reasons.push(reason);
            }
        }
        reasons.dedup();
        if reasons.is_empty() {
            Ok(())
        } else {
            Err(reasons.join("; "))
        }
    }

    pub fn startable_maps<'a>(&'a self, season: &Season) -> Vec<&'a Map> {
        self.maps.iter().filter(|map| self.availability(season, map).is_ok()).collect()
    }
}

/// The file names in `dir`, empty when it does not read.
fn file_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

/// The folder of a `Cars.ini` model path (`Cars\\xsara\\xsara.sgc` gives `xsara`).
fn car_folder(dos_path: &str) -> Option<&str> {
    dos_path.split(['\\', '/']).rev().nth(1)
}

/// `track-54_E` gives map 54, variant `E`.
pub fn stage_parts(name: &str) -> Option<(u32, char)> {
    let (number, letter) = name.strip_prefix("track-")?.split_once('_')?;
    let mut letters = letter.chars();
    match (number.parse().ok(), letters.next(), letters.next()) {
        (Some(map), Some(l), None) if l.is_ascii_uppercase() => Some((map, l)),
        _ => None,
    }
}

/// The game's surface number (`E14_4a1330`): 0 gravel, 1 tarmac, 2 snow.
pub fn surface_kind(map: u32) -> u32 {
    match map {
        10..=18 => 2,
        0x29..=0x31 | 0x49 | 0x55 | 0x56 | 0x5e => 1,
        _ => 0,
    }
}

/// A season row's weather word, as the Drive page labels it.
fn preset_word(preset: u32) -> &'static str {
    match preset {
        0 => "Crisp",
        1 => "Hazy",
        2 => "No rain",
        3 => "Light rain",
        4 => "Heavy rain",
        5 => "No snow",
        6 => "Light snow",
        7 => "Heavy snow",
        8 => "Light fog",
        9 => "Heavy fog",
        _ => "?",
    }
}

/// A season row's cloud word, as the Drive page labels it.
fn cloud_word(cloud: u32) -> &'static str {
    match cloud {
        0 => "clear",
        1 => "part cloud",
        2 => "light cloud",
        3 => "heavy cloud",
        _ => "?",
    }
}

/// The time of day a cloud letter starts at, as the Drive page labels it (M morning, N noon, E evening,
/// `rbr_gameplay::weather::start_clock`); another letter shows as itself.
fn time_word(cloud_letter: u32) -> String {
    match cloud_letter {
        0 => "morning".to_owned(),
        1 => "noon".to_owned(),
        2 => "evening".to_owned(),
        letter => ['M', 'N', 'E', 'O', 'S'].get(letter as usize).copied().unwrap_or('?').to_string(),
    }
}

/// A season row's Drive page label from its words, e.g. "Heavy fog, heavy cloud, morning (O)": the weather word,
/// the cloud, the time of day from the cloud letter and the track variant letter.
pub fn row_label(row: &weather::Row) -> String {
    let variant = ['M', 'N', 'E', 'O', 'S'].get(row.letter as usize).copied().unwrap_or('?');
    format!("{}, {}, {} ({variant})", preset_word(row.preset), cloud_word(row.cloud), time_word(row.cloud_letter))
}

/// The map's season rows with their Drive page labels: the game's own weathers for the stage, exactly as authored
/// (`Map::season`: retail `Season::load`, a community stage's own). Rows with identical words show once (the first
/// is kept). Each entry is the row's season-table index, its `TrackSettings.ini` section and its label.
pub fn environment_rows(season: &weather::Season, map: u32) -> Vec<(usize, String, String)> {
    let mut seen = std::collections::HashSet::new();
    let sky = season.sky.unwrap_or(map);
    season
        .rows_of(map)
        .filter(|&k| {
            let row = &season.rows[k];
            seen.insert((row.letter, row.cloud, row.cloud_letter, row.preset))
        })
        .filter_map(|k| {
            let row = &season.rows[k];
            let section = rsf::WeatherRow::section_name(sky, row.letter, row.cloud_letter, row.preset, row.cloud)?;
            Some((k, section, row_label(row)))
        })
        .collect()
}

/// The map's season row with the `TrackSettings.ini` section `section`, if it is the map's.
pub fn row_by_section(season: &weather::Season, map: u32, section: &str) -> Option<usize> {
    let sky = season.sky.unwrap_or(map);
    season.rows_of(map).find(|&k| {
        let row = &season.rows[k];
        rsf::WeatherRow::section_name(sky, row.letter, row.cloud_letter, row.preset, row.cloud).as_deref()
            == Some(section)
    })
}

pub fn tyre_label(tyre: u32) -> &'static str {
    match tyre {
        0 => "Tarmac dry",
        1 => "Tarmac damp",
        2 => "Tarmac wet",
        3 => "Gravel dry",
        4 => "Gravel damp",
        5 => "Gravel wet",
        6 => "Snow",
        _ => "Unknown tyre",
    }
}

pub fn map_label(map: &Map) -> String {
    match &map.origin {
        Origin::Community(stage) => format!("{} {} (RSF)", map.number, stage.name),
        Origin::Btb(btb) => format!("{} {} (BTB)", map.number, btb.name),
        Origin::Retail => match title(map.number) {
            Some(name) => format!("{} {name}", map.number),
            None => map.number.to_string(),
        },
    }
}

/// Whether the launcher lists `map`: an RSF stage or one of the 36 retail rally stages, not the rally school's and
/// test maps (`[HOMEBASE]`, `[RSSTAGE]`, `[RALLYYARD]` and the unnamed ones; user decision: no rally school). They
/// still start from the viewer's and `rbr-standalone`'s command lines.
pub fn is_listed(map: &Map) -> bool {
    !matches!(map.origin, Origin::Retail) || title(map.number).is_some_and(|name| !name.starts_with('['))
}

/// The stage's name in the game: `E52ModeHost::M4a1020` (0x4a1020), queried by map number.
pub fn title(map: u32) -> Option<&'static str> {
    Some(match map {
        10 => "Kaihuavaara",
        11 => "Mustaselka",
        12 => "Sikakama",
        13 => "Autiovaara",
        14 => "Kaihuavaara II",
        15 => "Mustaselka II",
        20 => "Harwood Forest",
        21 => "Falstone",
        22 => "Chirdonhead",
        23 => "Shepherds Shield",
        24 => "Harwood Forest II",
        25 => "Chirdonhead II",
        31 => "NewBobs",
        32 => "Greenhills",
        33 => "Mineshaft",
        34 => "East-West",
        35 => "NewBobs II",
        36 => "East-West II",
        41 => "Cote D'Arbroz",
        42 => "Joux Verte",
        43 => "Bisanne",
        44 => "Joux Plane",
        45 => "Joux Verte II",
        46 => "Cote D'Arbroz II",
        51 => "Noiker",
        52 => "Sipirkakim",
        53 => "Pirka Menoko",
        54 => "Tanner",
        55 => "Noiker II",
        56 => "Tanner II",
        61 => "Fraizer Wells",
        62 => "Prospect Ridge",
        63 => "Diamond Creek",
        64 => "Hualapai Nation",
        65 => "Prospect Ridge II",
        66 => "Diamond Creek II",
        71 | 74..=77 | 79 | 80 | 82 | 87 => "[RSSTAGE]",
        72 | 73 | 78 | 81 | 83..=86 | 88 | 89 => "[RALLYYARD]",
        18 | 28 | 39 | 49 | 59 | 69 => return None,
        _ => "[HOMEBASE]",
    })
}

pub fn car_label(car: &Car) -> String {
    let group = car.group.as_deref().unwrap_or("no group");
    match &car.physics {
        Physics::Pack(_) if car.added => format!("{} ({group}, added)", car.name),
        Physics::Pack(_) => {
            format!("{} ({group}, RSF {})", car.name, car.key.strip_prefix("rsf-").unwrap_or("?"))
        }
        Physics::Archive => format!("{} ({})", car.name, car.slot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbr_gameplay::weather::Row;

    /// NGP cars: none in the retail archive (`RBR_DATA`), 2, 4, 5 and 6 in our test archive (`RBR_NGP_PHYSICS`, NGP
    /// cars installed into those slots), and slot 5 only in a RallySimFans install, whose loose `Physics/` is its
    /// archive's tree (`RBR_RSF`).
    #[test]
    fn ngp_cars_come_from_the_archive() {
        let Some(data) = rbr_formats::data::Data::from_env() else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        assert_eq!(ngp_cars(&data.resolve("Root/physics.rbz").expect("retail physics.rbz")), Vec::<u32>::new());
        if let Some(ngp) = std::env::var_os("RBR_NGP_PHYSICS") {
            assert_eq!(ngp_cars(Path::new(&ngp)), [2, 4, 5, 6]);
        }
        if let Some(rsf) = std::env::var_os("RBR_RSF") {
            let common = |dir: &str| std::fs::read_to_string(Path::new(&rsf).join(format!("Physics/{dir}/common.lsp")));
            let slots: Vec<u32> =
                (0..8).filter(|&c| common(PHYSICS_DIRS[c as usize]).is_ok_and(|t| is_ngp_car(&t))).collect();
            assert_eq!(slots, [5]);
        }
    }

    /// An archive car's setups are its number's; a pack car's has its own key, then the old slot 5's.
    #[test]
    fn setup_keys_are_the_cars_own_with_slot_5_behind_a_pack_car() {
        let pack = |key: &str| Car {
            name: "Evo".into(),
            slot: car_pack::SLOT,
            physics: Physics::Pack("/p".into()),
            key: key.into(),
            ..Car::default()
        };
        let car = |slot| Car { slot, name: "Evo".into(), ..Car::default() };
        assert_eq!(car(6).setup_keys(), ["6"]);
        assert_eq!(pack("rsf-24").setup_keys(), ["rsf-24", "5"]);
        assert_eq!(pack("rsf-25").setup_keys(), ["rsf-25", "5"]);
    }

    /// NGP's archive slots are offered, the others hidden; with RSF cars the archive slot 5 is too.
    #[test]
    fn the_offered_cars_are_the_ones_ngp_drives() {
        let retail = |slot| Car { slot, name: format!("RETAIL{slot}"), ..Car::default() };
        let rsf = Car {
            slot: car_pack::SLOT,
            name: "RSF".into(),
            physics: Physics::Pack("/p".into()),
            key: "rsf-24".into(),
            ..Car::default()
        };
        let cars: Vec<Car> = (0..8).map(retail).collect();
        let names = |cars: Vec<Car>| cars.into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(ngp_offered(&cars, &[2, 4, 5, 6])), ["RETAIL2", "RETAIL4", "RETAIL5", "RETAIL6"]);
        assert!(ngp_offered(&cars, &[]).is_empty());
        let with_rsf: Vec<Car> = cars.into_iter().chain([rsf]).collect();
        assert_eq!(names(ngp_offered(&with_rsf, &[2, 4, 5, 6])), ["RETAIL2", "RETAIL4", "RETAIL6", "RSF"]);
    }

    use rbr_formats::data::Data;

    /// A car both RSF and the added cars have (by pack folder or by name) is listed once, the added one; the others
    /// are listed with their source. Needs `RBR_DATA` (its `Cars.ini`).
    #[test]
    fn an_added_car_rsf_has_is_listed_once() {
        let Some(install) = std::env::var_os("RBR_DATA").map(PathBuf::from) else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        let dir = crate::test_dir("catalog-added");
        let (rsf_root, added) = (dir.join("rsf"), dir.join("cars"));
        let cache = rsf_root.join("rsfdata").join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        for pack in ["Evo_IX_N4_ngp6", "F"] {
            let folder = rsf_root.join("rsfdata/cars").join(pack);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("common.lsp"), "x").unwrap();
        }
        let json = |rows: &[&str]| format!("[\n{}\n]\n", rows.join(",\n"));
        std::fs::write(cache.join("cargroups.json"), "[]").unwrap();
        std::fs::write(
            cache.join("carmodels.json"),
            json(&["{\"id\":1,\"name\":\"m\",\"path\":\"m\",\"filename\":\"m\",\"fallback\":5}"]),
        )
        .unwrap();
        std::fs::write(
            cache.join("cars.json"),
            json(&[
                "{\"id\":24,\"name\":\"Mitsubishi Lancer Evo IX N4\",\"path\":\"Evo_IX_N4_ngp6\",\"carmodel_id\":1}",
                "{\"id\":81,\"name\":\"Ford Fiesta Rally2\",\"path\":\"F\",\"carmodel_id\":1}",
            ]),
        )
        .unwrap();
        std::fs::write(cache.join("cars_data.json"), "[]").unwrap();
        let stored = |id, name: &str, pack: &str| crate::car_packs::StoredCar {
            id,
            name: name.into(),
            pack: pack.into(),
            group: None,
            drive_train: None,
            revision: String::new(),
            audio: None,
            model: crate::car_packs::StoredModel::Files { folder: "m".into(), file: "m".into(), name: "m".into() },
        };
        crate::car_packs::write_cars(
            &added,
            &[
                stored(1, "Evo IX N4 (gvrc)", "evo_ix_n4_NGP6"),
                stored(2, "Ford Fiesta Rally2", "Ford_Fiesta_Rally2_ngp7"),
                stored(3, "Alpine A110", "Alpine"),
            ],
        )
        .unwrap();
        let mut catalog = Catalog::default();
        catalog.set_community_cars(&install, &rsf_root.display().to_string(), &added);
        let listed: Vec<(String, bool)> = catalog.all_cars.iter().map(|c| (c.name.clone(), c.added)).collect();
        let want = [("Evo IX N4 (gvrc)", true), ("Ford Fiesta Rally2", true), ("Alpine A110", true)];
        assert_eq!(listed, want.map(|(n, a)| (n.to_owned(), a)));
        assert_eq!(car_label(&catalog.all_cars[2]), "Alpine A110 (no group, added)");
        // Without RSF the added ones; the retail slot 5 stays offered (it is not RSF's last pick).
        catalog.set_community_cars(&install, " ", &added);
        assert_eq!(catalog.all_cars.len(), 3);
        let retail = Car { slot: 5, name: "RETAIL5".into(), ..Car::default() };
        catalog.all_cars.insert(0, retail);
        catalog.offer(&[5]);
        assert_eq!(catalog.cars.len(), 4);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A BTB stage offers the rows of map 41 (every variant, good and bad) under its own number, one stage file (M)
    /// for all of them, and a saved ghost finds its row again.
    #[test]
    fn a_btb_map_plays_map_41s_rows_under_its_own_number() {
        let retail = Season {
            rows: vec![
                Row { map: 41, letter: 0, cloud: 0, cloud_letter: 0, preset: 0, wetness: 0 },
                Row { map: 41, letter: 3, cloud: 3, cloud_letter: 2, preset: 4, wetness: 2 },
                Row { map: 54, letter: 0, cloud: 0, cloud_letter: 0, preset: 0, wetness: 0 },
            ],
            nice: vec![0],
            bad: vec![1],
            sky: None,
        };
        let number = 1_234_567;
        let btb = crate::btb::Btb {
            folder: "/rsf/Tracks/Hokkara".into(),
            name: "Hokkara".into(),
            author: None,
            km: None,
            surface: "gravel",
            problem: None,
        };
        let stage = Stage { name: format!("/rsf/Tracks/Hokkara/track-{number}_M"), map: number, letter: 'M' };
        let map = Map { number, stages: vec![stage.clone()], origin: Origin::Btb(btb) };
        let catalog = Catalog { maps: vec![map], ..Catalog::default() };
        for (preset, row, wetness) in [(Preset::Good, 0, 0), (Preset::Bad, 1, 2)] {
            let (found, weather) = catalog.stage_weather(&retail, number, preset, weather::Draws::default()).unwrap();
            assert_eq!((found, weather.row, weather.wetness), (stage.clone(), Some(row), wetness));
        }
        let season = catalog.maps[0].season(&retail);
        let rows = environment_rows(&season, number);
        let sections: Vec<&str> = rows.iter().map(|(_, section, _)| section.as_str()).collect();
        assert_eq!(sections, ["41M_crisp_clear", "41O_E_heavyrain_heavycloud"]);
        let (_, weather) = catalog.stage_weather(&retail, number, Preset::Bad, weather::Draws::default()).unwrap();
        let words = weather.ghost_words(&season);
        assert_eq!(catalog.ghost_stage_weather(&retail, number, &words), Some((stage, weather)));
    }

    /// The Environment rows are the map's season rows with readable labels, deduped by words.
    #[test]
    fn environment_rows_label_and_dedupe() {
        let season = Season {
            rows: vec![
                Row { map: 54, letter: 3, cloud: 3, cloud_letter: 2, preset: 9, wetness: 2 },
                Row { map: 54, letter: 0, cloud: 1, cloud_letter: 0, preset: 2, wetness: 0 },
                // The same words as the first row (another wetness): shown once.
                Row { map: 54, letter: 3, cloud: 3, cloud_letter: 2, preset: 9, wetness: 0 },
                Row { map: 33, letter: 0, cloud: 0, cloud_letter: 0, preset: 0, wetness: 0 },
            ],
            nice: vec![1],
            bad: vec![0],
            sky: None,
        };
        assert_eq!(row_label(&season.rows[0]), "Heavy fog, heavy cloud, evening (O)");
        assert_eq!(row_label(&season.rows[1]), "No rain, part cloud, morning (M)");
        assert_eq!(row_label(&season.rows[3]), "Crisp, clear, morning (M)");
        let rows = environment_rows(&season, 54);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].0, rows[0].1.as_str()), (0, "54O_E_heavyfog_heavycloud"));
        assert_eq!((rows[1].0, rows[1].1.as_str()), (1, "54M_norain_partcloud"));
        assert_eq!(rows[0].2, "Heavy fog, heavy cloud, evening (O)");
        assert_eq!(row_by_section(&season, 54, &rows[0].1), Some(0));
        assert_eq!(row_by_section(&season, 54, "54O_E_heavyfog_heavycloud"), Some(0));
        // Another map's section is not this map's, and map 4 has no rows.
        assert_eq!(row_by_section(&season, 33, &rows[0].1), None);
        assert_eq!(row_by_section(&season, 54, "nope"), None);
        assert!(environment_rows(&season, 4).is_empty());
    }

    #[test]
    fn names_parse() {
        assert_eq!(car_folder("Cars\\Lancer_EVO_VII\\lancer_evo_vII.sgc"), Some("Lancer_EVO_VII"));
        assert_eq!(stage_parts("track-54_E"), Some((54, 'E')));
        assert_eq!(stage_parts("track-4_O"), Some((4, 'O')));
        assert_eq!(stage_parts("track-54_E_textures"), None);
        assert_eq!(stage_parts("Clouds"), None);
    }

    #[test]
    fn every_retail_map_starts_with_good_and_bad_or_has_a_reason() {
        let Some(install) = std::env::var_os("RBR_DATA").map(PathBuf::from) else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        let patch = std::env::var_os("RBR_PATCH").map(PathBuf::from);
        let season = Season::load(&Data::new(&install, patch)).unwrap();
        let catalog = Catalog::scan(&install).unwrap();
        let startable = catalog.startable_maps(&season);
        let unavailable: Vec<(u32, String)> = catalog
            .maps
            .iter()
            .filter_map(|map| catalog.availability(&season, map).err().map(|why| (map.number, why)))
            .collect();
        println!("launcher: {}/{} retail maps startable", startable.len(), catalog.maps.len());
        for (map, reason) in &unavailable {
            println!("launcher: map {map}: {reason}");
        }
        assert_eq!(catalog.maps.len(), 58, "retail maps changed");
        assert_eq!(startable.len(), 58);
        assert_eq!(unavailable, []);
    }

    #[test]
    fn every_random_row_of_a_startable_map_has_its_stage_installed() {
        let Some(install) = std::env::var_os("RBR_DATA").map(PathBuf::from) else {
            eprintln!("skipping: RBR_DATA not set");
            return;
        };
        let patch = std::env::var_os("RBR_PATCH").map(PathBuf::from);
        let season = Season::load(&Data::new(&install, patch)).unwrap();
        let catalog = Catalog::scan(&install).unwrap();
        let missing: Vec<String> = catalog
            .startable_maps(&season)
            .iter()
            .flat_map(|map| season.rows_of(map.number).map(move |row| (map.number, row)))
            .filter_map(|(map, row)| {
                let draws = weather::Draws { row: Some(row), wear: 1 };
                catalog.stage_weather(&season, map, Preset::Random, draws).err()
            })
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    /// A community stage with one broken variant offers the others' weathers, and is unavailable only when none work.
    #[test]
    fn a_broken_variant_hides_only_its_own_weathers() {
        let root = std::env::temp_dir().join(format!("rbr-catalog-{}", std::process::id()));
        let folder = root.join("Maps").join("777-Test Stage");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("Tracks777.ini"), "[Map777]\r\nTrackName=\"Maps\\777-Test Stage\\track-777\"\r\n")
            .unwrap();
        std::fs::write(
            folder.join("TrackSettings777.ini"),
            "[777O_E_crisp_clear]\r\nFogEnd = 5\r\n[777M_E_crisp_clear]\r\nFogEnd = 5\r\n",
        )
        .unwrap();
        for letter in ['O', 'M'] {
            for ext in ["lbs", "col", "trk", "dls", "mat", "ini"] {
                std::fs::write(folder.join(format!("track-777_{letter}.{ext}")), b"plain").unwrap();
            }
        }
        let mut catalog = Catalog::default();
        let base = Season { rows: Vec::new(), nice: Vec::new(), bad: Vec::new(), sky: None };
        let check = |catalog: &mut Catalog| {
            catalog.set_community_stages(root.to_str().unwrap());
            let map = catalog.map(777).unwrap().clone();
            let rows = environment_rows(&map.season(&base), 777);
            let picks: Vec<_> = [Preset::Good, Preset::Bad, Preset::Random]
                .map(|p| {
                    catalog
                        .stage_weather(&base, 777, p, weather::Draws { row: Some(0), wear: 1 })
                        .map(|(s, _)| s.letter)
                })
                .into();
            (catalog.availability(&base, &map), rows.len(), picks)
        };
        let (available, rows, picks) = check(&mut catalog);
        assert_eq!((available, rows), (Ok(()), 2));
        assert!(picks.iter().all(Result::is_ok));
        std::fs::write(folder.join("track-777_O.col"), [1u8, 0xa0, 0, 0, 9]).unwrap();
        let (available, rows, picks) = check(&mut catalog);
        assert_eq!((available, rows), (Ok(()), 1));
        assert!(picks.iter().all(|p| *p == Ok('M')));
        std::fs::write(folder.join("track-777_M.col"), [1u8, 0xa0, 0, 0, 9]).unwrap();
        assert!(check(&mut catalog).0.is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Setting the RSF folder again leaves the same stages; clearing it leaves the retail ones.
    #[test]
    fn set_community_stages_replaces_the_community_stages() {
        let (Some(install), Some(rsf)) = (std::env::var_os("RBR_DATA"), std::env::var("RBR_RSF").ok()) else {
            eprintln!("skipping: RBR_DATA or RBR_RSF not set");
            return;
        };
        let mut catalog = Catalog::scan(Path::new(&install)).unwrap();
        let retail = catalog.maps.len();
        catalog.set_community_stages(&rsf);
        let with_rsf = catalog.maps.len();
        assert!(with_rsf > retail, "no RSF stages in {rsf}");
        catalog.set_community_stages(&rsf);
        assert_eq!(catalog.maps.len(), with_rsf);
        catalog.set_community_stages(" ");
        assert_eq!(catalog.maps.len(), retail);
    }
}
