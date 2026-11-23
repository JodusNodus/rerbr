//! The quick race's ghost car: the game's best and saved ghosts, by map number.

use std::path::{Path, PathBuf};

use crate::catalog;
use crate::times::{slug, Road};
use crate::Form;
use rbr_gameplay::weather::Weather;

/// The ghost car to race in a quick race: none, the best one kept per map, or a saved ghost of `./GhostCars/`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GhostChoice {
    #[default]
    Off,
    Best,
    File(String),
}

/// Where the ghosts are, in the player's folder (the game's `./GhostCars/`).
pub const GHOST_DIR: &str = "GhostCars";
/// The best ghost's name (ours: the game names a ghost when the player saves it).
pub const BEST_GHOST: &str = "Best";

impl GhostChoice {
    pub fn parse(text: &str) -> GhostChoice {
        match text {
            "" | "off" => GhostChoice::Off,
            "best" => GhostChoice::Best,
            file => GhostChoice::File(file.to_owned()),
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            GhostChoice::Off => "off".to_owned(),
            GhostChoice::Best => "best".to_owned(),
            GhostChoice::File(file) => file.clone(),
        }
    }

    /// The viewer's arguments: `--best-ghost` with the `best` file (`best_ghost`) and the weather words a
    /// new best keeps, or `--ghost` with the file of `./GhostCars/` (none when it is gone).
    pub fn viewer_args(&self, here: &Path, best: &Path, weather: &rbr_formats::ghost::Weather) -> Vec<String> {
        let dir = here.join(GHOST_DIR);
        match self {
            GhostChoice::Off => Vec::new(),
            GhostChoice::Best => vec![
                "--best-ghost".into(),
                best.display().to_string(),
                "--ghost-weather".into(),
                rbr_render::ghost::weather_arg(weather),
            ],
            GhostChoice::File(file) if dir.join(file).is_file() => {
                vec!["--ghost".into(), dir.join(file).display().to_string()]
            }
            GhostChoice::File(_) => Vec::new(),
        }
    }
}

/// The folder of the best ghosts in the player's folder `home` (ours).
pub const BEST_DIR: &str = "ghosts";

/// The best ghost of map `map` for a physics (`PhysicsId::text`), car class and road, one file per combination so a
/// ghost is only raced against runs it can be compared with: `<home>/ghosts/<physics>/<class>/<road>/Best_<map>.ghc`.
pub fn best_ghost(home: &Path, physics: &str, class: &str, road: Road, map: u32) -> PathBuf {
    home.join(BEST_DIR)
        .join(slug(physics))
        .join(slug(class))
        .join(road.word())
        .join(rbr_formats::ghost::SavedGhost::file_name(BEST_GHOST, map))
}

/// The weather words of the saved ghost `file` in `./GhostCars/` (its header only), or `None` when it does not read.
pub fn ghost_words(here: &Path, file: &str) -> Option<rbr_formats::ghost::Weather> {
    use std::io::Read;
    let mut header = Vec::new();
    let file = std::fs::File::open(here.join(GHOST_DIR).join(file)).ok()?;
    file.take(rbr_formats::ghost::SAVED_HEADER_LEN as u64).read_to_end(&mut header).ok()?;
    rbr_formats::ghost::SavedGhost::header_weather_words(&header)
}

/// A saved ghost's name in the Ghost dropdown: the old per-map best (`Best_<map>.ghc`, from before the best ghost was
/// kept per class and road) is no longer written and reads apart from the new Best.
pub fn ghost_name(file: &str) -> String {
    let name = file.trim_end_matches(".ghc");
    match name.rsplit_once('_') {
        Some((BEST_GHOST, map)) if map.bytes().all(|b| b.is_ascii_digit()) => format!("{BEST_GHOST} (before classes)"),
        _ => name.to_owned(),
    }
}

/// The saved ghosts of map `map` in `./GhostCars/` (`<name>_<map:02>.ghc`, the best one too), sorted.
pub fn ghost_files(here: &Path, map: u32) -> Vec<String> {
    let suffix = format!("_{map:02}.ghc");
    let mut files: Vec<String> = std::fs::read_dir(here.join(GHOST_DIR))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|f| f.ends_with(&suffix))
        .collect();
    files.sort();
    files
}

impl Form {
    /// The stage and weather of the chosen saved ghost: loading a `.ghc` sets the race's weather from its words
    /// (`E18_ConnGhostMemory::Dispatch` 0x4930d0). `None` without a file ghost, or when its words fit no season row of
    /// the map (then the Weather field decides).
    pub fn ghost_weather(&self) -> Option<(catalog::Stage, Weather)> {
        let GhostChoice::File(file) = &self.ghost else {
            return None;
        };
        let words = ghost_words(&self.records(), file)?;
        self.catalog.ghost_stage_weather(&self.season, self.catalog.maps[self.map].number, &words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_ghost_lists_its_weather_words() {
        let here = crate::test_dir("ghost-words");
        std::fs::create_dir_all(here.join(GHOST_DIR)).unwrap();
        let weather =
            rbr_formats::ghost::Weather { track_letter: 3, cloud: 2, wetness: 2, wear: 1, ..Default::default() };
        let ghost = rbr_formats::ghost::SavedGhost { map: 54, car: 6, weather, stream: Default::default() };
        std::fs::write(here.join(GHOST_DIR).join("me_54.ghc"), ghost.write()).unwrap();
        assert_eq!(ghost_words(&here, "me_54.ghc"), Some(weather));
        assert_eq!(ghost_words(&here, "gone_54.ghc"), None);
        assert_eq!(ghost_files(&here, 54), ["me_54.ghc"]);
        std::fs::remove_dir_all(&here).unwrap();
    }

    /// Each physics, class and road keeps its own best ghost file, apart from the old `./GhostCars/` one, which still
    /// lists as a saved ghost.
    #[test]
    fn the_best_ghost_is_kept_per_physics_class_and_road() {
        let home = Path::new("/p");
        let path = |physics, class, road| best_ghost(home, physics, class, road, 54);
        let base = path("ngp@7.4.778", "Rally 2", Road::Dry);
        assert_eq!(base, home.join("ghosts/ngp@7.4.778/Rally_2/dry/Best_54.ghc"));
        assert_ne!(base, path("experimental-brush@0.1.0", "Rally 2", Road::Dry));
        assert_ne!(base, path("ngp@7.4.778", "Group A", Road::Dry));
        assert_ne!(base, path("ngp@7.4.778", "Rally 2", Road::Wet));
        let words = rbr_formats::ghost::Weather::default();
        let args = GhostChoice::Best.viewer_args(home, &base, &words);
        assert_eq!(args[..2], ["--best-ghost".to_owned(), base.display().to_string()]);
        assert!(GhostChoice::Off.viewer_args(home, &base, &words).is_empty());

        let here = crate::test_dir("ghost-old-best");
        std::fs::create_dir_all(here.join(GHOST_DIR)).unwrap();
        std::fs::write(here.join(GHOST_DIR).join("Best_54.ghc"), b"").unwrap();
        assert_eq!(ghost_files(&here, 54), ["Best_54.ghc"]);
        assert_eq!(ghost_name("Best_54.ghc"), "Best (before classes)");
        assert_eq!(ghost_name("me_54.ghc"), "me_54");
        std::fs::remove_dir_all(&here).unwrap();
    }
}
